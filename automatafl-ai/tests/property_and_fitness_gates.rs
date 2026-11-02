//! Property-based (`proptest`) invariant suites and tactical/tournament fitness gates
//! for `automatafl-ai`.

use std::time::Instant;

use automatafl_ai::*;
use proptest::prelude::*;

fn arb_game_config() -> impl Strategy<Value = GameConfig> {
    (
        prop_oneof![
            Just(BoardPreset::Mini5x5),
            Just(BoardPreset::Blitz7x7),
            Just(BoardPreset::Tactical9x9),
            Just(BoardPreset::Classic11x11),
        ],
        prop_oneof![
            Just(TieBreakMode::Column),
            Just(TieBreakMode::Row),
            Just(TieBreakMode::Freeze),
            Just(TieBreakMode::Momentum {
                prefer_same_axis: true
            }),
            Just(TieBreakMode::Momentum {
                prefer_same_axis: false
            }),
        ],
        prop_oneof![
            Just(ConfluenceMode::Conflict),
            Just(ConfluenceMode::BunchBefore),
            Just(ConfluenceMode::Annihilate),
        ],
        any::<bool>(),
        0u8..=3u8,
        any::<bool>(),
        1u8..=2u8,
    )
        .prop_map(
            |(
                preset,
                tie_break,
                confluence,
                two_cycle_swaps,
                cooldown_turns,
                railgun_fling,
                automata_count,
            )| GameConfig {
                preset,
                tie_break,
                confluence,
                two_cycle_swaps,
                cooldown_turns,
                railgun_fling,
                automata_count,
                max_turns: 24,
            },
        )
}

fn arb_round_state_and_config() -> impl Strategy<Value = (PackedRoundState, GameConfig, PackedGoals)>
{
    arb_game_config().prop_flat_map(|cfg| {
        let sz = cfg.preset.size();
        let n_cells = (sz as usize) * (sz as usize);
        (
            Just(cfg),
            0u8..sz,
            0u8..sz,
            prop::collection::vec((0usize..n_cells, 1u8..=2u8), 4..=18),
            prop::collection::vec(0usize..n_cells, 0..=8),
            prop::collection::vec((0usize..n_cells, 1u8..=2u8), 0..=6),
            1u8..=0b1111u8,
            0i8..=4i8,
        )
            .prop_map(
                move |(cfg, ax, ay, pieces, mark_idxs, cd_entries, waiting_mask, dir_code)| {
                    let col_rule = !matches!(cfg.tie_break, TieBreakMode::Freeze);
                    let mut board = PackedBoard::empty(sz, ax, ay, col_rule);
                    let (last_dx, last_dy) = match dir_code {
                        1 => (1, 0),
                        2 => (-1, 0),
                        3 => (0, 1),
                        4 => (0, -1),
                        _ => (0, 0),
                    };
                    board.last_dx = last_dx;
                    board.last_dy = last_dy;

                    let auto_idx = board.idx(PackedCoord::new(ax, ay));
                    for (idx, code) in pieces {
                        if idx != auto_idx {
                            board.place_idx(idx, code);
                        }
                    }

                    let mut marks = BitBoard128::EMPTY;
                    for m_idx in mark_idxs {
                        if m_idx != auto_idx {
                            marks.set(m_idx);
                        }
                    }

                    for (c_idx, rem) in cd_entries {
                        if c_idx < n_cells
                            && !board.cooldown_mask.get(c_idx)
                            && (board.cooldown_len as usize) < MAX_COOLDOWNS
                        {
                            let slot = board.cooldown_len as usize;
                            board.cooldown_cells[slot] = c_idx as u8;
                            board.cooldown_rem[slot] = rem;
                            board.cooldown_mask.set(c_idx);
                            board.cooldown_len += 1;
                        }
                    }

                    let rs = PackedRoundState::open_with_mask(board, waiting_mask);
                    let mut rs_with_marks = rs;
                    rs_with_marks.marks = marks;
                    let goals = if waiting_mask > 0b11 {
                        PackedGoals::stock_four_player(sz)
                    } else {
                        PackedGoals::stock_two_player(sz)
                    };
                    (rs_with_marks, cfg, goals)
                },
            )
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 24,
        max_shrink_iters: 0,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_all_bots_legality_seat_tag_determinism_and_executable_preference(
        (rs, cfg, goals) in arb_round_state_and_config(),
        seed in any::<u64>(),
    ) {
        let state_bytes = rs.to_packed_bytes_with_goals(&cfg, &goals);

        // Compute whether any executable move from an unmarked, uncooled movable piece exists:
        let candidate_sources = rs
            .board
            .movable_pieces()
            .without(rs.marks)
            .without(rs.board.cooldown_mask)
            .without(rs.board.automata);
        let mut any_exec_from_movable = false;
        for src_idx in candidate_sources.iter_ones() {
            let frm = rs.board.coord_of(src_idx);
            let (_, exec_mask) = rs.legal_target_masks(0, frm);
            if !exec_mask.is_empty() {
                any_exec_from_movable = true;
                break;
            }
        }

        // Fast search configs for property fuzzing
        let test_seat = (0..4u8).find(|&s| rs.is_waiting(s)).unwrap_or(0);
        let inactive_seat = (0..4u8).find(|&s| !rs.is_waiting(s));

        for kind in [
            BotKind::Greedy,
            BotKind::Minimax,
            BotKind::DecoupledUct,
            BotKind::RegretMatching,
        ] {
            let make_bot = |s: u64| -> Box<dyn SimultaneousBot> {
                match kind {
                    BotKind::Greedy => Box::new(GreedyHeuristicBot::new(s)),
                    BotKind::Minimax => Box::new(SimultaneousMinimaxBot::new(
                        MinimaxConfig {
                            max_depth: 1,
                            root_branching: 8,
                            inner_branching: 4,
                            tie_epsilon: 1e-4,
                        },
                        s,
                    )),
                    BotKind::DecoupledUct => Box::new(DecoupledUctBot::new(
                        DuctConfig {
                            num_simulations: 24,
                            exploration_constant: 1.35,
                            max_actions_per_seat: 8,
                            rollout_depth: 2,
                            rollout_epsilon: 0.25,
                        },
                        s,
                    )),
                    BotKind::RegretMatching => Box::new(RegretMatchingBot::new(
                        RegretConfig {
                            candidate_count: 8,
                            iterations: 40,
                            support_threshold: 0.02,
                            one_ply_lookahead: false,
                        },
                        s,
                    )),
                }
            };

            if let Some(non_wait) = inactive_seat {
                let mut b_inactive = make_bot(seed);
                prop_assert!(
                    b_inactive.choose_move(&rs, &goals, &cfg, non_wait).is_none(),
                    "{kind:?} must return None for non-waiting seat {non_wait}"
                );
            }

            let mut bot1 = make_bot(seed);
            let mut bot2 = make_bot(seed);
            let mv1 = bot1.choose_move(&rs, &goals, &cfg, test_seat);
            let mv2 = bot2.choose_move(&rs, &goals, &cfg, test_seat);
            prop_assert_eq!(mv1, mv2, "{:?} must be deterministic for identical seed", kind);

            if let Some(mv) = mv1 {
                prop_assert_eq!(mv.who, test_seat, "{:?} emitted wrong seat tag", kind);
                prop_assert!(
                    rs.board.move_legal(rs.marks, mv),
                    "{:?} emitted illegal move {:?}",
                    kind,
                    mv
                );

                let (prop_mask, exec_mask) = rs.legal_target_masks(test_seat, mv.frm);
                let to_idx = rs.board.idx(mv.to);
                prop_assert!(
                    prop_mask.get(to_idx),
                    "{kind:?} destination {mv:?} not in Lean proposable target mask"
                );
                if any_exec_from_movable {
                    let src_idx = rs.board.idx(mv.frm);
                    prop_assert!(
                        candidate_sources.get(src_idx),
                        "{kind:?} failed to pick a movable piece when executable moves exist: {mv:?}"
                    );
                    prop_assert!(
                        exec_mask.get(to_idx),
                        "{kind:?} failed to pick an executable destination when executable moves exist: {mv:?}"
                    );
                }
            }
        }

        // Also verify C-ABI `automatafl_client_choose_move` determinism on Greedy (bot_kind = 0)
        let mut out_a = [0u8; 5];
        let mut out_b = [0u8; 5];
        let ok_a = unsafe {
            automatafl_client_choose_move(
                state_bytes.as_ptr(),
                state_bytes.len(),
                0,
                test_seat,
                seed,
                out_a.as_mut_ptr(),
            )
        };
        let ok_b = unsafe {
            automatafl_client_choose_move(
                state_bytes.as_ptr(),
                state_bytes.len(),
                0,
                test_seat,
                seed,
                out_b.as_mut_ptr(),
            )
        };
        prop_assert_eq!(ok_a, ok_b);
        if ok_a == 1 {
            prop_assert_eq!(out_a, out_b);
            prop_assert_eq!(out_a[0], test_seat);
        }
    }

    #[test]
    fn prop_mate_in_1_and_save_in_1_tactical_fitness(
        use_right_corner in any::<bool>(),
        seat in 0u8..=1u8,
        seed in any::<u64>(),
    ) {
        // Construct a 7x7 tactical Mate-in-1 board for `seat`:
        // - Goal corners for Seat 0 are (0, 0) and (6, 0).
        // - Goal corners for Seat 1 are (0, 6) and (6, 6).
        let sz = 7u8;
        let _goal_x = if use_right_corner { sz - 1 } else { 0 };
        let goal_y = if seat == 0 { 0 } else { sz - 1 };
        // Place the automaton 1 step horizontally away from the target corner:
        // e.g. at (1, goal_y) if goal_x == 0, or (5, goal_y) if goal_x == 6.
        let auto_x = if use_right_corner { sz - 2 } else { 1 };
        let auto_y = goal_y;
        // Place a single Repulsor that can slide onto (auto_x -/+ 1, auto_y) to shove the automaton
        // directly into `(goal_x, goal_y)`!
        let shove_x = if use_right_corner { sz - 3 } else { 2 };
        let rep_start_y = 3u8;

        let mut board = PackedBoard::empty(sz, auto_x, auto_y, true);
        board.place(PackedCoord::new(shove_x, rep_start_y), REP);

        let goals = PackedGoals::stock_two_player(sz);
        let cfg = GameConfig {
            preset: BoardPreset::Blitz7x7,
            ..GameConfig::default()
        };
        // Only `seat` is waiting (re-entry sub-round or solo tactical check) so the mate-in-1 is forced:
        let rs = PackedRoundState::open_with_mask(board, 1u8 << seat);

        let winning_move = PackedMove::new(
            seat,
            PackedCoord::new(shove_x, rep_start_y),
            PackedCoord::new(shove_x, auto_y),
        );
        match rs.round_step(&cfg, &goals, &[winning_move]) {
            PackedRoundOutcome::Resolved { win, .. } => {
                prop_assert_eq!(win, Some(seat), "constructed winning_move must win in 1 turn");
            }
            other => prop_assert!(false, "expected Resolved win, got {other:?}"),
        }

        for kind in [
            BotKind::Greedy,
            BotKind::Minimax,
            BotKind::DecoupledUct,
            BotKind::RegretMatching,
        ] {
            let mut bot = BotEngine::new(kind, seed);
            let chosen = bot
                .choose_move(&rs, &goals, &cfg, seat)
                .expect("bot must find a move in mate-in-1 position");
            match rs.round_step(&cfg, &goals, &[chosen]) {
                PackedRoundOutcome::Resolved { win, .. } => {
                    prop_assert_eq!(
                        win,
                        Some(seat),
                        "{:?} failed to play Mate-in-1 winning move (chose {:?}, expected {:?})",
                        kind,
                        chosen,
                        winning_move
                    );
                }
                other => prop_assert!(
                    false,
                    "{:?} failed to resolve Mate-in-1: got {:?}",
                    kind,
                    other
                ),
            }
        }
    }
}

#[test]
fn bot_tournament_multi_variant_fitness_and_bounded_latency_gate() {
    // Seeded random-legal baseline bot for tournament fitness comparison
    struct RandomLegalBot {
        rng: SplitMix64,
    }
    impl SimultaneousBot for RandomLegalBot {
        fn choose_move(
            &mut self,
            rs: &PackedRoundState,
            _goals: &PackedGoals,
            cfg: &GameConfig,
            seat: u8,
        ) -> Option<PackedMove> {
            if !rs.is_waiting(seat) {
                return None;
            }
            let mut list = MoveList::new();
            rs.board
                .generate_tactical_moves(rs.marks, rs.locked_slice(), seat, cfg, &mut list);
            if list.is_empty() {
                return None;
            }
            let idx = self.rng.next_usize(list.len());
            Some(list.as_slice()[idx])
        }
    }

    let configs = [
        GameConfig {
            preset: BoardPreset::Mini5x5,
            max_turns: 14,
            ..GameConfig::default()
        },
        GameConfig {
            preset: BoardPreset::Blitz7x7,
            confluence: ConfluenceMode::BunchBefore,
            two_cycle_swaps: true,
            cooldown_turns: 1,
            max_turns: 14,
            ..GameConfig::default()
        },
    ];

    let mut search_wins_or_leads = 0usize;
    let mut total_games = 0usize;

    for (c_idx, cfg) in configs.iter().enumerate() {
        for seed in 0..4u64 {
            let mut mm = SimultaneousMinimaxBot::new(
                MinimaxConfig {
                    max_depth: 1,
                    root_branching: 10,
                    inner_branching: 6,
                    tie_epsilon: 1e-4,
                },
                100 + seed + (c_idx as u64) * 50,
            );
            let mut rnd = RandomLegalBot {
                rng: SplitMix64::new(900 + seed + (c_idx as u64) * 50),
            };
            let t0 = Instant::now();
            let rep = play_simultaneous_match(&mut mm, &mut rnd, cfg);
            let elapsed = t0.elapsed();
            assert!(
                elapsed.as_secs_f64() < 15.0,
                "match took too long ({elapsed:?})"
            );
            total_games += 1;
            if rep.adjudicated_winner == Some(0) {
                search_wins_or_leads += 1;
            }
        }
    }

    assert!(
        search_wins_or_leads * 2 >= total_games,
        "SimultaneousMinimaxBot should win/lead at least 50% of matches vs RandomLegalBot, got {search_wins_or_leads}/{total_games}"
    );
}
