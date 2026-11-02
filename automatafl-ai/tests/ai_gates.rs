//! Verification gates for `automatafl-ai`:
//! 1. Canonical `AutomataflRules.lean` conformance on `PackedBoard` (D1–D6, Ruling D, `winOnEntry`, `adjudicateCapped`, R2 variants).
//! 2. Zero Move Leakage gate (uncommitted moves in `Game::pending_moves` never leak to bots).
//! 3. Zero Duplicate Moves gate (bots never duplicate a seat's move for another seat).
//! 4. Conflict Re-entry gate (all search engines respect `marks` and `locked` on `.again` sub-rounds).
//! 5. Full bot-vs-bot match completion across Decoupled UCT, Minimax, and Regret Matching.
//! 6. Exploratory game-theoretic symmetry & allocation-free throughput verification.

use automatafl_ai::*;

#[test]
fn packed_board_canonical_conformance_d1_to_d6() {
    let cfg = GameConfig::default();

    // D1: Inclusive destination path check (stationary piece on destination blocks move and is NOT overwritten)
    let mut b_d1 = PackedBoard::empty(5, 2, 2, true);
    let c00 = PackedCoord::new(0, 0);
    let c02 = PackedCoord::new(0, 2);
    b_d1.place(c00, ATT);
    b_d1.place(c02, REP);
    let (res_d1, unres_d1) = b_d1.resolve_moves(&[PackedMove::new(0, c00, c02)], &cfg);
    assert!(unres_d1.is_empty());
    assert_eq!(res_d1.cell_at(c00), ATT, "D1: blocked mover must remain at origin");
    assert_eq!(res_d1.cell_at(c02), REP, "D1: destination piece must not be overwritten");

    // D2: 2-cycle stays put under canonical rules (`two_cycle_swaps = false`), swaps when enabled
    let mut b_d2 = PackedBoard::empty(5, 4, 4, true);
    let c11 = PackedCoord::new(1, 1);
    let c21 = PackedCoord::new(2, 1);
    b_d2.place(c11, ATT);
    b_d2.place(c21, REP);
    let moves_2cyc = [PackedMove::new(0, c11, c21), PackedMove::new(1, c21, c11)];
    let (res_d2_stay, _) = b_d2.resolve_moves(&moves_2cyc, &cfg);
    assert_eq!(res_d2_stay.cell_at(c11), ATT, "D2: 2-cycle must stay put by default");
    assert_eq!(res_d2_stay.cell_at(c21), REP, "D2: 2-cycle must stay put by default");

    let cfg_swap = GameConfig {
        two_cycle_swaps: true,
        ..cfg
    };
    let (res_d2_swap, _) = b_d2.resolve_moves(&moves_2cyc, &cfg_swap);
    assert_eq!(res_d2_swap.cell_at(c11), REP, "D2 variant: 2-cycle swaps when enabled");
    assert_eq!(res_d2_swap.cell_at(c21), ATT, "D2 variant: 2-cycle swaps when enabled");

    // D3: Move from empty square directly back to source square (vacuum 2-cycle) stays put
    let mut b_d3 = PackedBoard::empty(5, 4, 4, true);
    b_d3.place(c00, ATT);
    let moves_d3 = [PackedMove::new(0, c00, c02), PackedMove::new(1, c02, c00)];
    let (res_d3, _) = b_d3.resolve_moves(&moves_d3, &cfg);
    assert_eq!(res_d3.cell_at(c00), ATT, "D3: empty-square 2-cycle must keep piece at origin");
    assert_eq!(res_d3.cell_at(c02), VAC);

    // D4: Conflicted coordinate marks and drops moves touching it at EITHER endpoint
    let mut b_d4 = PackedBoard::empty(5, 4, 4, true);
    let c40 = PackedCoord::new(4, 0);
    let c20 = PackedCoord::new(2, 0);
    b_d4.place(c00, ATT);
    b_d4.place(c40, REP);
    let rs_d4 = PackedRoundState::open(b_d4, 2);
    let goals5 = PackedGoals::stock_two_player(5);
    let outcome_d4 = rs_d4.round_step(
        &cfg,
        &goals5,
        &[PackedMove::new(0, c00, c20), PackedMove::new(1, c40, c20)],
    );
    match outcome_d4 {
        PackedRoundOutcome::Again(next_rs) => {
            assert!(next_rs.marks.get(b_d4.idx(c20)), "D4: contested destination must be marked");
            assert_eq!(next_rs.locked_len, 0);
            assert!(next_rs.is_waiting(0) && next_rs.is_waiting(1));
        }
        other => panic!("expected clash re-entry, got {other:?}"),
    }

    // D5: Vacuum confluence / merge resolution modes (Conflict, BunchBefore, Annihilate) & permutation invariance
    let mut b_d5 = PackedBoard::empty(5, 4, 4, true);
    let ca = PackedCoord::new(0, 0);
    let cx = PackedCoord::new(1, 0);
    let cb = PackedCoord::new(0, 1);
    let cy = PackedCoord::new(1, 1);
    let cm = PackedCoord::new(1, 2);
    b_d5.place(ca, ATT);
    b_d5.place(cb, REP);
    let m_fwd = [
        PackedMove::new(0, ca, cx),
        PackedMove::new(1, cx, cm),
        PackedMove::new(2, cb, cy),
        PackedMove::new(3, cy, cm),
    ];
    let m_rev = [m_fwd[2], m_fwd[3], m_fwd[0], m_fwd[1]];

    // Canonical Conflict mode: unresolved merge is flagged, board is untouched, permutation-invariant
    let (res_conf_a, unres_a) = b_d5.resolve_moves(&m_fwd, &cfg);
    let (res_conf_b, unres_b) = b_d5.resolve_moves(&m_rev, &cfg);
    assert!(!unres_a.is_empty(), "D5: vacuum confluence must be detected as unresolved");
    assert_eq!(unres_a, unres_b, "D5: unresolved set must be permutation-invariant");
    assert_eq!(res_conf_a, b_d5);
    assert_eq!(res_conf_b, b_d5);

    // BunchBefore mode: `cx(1,0)->cm(1,2)` has interior `cy(1,1)`, so both chains first back up
    // from `cm(1,2)` to `cy(1,1)` (contested) and then relax to `cx(1,0)` and `cb(0,1)`.
    let cfg_bunch = GameConfig {
        confluence: ConfluenceMode::BunchBefore,
        ..cfg
    };
    let (res_bunch, unres_bunch) = b_d5.resolve_moves(&m_fwd, &cfg_bunch);
    assert!(unres_bunch.is_empty());
    assert_eq!(res_bunch.cell_at(cx), ATT);
    assert_eq!(res_bunch.cell_at(cb), REP);
    assert_eq!(res_bunch.cell_at(cy), VAC);
    assert_eq!(res_bunch.cell_at(cm), VAC);

    // Annihilate mode: colliding movers destroy each other
    let cfg_ann = GameConfig {
        confluence: ConfluenceMode::Annihilate,
        ..cfg
    };
    let (res_ann, unres_ann) = b_d5.resolve_moves(&m_fwd, &cfg_ann);
    assert!(unres_ann.is_empty());
    assert_eq!(res_ann.cell_at(ca), VAC);
    assert_eq!(res_ann.cell_at(cb), VAC);
    assert_eq!(res_ann.cell_at(cm), VAC);

    // D6: Empty cycle cannot pull a piece in
    let mut b_d6 = PackedBoard::empty(5, 4, 4, true);
    let c10 = PackedCoord::new(1, 0);
    b_d6.place(c00, ATT);
    let moves_d6 = [
        PackedMove::new(0, c00, c10),
        PackedMove::new(1, c10, c20),
        PackedMove::new(2, c20, c10),
    ];
    let (res_d6, _) = b_d6.resolve_moves(&moves_d6, &cfg);
    assert_eq!(res_d6.cell_at(c00), ATT, "D6: piece moving into empty cycle must stay at origin");
    assert_eq!(res_d6.cell_at(c10), VAC);
    assert_eq!(res_d6.cell_at(c20), VAC);

    // Ruling D: Automaton square is banned as source ONLY, legal as destination (and fails to execute)
    let stock = PackedBoard::stock_two_player();
    let auto_c = stock.automaton();
    let adj_c = PackedCoord::new(auto_c.x, auto_c.y - 1);
    assert!(
        !stock.move_legal(BitBoard128::EMPTY, PackedMove::new(0, auto_c, adj_c)),
        "Ruling D: moving the automaton itself must be illegal"
    );
    let att_src = PackedCoord::new(5, 1);
    assert!(
        stock.move_legal(BitBoard128::EMPTY, PackedMove::new(0, att_src, auto_c)),
        "Ruling D: naming the automaton square as destination must be legal to propose"
    );
    let (res_rule_d, _) = stock.resolve_moves(&[PackedMove::new(0, att_src, auto_c)], &cfg);
    assert_eq!(res_rule_d, stock, "Ruling D: move targeting automaton must fail to execute");
}

#[test]
fn win_on_entry_and_adjudicate_capped_conformance() {
    let stock = PackedBoard::stock_two_player();
    let goals = PackedGoals::stock_two_player(11);

    // Sitting on a goal without moving is NOT a win (`winOnEntry`)
    let mut stationary_goal = PackedBoard::empty(5, 0, 0, true);
    let goals5 = PackedGoals::stock_two_player(5);
    assert_eq!(
        PackedBoard::win_on_entry(&stationary_goal, &stationary_goal, &goals5),
        None
    );

    // Moving INTO a goal corner IS a win
    let before = PackedBoard::empty(5, 0, 1, true);
    stationary_goal.place(PackedCoord::new(0, 1), VAC);
    stationary_goal.place(PackedCoord::new(0, 0), AUTO);
    assert_eq!(
        PackedBoard::win_on_entry(&before, &stationary_goal, &goals5),
        Some(0)
    );

    // Stock opening adjudicates to None (exact tie: 10 vs 10)
    assert_eq!(stock.goal_distance(&goals, 0), Some(10));
    assert_eq!(stock.goal_distance(&goals, 1), Some(10));
    assert_eq!(stock.adjudicate_capped(&goals), None);

    // Nudging automaton to y = 4 favors Seat 0; y = 6 favors Seat 1
    let mut nudged0 = stock;
    nudged0.place(PackedCoord::new(5, 5), VAC);
    nudged0.place(PackedCoord::new(5, 4), AUTO);
    assert_eq!(nudged0.adjudicate_capped(&goals), Some(0));

    let mut nudged1 = stock;
    nudged1.place(PackedCoord::new(5, 5), VAC);
    nudged1.place(PackedCoord::new(5, 6), AUTO);
    assert_eq!(nudged1.adjudicate_capped(&goals), Some(1));
}

#[test]
fn no_move_leakage_gate() {
    // Verify that uncommitted moves from a waiting seat (`waiting_mask` bit set) are structurally
    // unrepresentable in `PackedRoundState` (and stripped if a caller passes only locked moves
    // from non-waiting seats via `choose_move_from_wire`), so Seat 1's bot never sees Seat 0's secret move.
    let board = PackedBoard::stock_two_player();
    let wire = board.to_wire();
    let rs_a = PackedRoundState::open(board, 2);
    let rs_b = PackedRoundState::open(board, 2);

    assert_eq!(rs_a.locked_len, 0);
    assert_eq!(rs_b.locked_len, 0);
    assert_eq!(rs_a, rs_b, "public round state must contain zero secret moves");

    let goals = PackedGoals::stock_two_player(11);
    let cfg = GameConfig::default();

    for kind in [
        BotKind::Greedy,
        BotKind::Minimax,
        BotKind::DecoupledUct,
        BotKind::RegretMatching,
    ] {
        let mut bot_a = BotEngine::new(kind, 12345);
        let mut bot_b = BotEngine::new(kind, 12345);
        let mv_a = bot_a.choose_move(&rs_a, &goals, &cfg, 1).unwrap();
        let mv_b = bot_b.choose_move(&rs_b, &goals, &cfg, 1).unwrap();
        assert_eq!(
            mv_a, mv_b,
            "{kind:?} produced different moves for identical public state!"
        );

        let mut wire_bot = BotEngine::new(kind, 12345);
        let wire_mv = choose_move_from_wire(&mut wire_bot, &wire, &[], &[], &[0, 1], 1, &cfg)
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                mv_a.frm.x as i32,
                mv_a.frm.y as i32,
                mv_a.to.x as i32,
                mv_a.to.y as i32
            ),
            wire_mv,
            "{kind:?} choose_move_from_wire must match PackedRoundState::choose_move"
        );
    }
}

#[test]
fn no_duplicate_moves_gate() {
    let cfg = GameConfig::default();
    let board = PackedBoard::stock_two_player();
    let goals = PackedGoals::stock_two_player(11);
    let rs = PackedRoundState::open(board, 2);

    for kind in [
        BotKind::Greedy,
        BotKind::Minimax,
        BotKind::DecoupledUct,
        BotKind::RegretMatching,
    ] {
        let mut bot0 = BotEngine::new(kind, 101);
        let mut bot1 = BotEngine::new(kind, 202);

        let m0 = bot0.choose_move(&rs, &goals, &cfg, 0).expect("seat 0 move");
        let m1 = bot1.choose_move(&rs, &goals, &cfg, 1).expect("seat 1 move");

        assert_eq!(m0.who, 0, "{kind:?} seat 0 emitted wrong seat ID");
        assert_eq!(m1.who, 1, "{kind:?} seat 1 emitted wrong seat ID");
        assert!(validate_no_duplicate_seats(&[m0, m1]).is_ok());
    }

    // Verify duplicate-seat detector catches duplicate player moves
    let dup = [
        PackedMove::new(0, PackedCoord::new(3, 1), PackedCoord::new(3, 2)),
        PackedMove::new(0, PackedCoord::new(7, 1), PackedCoord::new(7, 2)),
    ];
    assert_eq!(
        validate_no_duplicate_seats(&dup),
        Err(DuplicateSeatSubmissionError { seat: 0 })
    );
}

#[test]
fn conflict_reentry_resubmission_gate() {
    let cfg = GameConfig::default();
    let board = PackedBoard::stock_two_player();
    let goals = PackedGoals::stock_two_player(11);
    let rs = PackedRoundState::open(board, 2);

    // Force a round 1 fork clash at (3, 1)
    let clash_subs = [
        PackedMove::new(0, PackedCoord::new(3, 1), PackedCoord::new(3, 3)),
        PackedMove::new(1, PackedCoord::new(3, 1), PackedCoord::new(2, 1)),
    ];
    let PackedRoundOutcome::Again(rs_reentry) = rs.round_step(&cfg, &goals, &clash_subs) else {
        panic!("expected clash re-entry on fork at (3,1)");
    };
    let marked_idx = board.idx(PackedCoord::new(3, 1));
    assert!(rs_reentry.marks.get(marked_idx));

    for kind in [
        BotKind::Greedy,
        BotKind::Minimax,
        BotKind::DecoupledUct,
        BotKind::RegretMatching,
    ] {
        let mut bot0 = BotEngine::new(kind, 777);
        let mut bot1 = BotEngine::new(kind, 888);

        let m0 = bot0
            .choose_move(&rs_reentry, &goals, &cfg, 0)
            .expect("re-entry move 0");
        let m1 = bot1
            .choose_move(&rs_reentry, &goals, &cfg, 1)
            .expect("re-entry move 1");

        assert!(
            rs_reentry.board.move_legal(rs_reentry.marks, m0),
            "{kind:?} seat 0 proposed move touching marked square: {m0:?}"
        );
        assert!(
            rs_reentry.board.move_legal(rs_reentry.marks, m1),
            "{kind:?} seat 1 proposed move touching marked square: {m1:?}"
        );
    }
}

#[test]
fn all_bots_play_complete_valid_matches() {
    let presets = [
        BoardPreset::Blitz7x7,
        BoardPreset::Tactical9x9,
        BoardPreset::Classic11x11,
    ];

    for preset in presets {
        let cfg = GameConfig {
            preset,
            max_turns: 16,
            ..GameConfig::default()
        };

        // 1. Minimax vs RegretMatching
        let mut mm = SimultaneousMinimaxBot::new(
            MinimaxConfig {
                max_depth: 2,
                root_branching: 10,
                inner_branching: 6,
                tie_epsilon: 1e-4,
            },
            42,
        );
        let mut rm = RegretMatchingBot::new(
            RegretConfig {
                candidate_count: 10,
                iterations: 100,
                support_threshold: 0.02,
                one_ply_lookahead: true,
            },
            99,
        );
        let rep1 = play_simultaneous_match(&mut mm, &mut rm, &cfg);
        assert!(rep1.turns_played >= 1 && rep1.turns_played <= cfg.max_turns);

        // 2. DecoupledUct vs GreedyHeuristic
        let mut duct = DecoupledUctBot::new(
            DuctConfig {
                num_simulations: 64,
                exploration_constant: 1.35,
                max_actions_per_seat: 12,
                rollout_depth: 4,
                rollout_epsilon: 0.25,
            },
            1337,
        );
        let mut greedy = GreedyHeuristicBot::new(2026);
        let rep2 = play_simultaneous_match(&mut duct, &mut greedy, &cfg);
        assert!(rep2.turns_played >= 1 && rep2.turns_played <= cfg.max_turns);
    }
}

#[test]
fn opening_symmetry_and_wire_roundtrip() {
    let cfg = GameConfig::default();
    let stock = PackedBoard::stock_two_player();
    let goals = PackedGoals::stock_two_player(11);

    // Wire round-trip
    let wire = stock.to_wire();
    let decoded = PackedBoard::from_wire(&wire).expect("decode stock wire");
    assert_eq!(stock, decoded);

    // Exploratory game-theoretic analysis: vertical symmetry & zero first-player advantage
    let report = analyze_opening_symmetry(&stock, &goals, &cfg);
    assert!(report.is_vertically_symmetric);
    assert!(report.reflection_commutes);
    assert!(
        report.stage_equilibrium.game_value_seat0.abs() < 1e-6,
        "expected symmetric 0.0 opening game value, got {}",
        report.stage_equilibrium.game_value_seat0
    );

    // Also verify symmetry commutes when `cooldown_turns > 0`
    let cfg_cd = GameConfig {
        cooldown_turns: 2,
        ..cfg
    };
    let report_cd = analyze_opening_symmetry(&stock, &goals, &cfg_cd);
    assert!(report_cd.is_vertically_symmetric);
    assert!(report_cd.reflection_commutes);

    // Wire-based entry point for all 4 bot kinds
    for kind in [
        BotKind::Greedy,
        BotKind::Minimax,
        BotKind::DecoupledUct,
        BotKind::RegretMatching,
    ] {
        let mut bot = BotEngine::new(kind, 2026);
        let mv_coords = choose_move_from_wire(&mut bot, &wire, &[], &[], &[0, 1], 0, &cfg)
            .expect("wire entry point should decode")
            .expect("wire entry point should return a move");
        assert!(
            mv_coords.0 != mv_coords.2 || mv_coords.1 != mv_coords.3,
            "expected non-zero displacement, got {mv_coords:?}"
        );
    }
}

#[test]
fn packed_board_high_throughput_and_r2_variants_gate() {
    // 1. Verify TieBreakMode variants (Column, Row, Freeze, Momentum)
    let mut b_tie = PackedBoard::empty(5, 2, 2, true);
    // Place equidistant attractors on X+ (4,2) and Y+ (2,4) so net = (1, 1) and tie = (1, 1)
    b_tie.place(PackedCoord::new(4, 2), ATT);
    b_tie.place(PackedCoord::new(2, 4), ATT);

    let cfg_col = GameConfig {
        tie_break: TieBreakMode::Column,
        ..GameConfig::default()
    };
    let cfg_row = GameConfig {
        tie_break: TieBreakMode::Row,
        ..GameConfig::default()
    };
    let cfg_freeze = GameConfig {
        tie_break: TieBreakMode::Freeze,
        ..GameConfig::default()
    };
    let cfg_mom_same = GameConfig {
        tie_break: TieBreakMode::Momentum {
            prefer_same_axis: true,
        },
        ..GameConfig::default()
    };
    let cfg_mom_ortho = GameConfig {
        tie_break: TieBreakMode::Momentum {
            prefer_same_axis: false,
        },
        ..GameConfig::default()
    };

    assert_eq!(b_tie.automaton_step(&cfg_col).automaton(), PackedCoord::new(2, 3));
    assert_eq!(b_tie.automaton_step(&cfg_row).automaton(), PackedCoord::new(3, 2));
    assert_eq!(b_tie.automaton_step(&cfg_freeze).automaton(), PackedCoord::new(2, 2));

    let mut b_tie_x = b_tie;
    b_tie_x.last_dx = 1;
    b_tie_x.last_dy = 0; // Previous move was along X
    assert_eq!(
        b_tie_x.automaton_step(&cfg_mom_same).automaton(),
        PackedCoord::new(3, 2)
    );
    assert_eq!(
        b_tie_x.automaton_step(&cfg_mom_ortho).automaton(),
        PackedCoord::new(2, 3)
    );

    // 2. High-throughput simulation gate on PackedBoard (1,500 full simultaneous steps)
    let cfg = GameConfig::default();
    let goals = PackedGoals::stock_two_player(11);
    let mut rs = PackedRoundState::open(PackedBoard::stock_two_player(), 2);
    let mut completed_steps = 0usize;

    for step in 0..1500u64 {
        let mut m0 = MoveList::new();
        let mut m1 = MoveList::new();
        rs.board
            .generate_tactical_moves(rs.marks, rs.locked_slice(), 0, &cfg, &mut m0);
        rs.board
            .generate_tactical_moves(rs.marks, rs.locked_slice(), 1, &cfg, &mut m1);
        if m0.is_empty() || m1.is_empty() {
            rs = PackedRoundState::open(PackedBoard::stock_two_player(), 2);
            continue;
        }
        let mv0 = m0.as_slice()[(step as usize * 7) % m0.len()];
        let mv1 = m1.as_slice()[(step as usize * 13 + 3) % m1.len()];
        match rs.round_step(&cfg, &goals, &[mv0, mv1]) {
            PackedRoundOutcome::Resolved { board, win } => {
                completed_steps += 1;
                if win.is_some() {
                    rs = PackedRoundState::open(PackedBoard::stock_two_player(), 2);
                } else {
                    rs = PackedRoundState::open(board, 2);
                }
            }
            PackedRoundOutcome::Again(next_rs) => {
                completed_steps += 1;
                rs = next_rs;
            }
        }
    }
    assert_eq!(completed_steps, 1500);
}

#[test]
fn blitz_7x7_layout_and_binary_packed_bytes_codec_parity() {
    let blitz = PackedBoard::stock_blitz_7x7();
    assert_eq!(blitz.size, 7);
    assert_eq!(blitz.automaton(), PackedCoord::new(3, 3));
    // Verify repulsors are at x in [2, 4] on rows 1 and 5 per Lean's `stockBlitz7`
    for y in [1u8, 5u8] {
        assert_eq!(blitz.cell_at(PackedCoord::new(2, y)), REP);
        assert_eq!(blitz.cell_at(PackedCoord::new(4, y)), REP);
        assert_eq!(blitz.cell_at(PackedCoord::new(3, y)), VAC);
    }

    // Verify binary `PackedBoard.lean` codec roundtrip (`PackedRoundState::to_packed_bytes` / `from_packed_bytes`)
    let mut b = PackedBoard::stock_two_player();
    b.last_dx = -1;
    b.last_dy = 0;
    let cd_idx = b.idx(PackedCoord::new(3, 4));
    b.cooldown_cells[0] = cd_idx as u8;
    b.cooldown_rem[0] = 2;
    b.cooldown_mask.set(cd_idx);
    b.cooldown_len = 1;
    let mut rs = PackedRoundState::open(b, 2);
    rs.marks.set(b.idx(PackedCoord::new(5, 1)));
    rs.locked[0] = PackedMove::new(0, PackedCoord::new(3, 1), PackedCoord::new(3, 2));
    rs.locked_len = 1;
    rs.waiting_mask = 0b10;
    let cfg = GameConfig {
        tie_break: TieBreakMode::Momentum {
            prefer_same_axis: true,
        },
        confluence: ConfluenceMode::BunchBefore,
        two_cycle_swaps: true,
        cooldown_turns: 2,
        railgun_fling: true,
        automata_count: 2,
        ..GameConfig::default()
    };
    let bytes = rs.to_packed_bytes(&cfg);
    let (unpacked_rs, unpacked_cfg, unpacked_goals2) =
        PackedRoundState::from_packed_bytes(&bytes).expect("unpack PackedRoundState bytes");
    assert_eq!(rs, unpacked_rs);
    assert_eq!(cfg, unpacked_cfg);
    assert_eq!(unpacked_goals2, PackedGoals::stock_two_player(11));

    // Verify custom/4-player `goals` roundtrip and backwards-compatible fallback when omitted
    let goals4 = PackedGoals::stock_four_player(11);
    let bytes_with_goals4 = rs.to_packed_bytes_with_goals(&cfg, &goals4);
    let (unpacked_rs4, unpacked_cfg4, unpacked_goals4) =
        PackedRoundState::from_packed_bytes(&bytes_with_goals4)
            .expect("unpack PackedRoundState bytes with 4P goals");
    assert_eq!(rs, unpacked_rs4);
    assert_eq!(cfg, unpacked_cfg4);
    assert_eq!(unpacked_goals4, goals4);

    // Verify `to_packed_bytes` on a 4-player round state (`active_player_count() > 2`) defaults to `stock_four_player`
    let rs_open4 = PackedRoundState::open(PackedBoard::stock_two_player(), 4);
    let (_, _, open4_goals) = PackedRoundState::from_packed_bytes(&rs_open4.to_packed_bytes(&cfg))
        .expect("unpack 4P to_packed_bytes");
    assert_eq!(open4_goals, goals4);

    // Strip the trailing goals section (`1 + 4 * 3 = 13` bytes) to verify legacy fallback
    let legacy_bytes = &bytes_with_goals4[..bytes_with_goals4.len() - 13];
    let (_, _, legacy_goals) =
        PackedRoundState::from_packed_bytes(legacy_bytes).expect("unpack legacy bytes without goals");
    assert_eq!(legacy_goals, PackedGoals::stock_two_player(11));

    // Verify trailing garbage after `goals` is rejected (matching `packed_bytes_rejects_trailing_garbage` in Lean)
    let mut corrupt_trailing = bytes_with_goals4.clone();
    corrupt_trailing.push(99);
    assert!(
        PackedRoundState::from_packed_bytes(&corrupt_trailing).is_err(),
        "from_packed_bytes must reject trailing garbage bytes"
    );

    // Verify `from_wire` rejects `"-"` when `size > 0` (matching `lean/Automatafl/FFI.lean::parseBoard`)
    assert!(PackedBoard::from_wire("5 - 2 2 1").is_err());
    assert!(PackedBoard::from_wire("0 - 0 0 1").is_ok());
}

#[test]
fn bunch_before_step_one_short_and_round_step_confluence_gate() {
    let goals = PackedGoals::stock_two_player(5);

    // Two pieces moving directly to the same vacuum square (2, 0) from (0, 0) and (4, 0)
    let mut b = PackedBoard::empty(5, 2, 2, true);
    let c00 = PackedCoord::new(0, 0);
    let c10 = PackedCoord::new(1, 0);
    let c20 = PackedCoord::new(2, 0);
    let c30 = PackedCoord::new(3, 0);
    let c40 = PackedCoord::new(4, 0);
    b.place(c00, ATT);
    b.place(c40, REP);

    let subs = [PackedMove::new(0, c00, c20), PackedMove::new(1, c40, c20)];

    // 1. In `BunchBefore` mode, `round_step` must NOT trigger a pre-move `collideAt` clash (`Again`),
    //    and single-edge moves of length 2 must stop 1 cell short at (1, 0) and (3, 0) (`stepOneShort`).
    let cfg_bunch = GameConfig {
        confluence: ConfluenceMode::BunchBefore,
        ..GameConfig::default()
    };
    let rs = PackedRoundState::open(b, 2);
    match rs.round_step(&cfg_bunch, &goals, &subs) {
        PackedRoundOutcome::Resolved { board, .. } => {
            assert_eq!(board.cell_at(c10), ATT, "ATT should stop 1 step short at (1,0)");
            assert_eq!(board.cell_at(c30), REP, "REP should stop 1 step short at (3,0)");
            assert_eq!(board.cell_at(c20), VAC, "collision square (2,0) must remain vacuum");
            assert_eq!(board.cell_at(c00), VAC);
            assert_eq!(board.cell_at(c40), VAC);
        }
        other => panic!("BunchBefore direct collision should resolve without Again, got {other:?}"),
    }

    let (res_bunch_direct, unres_direct) = b.resolve_moves(&subs, &cfg_bunch);
    assert!(unres_direct.is_empty());
    assert_eq!(res_bunch_direct.cell_at(c10), ATT);
    assert_eq!(res_bunch_direct.cell_at(c30), REP);

    // 2. In `Annihilate` mode, `round_step` resolves directly and destroys both colliding movers.
    let cfg_ann = GameConfig {
        confluence: ConfluenceMode::Annihilate,
        ..GameConfig::default()
    };
    match rs.round_step(&cfg_ann, &goals, &subs) {
        PackedRoundOutcome::Resolved { board, .. } => {
            assert_eq!(board.cell_at(c00), VAC);
            assert_eq!(board.cell_at(c10), VAC);
            assert_eq!(board.cell_at(c20), VAC);
            assert_eq!(board.cell_at(c30), VAC);
            assert_eq!(board.cell_at(c40), VAC);
        }
        other => panic!("Annihilate direct collision should resolve without Again, got {other:?}"),
    }
}

#[test]
fn railgun_fling_and_automata_count_and_cooldown_dedup_gate() {
    // 1. `railgun_fling` slides the automaton along its step direction until blocked by a piece or edge
    let mut b_fling = PackedBoard::empty(7, 1, 3, true);
    b_fling.place(PackedCoord::new(6, 3), ATT); // Attractor at (6, 3) pulls automaton right (+1, 0)
    let cfg_fling = GameConfig {
        railgun_fling: true,
        ..GameConfig::default()
    };
    let stepped_fling = b_fling.automaton_step(&cfg_fling);
    // Automaton slides from (1, 3) across vacuum to (5, 3), stopping right before the attractor at (6, 3)
    assert_eq!(stepped_fling.automaton(), PackedCoord::new(5, 3));
    assert_eq!(stepped_fling.cell_at(PackedCoord::new(6, 3)), ATT);

    // Verify player pieces are still blocked by intermediate obstacles even when `railgun_fling = true`
    let mut b_player_block = PackedBoard::empty(5, 4, 4, true);
    b_player_block.place(PackedCoord::new(0, 0), ATT);
    b_player_block.place(PackedCoord::new(1, 0), REP);
    let (res_pb, _) = b_player_block.resolve_moves(
        &[PackedMove::new(
            0,
            PackedCoord::new(0, 0),
            PackedCoord::new(3, 0),
        )],
        &cfg_fling,
    );
    assert_eq!(
        res_pb.cell_at(PackedCoord::new(0, 0)),
        ATT,
        "railgun_fling must not allow player pieces to jump over obstacles"
    );

    // 2. `automata_count` steps the automaton multiple times in `round_step` without placing extra AUTO tokens
    let cfg_multi_auto = GameConfig {
        preset: BoardPreset::Mini5x5,
        automata_count: 2,
        ..GameConfig::default()
    };
    let b_from_cfg = PackedBoard::from_config(&cfg_multi_auto);
    assert_eq!(
        b_from_cfg.automata.count_ones(),
        1,
        "from_config must keep exactly 1 canonical automaton token"
    );

    let mut b_two_steps = PackedBoard::empty(7, 1, 3, true);
    b_two_steps.place(PackedCoord::new(6, 3), ATT);
    b_two_steps.place(PackedCoord::new(0, 0), REP);
    let rs_two = PackedRoundState::open(b_two_steps, 1);
    let goals7 = PackedGoals::stock_two_player(7);
    let cfg_two = GameConfig {
        automata_count: 2,
        ..GameConfig::default()
    };
    match rs_two.round_step(
        &cfg_two,
        &goals7,
        &[PackedMove::new(
            0,
            PackedCoord::new(0, 0),
            PackedCoord::new(0, 1),
        )],
    ) {
        PackedRoundOutcome::Resolved { board, .. } => {
            // Automaton at (1, 3) steps twice toward (6, 3) -> lands at (3, 3)
            assert_eq!(board.automaton(), PackedCoord::new(3, 3));
        }
        other => panic!("expected resolved round, got {other:?}"),
    }

    // 3. Stationary automaton preserves `last_dx`/`last_dy` momentum
    let mut b_stationary = PackedBoard::empty(5, 2, 2, true);
    b_stationary.last_dx = 0;
    b_stationary.last_dy = -1;
    let after_stat = b_stationary.automaton_step(&GameConfig::default());
    assert_eq!(after_stat.last_dx, 0);
    assert_eq!(after_stat.last_dy, -1);

    // 4. Cooldown tracking records landed squares and blocks immediate re-move
    let mut b_cd = PackedBoard::empty(5, 4, 4, true);
    let c00 = PackedCoord::new(0, 0);
    let c01 = PackedCoord::new(0, 1);
    b_cd.place(c00, ATT);
    let cfg_cd = GameConfig {
        cooldown_turns: 2,
        ..GameConfig::default()
    };
    let (after_cd, _) = b_cd.resolve_moves(&[PackedMove::new(0, c00, c01)], &cfg_cd);
    assert_eq!(after_cd.cooldown_len, 1);
    assert!(after_cd.cooldown_mask.get(after_cd.idx(c01)));
    assert_eq!(after_cd.cooldown_rem[0], 2);
}

#[test]
fn four_player_simultaneous_and_reentry_cascade_all_bots_gate() {
    let cfg = GameConfig::default();
    let board = PackedBoard::stock_two_player();
    let goals = PackedGoals::stock_four_player(11);
    let rs_open = PackedRoundState::open(board, 4);

    // 1. Open 4-player round: all 4 bots must produce moves with `m.who == seat` for seats 0..4
    for kind in [
        BotKind::Greedy,
        BotKind::Minimax,
        BotKind::DecoupledUct,
        BotKind::RegretMatching,
    ] {
        let mut subs = Vec::new();
        for seat in 0..4u8 {
            let mut bot = BotEngine::new(kind, 500 + seat as u64);
            let mv = bot
                .choose_move(&rs_open, &goals, &cfg, seat)
                .unwrap_or_else(|| panic!("{kind:?} failed to choose move for seat {seat}"));
            assert_eq!(
                mv.who, seat,
                "{kind:?} emitted move with wrong seat ID for seat {seat}"
            );
            assert!(rs_open.board.move_legal(rs_open.marks, mv));
            subs.push(mv);
        }
        assert!(validate_no_duplicate_seats(&subs).is_ok());
    }

    // 2. Cascading `.again` clash in a 4-player round:
    // Seats 0 and 1 clash at (3, 1); Seat 2 locks a non-clashing move `(0, 4) -> (2, 4)`;
    // Seat 3 also clashes at (3, 1) -> Seats 0, 1, 3 must re-enter while Seat 2 stays locked.
    let c31 = PackedCoord::new(3, 1);
    let cascade_subs = [
        PackedMove::new(0, c31, PackedCoord::new(3, 2)),
        PackedMove::new(1, c31, PackedCoord::new(2, 1)),
        PackedMove::new(2, PackedCoord::new(0, 4), PackedCoord::new(2, 4)),
        PackedMove::new(3, c31, PackedCoord::new(3, 0)),
    ];
    let PackedRoundOutcome::Again(rs_cascade) = rs_open.round_step(&cfg, &goals, &cascade_subs)
    else {
        panic!("expected 4P partial clash re-entry");
    };
    assert!(rs_cascade.is_waiting(0));
    assert!(rs_cascade.is_waiting(1));
    assert!(!rs_cascade.is_waiting(2), "Seat 2 move should be locked");
    assert!(rs_cascade.is_waiting(3));
    assert_eq!(rs_cascade.locked_len, 1);
    assert_eq!(rs_cascade.locked_slice()[0].who, 2);

    for kind in [
        BotKind::Greedy,
        BotKind::Minimax,
        BotKind::DecoupledUct,
        BotKind::RegretMatching,
    ] {
        let mut bot2 = BotEngine::new(kind, 902);
        assert!(
            bot2.choose_move(&rs_cascade, &goals, &cfg, 2).is_none(),
            "Locked seat 2 must not emit a move"
        );

        let mut reentry_subs = Vec::new();
        for seat in [0u8, 1, 3] {
            let mut bot = BotEngine::new(kind, 900 + seat as u64);
            let mv = bot
                .choose_move(&rs_cascade, &goals, &cfg, seat)
                .unwrap_or_else(|| panic!("{kind:?} failed on 4P re-entry for seat {seat}"));
            assert_eq!(mv.who, seat);
            assert!(rs_cascade.board.move_legal(rs_cascade.marks, mv));
            reentry_subs.push(mv);
        }
        assert!(validate_no_duplicate_seats(&reentry_subs).is_ok());
    }
}

#[test]
fn bunch_before_caterpillar_chain_relaxation_and_cooldowns_gate() {
    // 1. 3-move caterpillar behind a collision:
    //    `m1: (0,1) -> (0,0)` trails leader `m2: (0,0) -> (1,0)` which collides with `m3: (2,0) -> (1,0)`.
    //    `m2` and `m3` stay at `(0,0)` and `(2,0)`, and follower `m1` relaxes back to `(0,1)`.
    //    Must conserve all 3 pieces, remain 100% permutation-invariant across all 6 orderings,
    //    and record ZERO new cooldowns since no piece changed square (`effLand c != c` is false).
    let mut b = PackedBoard::empty(5, 4, 4, true);
    let c01 = PackedCoord::new(0, 1);
    let c00 = PackedCoord::new(0, 0);
    let c10 = PackedCoord::new(1, 0);
    let c20 = PackedCoord::new(2, 0);
    b.place(c01, ATT);
    b.place(c00, REP);
    b.place(c20, ATT);

    let m1 = PackedMove::new(0, c01, c00);
    let m2 = PackedMove::new(1, c00, c10);
    let m3 = PackedMove::new(2, c20, c10);
    let perms = [
        [m1, m2, m3],
        [m1, m3, m2],
        [m2, m1, m3],
        [m2, m3, m1],
        [m3, m1, m2],
        [m3, m2, m1],
    ];

    let cfg_bunch_cd = GameConfig {
        confluence: ConfluenceMode::BunchBefore,
        cooldown_turns: 2,
        ..GameConfig::default()
    };
    let goals = PackedGoals::stock_two_player(5);
    let rs = PackedRoundState::open(b, 3);

    for perm in &perms {
        let (res_b, unres) = b.resolve_moves(perm, &cfg_bunch_cd);
        assert!(unres.is_empty());
        assert_eq!(res_b.cell_at(c01), ATT, "follower m1 must relax back to (0,1)");
        assert_eq!(res_b.cell_at(c00), REP, "leader m2 must stay at (0,0)");
        assert_eq!(res_b.cell_at(c20), ATT, "collider m3 must stay at (2,0)");
        assert_eq!(res_b.cell_at(c10), VAC, "collision square (1,0) must stay vacuum");
        assert_eq!(
            res_b.movable_pieces().count_ones(),
            3,
            "all 3 pieces must be conserved"
        );
        assert_eq!(
            res_b.cooldown_len, 0,
            "pieces that relaxed back to their starting squares must not receive cooldowns"
        );

        match rs.round_step(&cfg_bunch_cd, &goals, perm) {
            PackedRoundOutcome::Resolved { board, .. } => {
                assert_eq!(board.cell_at(c01), ATT);
                assert_eq!(board.cell_at(c00), REP);
                assert_eq!(board.cell_at(c20), ATT);
                assert_eq!(board.cell_at(c10), VAC);
                assert_eq!(board.cooldown_len, 0);
            }
            other => panic!("expected caterpillar BunchBefore to resolve cleanly, got {other:?}"),
        }
    }

    // 2. Multi-cell axial slide in `BunchBefore` with cooldowns placed on post-confluence landing squares:
    //    `m_left: (0,0) -> (2,0)` and `m_right: (4,0) -> (2,0)` collide at `(2,0)` and relax to
    //    `(1,0)` and `(3,0)`. Cooldowns must be placed on `(1,0)` and `(3,0)` (NOT `(2,0)`).
    let mut b_slide = PackedBoard::empty(5, 2, 2, true);
    let c30 = PackedCoord::new(3, 0);
    let c40 = PackedCoord::new(4, 0);
    b_slide.place(c00, ATT);
    b_slide.place(c40, REP);
    let slide_moves = [PackedMove::new(0, c00, c20), PackedMove::new(1, c40, c20)];
    let (res_slide, _) = b_slide.resolve_moves(&slide_moves, &cfg_bunch_cd);
    assert_eq!(res_slide.cell_at(c10), ATT);
    assert_eq!(res_slide.cell_at(c30), REP);
    assert_eq!(res_slide.cell_at(c20), VAC);
    assert_eq!(res_slide.cooldown_len, 2);
    assert!(res_slide.cooldown_mask.get(res_slide.idx(c10)));
    assert!(res_slide.cooldown_mask.get(res_slide.idx(c30)));
    assert!(!res_slide.cooldown_mask.get(res_slide.idx(c20)));
}

#[cfg(feature = "lean-bridge")]
#[test]
fn differential_ffi_verification_against_automatafl_lean_packed_step_gate() {
    // Helper that packs `(rs, cfg, goals)` into `automatafl_lean::PackedBoard`, runs both
    // `PackedRoundState::round_step` (Rust AI bitboard engine) and `automatafl_lean::PackedBoard::step`
    // (formally verified Lean 4 C FFI), and asserts identical post-step boards, waiting lists,
    // marks, and cooldowns.
    let assert_step_parity = |rs: &PackedRoundState,
                              cfg: &GameConfig,
                              goals: &PackedGoals,
                              subs: &[PackedMove],
                              label: &str| {
        let packed_bytes = rs.to_packed_bytes_with_goals(cfg, goals);
        let lean_pb = automatafl_lean::PackedBoard::from_bytes(&packed_bytes)
            .unwrap_or_else(|| panic!("{label}: Lean PackedBoard::from_bytes failed"));
        let lean_moves: Vec<automatafl_lean::Move> = subs
            .iter()
            .map(|m| automatafl_lean::Move {
                who: m.who as u32,
                frm: (m.frm.x as i32, m.frm.y as i32),
                to: (m.to.x as i32, m.to.y as i32),
            })
            .collect();

        let mut move_bytes = Vec::with_capacity(subs.len() * 5);
        for m in subs {
            move_bytes.push(m.who);
            move_bytes.push(m.frm.x);
            move_bytes.push(m.frm.y);
            move_bytes.push(m.to.x);
            move_bytes.push(m.to.y);
        }
        let mut client_out_buf = [0u8; 384];
        let client_out_len = unsafe {
            automatafl_client_step(
                packed_bytes.as_ptr(),
                packed_bytes.len(),
                move_bytes.as_ptr(),
                move_bytes.len(),
                client_out_buf.as_mut_ptr(),
                client_out_buf.len(),
            )
        };
        assert!(client_out_len > 62, "{label}: automatafl_client_step failed");

        let rust_out = rs.round_step(cfg, goals, subs);
        let lean_out = lean_pb
            .step(&lean_moves)
            .unwrap_or_else(|e| panic!("{label}: Lean PackedBoard::step failed: {e}"));

        match (rust_out, lean_out) {
            (
                PackedRoundOutcome::Resolved {
                    board: r_board,
                    win: r_win,
                },
                automatafl_lean::PackedOutcome::Resolved {
                    board: l_pb,
                    win: l_win,
                },
            ) => {
                assert_eq!(client_out_buf[0], 2, "{label}: client_step status should be 2");
                assert_eq!(
                    client_out_buf[1],
                    r_win.unwrap_or(255),
                    "{label}: client_step winner byte mismatch"
                );
                assert_eq!(
                    &client_out_buf[2..client_out_len],
                    l_pb.as_bytes(),
                    "{label}: client_step Resolved output bytes must match Lean packedStepFFI byte-for-byte"
                );
                let (c_rs, _, _) =
                    PackedRoundState::from_packed_bytes(&client_out_buf[2..client_out_len])
                        .expect("unpack client_step Resolved bytes");
                let (l_rs, _, _) = PackedRoundState::from_packed_bytes(l_pb.as_bytes())
                    .unwrap_or_else(|e| panic!("{label}: unpack Lean Resolved bytes failed: {e}"));
                assert_eq!(c_rs, l_rs, "{label}: client_step Resolved state mismatch vs Lean");
                assert_eq!(
                    r_win.map(u32::from),
                    l_win,
                    "{label}: winner mismatch between Rust and Lean"
                );
                assert_eq!(
                    r_board, l_rs.board,
                    "{label}: post-step board / cooldowns / momentum mismatch"
                );
                assert_eq!(
                    l_rs.marks,
                    BitBoard128::EMPTY,
                    "{label}: resolved round must clear marks"
                );
                let mut expected_waiting_mask = 0u8;
                for &(_, owner) in goals.slice() {
                    if owner < 8 {
                        expected_waiting_mask |= 1u8 << owner;
                    }
                }
                if expected_waiting_mask == 0 {
                    expected_waiting_mask = 0b11;
                }
                assert_eq!(
                    l_rs.waiting_mask, expected_waiting_mask,
                    "{label}: resolved round must reset waiting list to goal seats"
                );
            }
            (PackedRoundOutcome::Again(r_rs), automatafl_lean::PackedOutcome::Again(l_pb)) => {
                assert_eq!(client_out_buf[0], 1, "{label}: client_step status should be 1");
                assert_eq!(client_out_buf[1], 255);
                assert_eq!(
                    &client_out_buf[2..client_out_len],
                    l_pb.as_bytes(),
                    "{label}: client_step Again output bytes must match Lean packedStepFFI byte-for-byte"
                );
                let (c_rs, _, _) =
                    PackedRoundState::from_packed_bytes(&client_out_buf[2..client_out_len])
                        .expect("unpack client_step Again bytes");
                let (l_rs, _, _) = PackedRoundState::from_packed_bytes(l_pb.as_bytes())
                    .unwrap_or_else(|e| panic!("{label}: unpack Lean Again bytes failed: {e}"));
                assert_eq!(c_rs, l_rs, "{label}: client_step Again state mismatch vs Lean");
                assert_eq!(r_rs.board, l_rs.board, "{label}: Again board mismatch");
                assert_eq!(r_rs.marks, l_rs.marks, "{label}: Again marks mismatch");
                assert_eq!(
                    r_rs.waiting_mask, l_rs.waiting_mask,
                    "{label}: Again waiting_mask mismatch"
                );
                assert_eq!(
                    r_rs.locked_slice(),
                    l_rs.locked_slice(),
                    "{label}: Again locked moves mismatch"
                );
            }
            (r_other, l_other) => {
                panic!("{label}: outcome variant mismatch: Rust={r_other:?} vs Lean={l_other:?}");
            }
        }
    };

    let goals7 = PackedGoals::stock_two_player(7);
    let goals11 = PackedGoals::stock_two_player(11);
    let goals11_4p = PackedGoals::stock_four_player(11);

    // 1. Canonical rounds (clean resolution + fork/collide clash re-entry + out-of-range seat rejection)
    let cfg_canon = GameConfig::default();
    let rs_stock = PackedRoundState::open(PackedBoard::stock_two_player(), 2);
    assert_step_parity(
        &rs_stock,
        &cfg_canon,
        &goals11,
        &[
            PackedMove::new(0, PackedCoord::new(3, 1), PackedCoord::new(3, 3)),
            PackedMove::new(1, PackedCoord::new(7, 9), PackedCoord::new(7, 7)),
            // Out-of-range seat 8 must be ignored by `is_waiting` (not wrapped via `8 & 7 == 0`)
            PackedMove::new(8, PackedCoord::new(4, 1), PackedCoord::new(4, 3)),
        ],
        "canonical-clean-with-out-of-range-seat",
    );
    assert_step_parity(
        &rs_stock,
        &cfg_canon,
        &goals11,
        &[
            PackedMove::new(0, PackedCoord::new(3, 1), PackedCoord::new(3, 3)),
            PackedMove::new(1, PackedCoord::new(3, 9), PackedCoord::new(3, 3)),
        ],
        "canonical-clash",
    );

    // 2. 3-move caterpillar `BunchBefore` collision across all 6 move permutations
    let mut b_cat = PackedBoard::empty(7, 5, 5, true);
    let c01 = PackedCoord::new(0, 1);
    let c00 = PackedCoord::new(0, 0);
    let c10 = PackedCoord::new(1, 0);
    let c20 = PackedCoord::new(2, 0);
    b_cat.place(c01, ATT);
    b_cat.place(c00, REP);
    b_cat.place(c20, ATT);
    let rs_cat = PackedRoundState::open(b_cat, 3);
    let cfg_bunch = GameConfig {
        preset: BoardPreset::Blitz7x7,
        confluence: ConfluenceMode::BunchBefore,
        cooldown_turns: 2,
        ..GameConfig::default()
    };
    let m1 = PackedMove::new(0, c01, c00);
    let m2 = PackedMove::new(1, c00, c10);
    let m3 = PackedMove::new(2, c20, c10);
    let perms = [
        [m1, m2, m3],
        [m1, m3, m2],
        [m2, m1, m3],
        [m2, m3, m1],
        [m3, m1, m2],
        [m3, m2, m1],
    ];
    for (idx, perm) in perms.iter().enumerate() {
        assert_step_parity(
            &rs_cat,
            &cfg_bunch,
            &goals7,
            perm,
            &format!("caterpillar-bunch-perm-{idx}"),
        );
    }

    // Also test a multi-cell slide collision in `BunchBefore` with non-empty post-confluence cooldowns
    let mut b_slide = PackedBoard::empty(7, 3, 3, true);
    b_slide.place(PackedCoord::new(0, 0), ATT);
    b_slide.place(PackedCoord::new(4, 0), REP);
    let rs_slide = PackedRoundState::open(b_slide, 2);
    assert_step_parity(
        &rs_slide,
        &cfg_bunch,
        &goals7,
        &[
            PackedMove::new(0, PackedCoord::new(0, 0), PackedCoord::new(2, 0)),
            PackedMove::new(1, PackedCoord::new(4, 0), PackedCoord::new(2, 0)),
        ],
        "slide-bunch-with-cooldowns",
    );

    // 3. `ConfluenceMode::Annihilate` (colliding movers annihilate; surviving mover lands and gets cooldown)
    let mut b_ann = PackedBoard::empty(7, 3, 3, true);
    b_ann.place(PackedCoord::new(0, 0), ATT);
    b_ann.place(PackedCoord::new(4, 0), REP);
    b_ann.place(PackedCoord::new(0, 6), ATT);
    let rs_ann = PackedRoundState::open(b_ann, 3);
    let cfg_ann = GameConfig {
        preset: BoardPreset::Blitz7x7,
        confluence: ConfluenceMode::Annihilate,
        cooldown_turns: 2,
        ..GameConfig::default()
    };
    assert_step_parity(
        &rs_ann,
        &cfg_ann,
        &goals7,
        &[
            PackedMove::new(0, PackedCoord::new(0, 0), PackedCoord::new(2, 0)),
            PackedMove::new(1, PackedCoord::new(4, 0), PackedCoord::new(2, 0)),
            PackedMove::new(2, PackedCoord::new(0, 6), PackedCoord::new(0, 5)),
        ],
        "annihilate-with-survivor-cooldown",
    );

    // 4. `railgun_fling` + `cooldown_turns` across consecutive turns
    let mut b_fling = PackedBoard::empty(7, 1, 3, true);
    b_fling.place(PackedCoord::new(6, 3), ATT);
    b_fling.place(PackedCoord::new(0, 0), REP);
    b_fling.place(PackedCoord::new(6, 0), ATT);
    let rs_fling = PackedRoundState::open(b_fling, 2);
    let cfg_fling_cd = GameConfig {
        preset: BoardPreset::Blitz7x7,
        railgun_fling: true,
        cooldown_turns: 2,
        ..GameConfig::default()
    };
    assert_step_parity(
        &rs_fling,
        &cfg_fling_cd,
        &goals7,
        &[
            PackedMove::new(0, PackedCoord::new(0, 0), PackedCoord::new(0, 2)),
            PackedMove::new(1, PackedCoord::new(6, 0), PackedCoord::new(6, 2)),
        ],
        "railgun-fling-and-cooldown-turn-1",
    );

    // Step into turn 2 and verify cooldown blocking & decrement parity
    let PackedRoundOutcome::Resolved { board: b_turn2, .. } = rs_fling.round_step(
        &cfg_fling_cd,
        &goals7,
        &[
            PackedMove::new(0, PackedCoord::new(0, 0), PackedCoord::new(0, 2)),
            PackedMove::new(1, PackedCoord::new(6, 0), PackedCoord::new(6, 2)),
        ],
    ) else {
        panic!("expected turn 1 to resolve");
    };
    let rs_turn2 = PackedRoundState::open(b_turn2, 2);
    assert_step_parity(
        &rs_turn2,
        &cfg_fling_cd,
        &goals7,
        &[
            // Seat 0 attempts to move the on-cooldown piece at (0, 2) -> filtered out by moveLegalVariantB
            PackedMove::new(0, PackedCoord::new(0, 2), PackedCoord::new(0, 4)),
            // Seat 1 moves the non-cooldown attractor at (6, 3) -> (6, 4)
            PackedMove::new(1, PackedCoord::new(6, 3), PackedCoord::new(6, 4)),
        ],
        "railgun-fling-and-cooldown-turn-2",
    );

    // 5. Multi-turn 4-player stress test: `BunchBefore` + `two_cycle_swaps = true` + `cooldown_turns = 3`
    //    accumulating 12 (> 8) concurrent active cooldowns across 3 consecutive turns.
    let cfg_4p_stress = GameConfig {
        preset: BoardPreset::Classic11x11,
        confluence: ConfluenceMode::BunchBefore,
        two_cycle_swaps: true,
        cooldown_turns: 3,
        automata_count: 2,
        ..GameConfig::default()
    };
    let mut rs_4p = PackedRoundState::open(PackedBoard::stock_two_player(), 4);

    // Turn 1: 2-cycle swap between (4,1) and (5,1) + two independent slides -> 4 cooldowns recorded
    let t1_moves = [
        PackedMove::new(0, PackedCoord::new(4, 1), PackedCoord::new(5, 1)),
        PackedMove::new(1, PackedCoord::new(5, 1), PackedCoord::new(4, 1)),
        PackedMove::new(2, PackedCoord::new(3, 1), PackedCoord::new(3, 3)),
        PackedMove::new(3, PackedCoord::new(7, 1), PackedCoord::new(7, 3)),
    ];
    assert_step_parity(&rs_4p, &cfg_4p_stress, &goals11_4p, &t1_moves, "4p-stress-turn-1");
    let PackedRoundOutcome::Resolved { board: b_after_t1, .. } =
        rs_4p.round_step(&cfg_4p_stress, &goals11_4p, &t1_moves)
    else {
        panic!("expected 4p turn 1 to resolve");
    };
    assert_eq!(b_after_t1.cooldown_len, 4);
    rs_4p = PackedRoundState::open(b_after_t1, 4);

    // Turn 2: 2-cycle swap on bottom row + two slides -> 4 + 4 = 8 cooldowns active
    let t2_moves = [
        PackedMove::new(0, PackedCoord::new(4, 9), PackedCoord::new(5, 9)),
        PackedMove::new(1, PackedCoord::new(5, 9), PackedCoord::new(4, 9)),
        PackedMove::new(2, PackedCoord::new(3, 9), PackedCoord::new(3, 7)),
        PackedMove::new(3, PackedCoord::new(7, 9), PackedCoord::new(7, 7)),
    ];
    assert_step_parity(&rs_4p, &cfg_4p_stress, &goals11_4p, &t2_moves, "4p-stress-turn-2");
    let PackedRoundOutcome::Resolved { board: b_after_t2, .. } =
        rs_4p.round_step(&cfg_4p_stress, &goals11_4p, &t2_moves)
    else {
        panic!("expected 4p turn 2 to resolve");
    };
    assert_eq!(b_after_t2.cooldown_len, 8);
    rs_4p = PackedRoundState::open(b_after_t2, 4);

    // Turn 3: 2-piece BunchBefore slide collision + two flank moves -> 4 + 4 + 4 = 12 (> 8) cooldowns active!
    let t3_moves = [
        PackedMove::new(0, PackedCoord::new(1, 4), PackedCoord::new(4, 4)),
        PackedMove::new(1, PackedCoord::new(9, 4), PackedCoord::new(4, 4)),
        PackedMove::new(2, PackedCoord::new(1, 6), PackedCoord::new(3, 6)),
        PackedMove::new(3, PackedCoord::new(9, 6), PackedCoord::new(7, 6)),
    ];
    assert_step_parity(&rs_4p, &cfg_4p_stress, &goals11_4p, &t3_moves, "4p-stress-turn-3");
    let PackedRoundOutcome::Resolved { board: b_after_t3, .. } =
        rs_4p.round_step(&cfg_4p_stress, &goals11_4p, &t3_moves)
    else {
        panic!("expected 4p turn 3 to resolve");
    };
    assert_eq!(
        b_after_t3.cooldown_len, 12,
        "PackedBoard must retain all 12 active cooldowns (> 8) matching Lean"
    );

    // 5b. Reverse-seat-order clash submission `[seat 1, seat 0]` (`waiting` order `[1, 0]`):
    assert_step_parity(
        &rs_stock,
        &cfg_canon,
        &goals11,
        &[
            PackedMove::new(1, PackedCoord::new(3, 9), PackedCoord::new(3, 3)),
            PackedMove::new(0, PackedCoord::new(3, 1), PackedCoord::new(3, 3)),
        ],
        "reverse-seat-order-clash",
    );

    // 5c. Identical-move exception (`[seat 0, seat 1, seat 2]` where Seat 0 and Seat 2 submit
    //     the identical move `(3, 1) -> (3, 3)` and Seat 1 submits `(7, 9) -> (7, 7)`) with
    //     `cooldown_turns = 2`: verifies `List.dedup` last-occurrence mover & cooldown order.
    let rs_ident_cd = PackedRoundState::open(PackedBoard::stock_two_player(), 3);
    assert_step_parity(
        &rs_ident_cd,
        &cfg_4p_stress,
        &goals11_4p,
        &[
            PackedMove::new(0, PackedCoord::new(3, 1), PackedCoord::new(3, 3)),
            PackedMove::new(1, PackedCoord::new(7, 9), PackedCoord::new(7, 7)),
            PackedMove::new(2, PackedCoord::new(3, 1), PackedCoord::new(3, 3)),
        ],
        "identical-move-cooldown-dedup-last-occurrence",
    );

    // 5d. Custom `PackedGoals` with non-contiguous and reverse seat order `[3, 0, 3, 0]`:
    let mut custom_goals_30 = PackedGoals::empty();
    custom_goals_30.entries[0] = (PackedCoord::new(0, 0), 3);
    custom_goals_30.entries[1] = (PackedCoord::new(10, 0), 0);
    custom_goals_30.entries[2] = (PackedCoord::new(10, 10), 3);
    custom_goals_30.entries[3] = (PackedCoord::new(0, 10), 0);
    custom_goals_30.len = 4;
    let rs_custom_30 = PackedRoundState::open_with_mask(PackedBoard::stock_two_player(), 0b1001);
    assert_step_parity(
        &rs_custom_30,
        &cfg_canon,
        &custom_goals_30,
        &[
            PackedMove::new(3, PackedCoord::new(3, 1), PackedCoord::new(3, 3)),
            PackedMove::new(0, PackedCoord::new(7, 9), PackedCoord::new(7, 7)),
        ],
        "custom-goals-non-contiguous-seats-3-and-0",
    );

    // 6. Differential verification of `automatafl_client_legal_targets` against
    //    `automatafl_lean::PackedBoard::legal_targets_mask` across all squares on:
    //    - 11x11 Classic opening (`rs_stock`)
    //    - 4P multi-cooldown post-turn board (`b_after_t3`)
    //    - Clash re-entry state with `marks` and `locked` moves
    let mut rs_reentry_locked = PackedRoundState::open(PackedBoard::stock_two_player(), 3);
    rs_reentry_locked.marks.set(rs_reentry_locked.board.idx(PackedCoord::new(3, 3)));
    rs_reentry_locked.locked[0] =
        PackedMove::new(2, PackedCoord::new(4, 1), PackedCoord::new(4, 3));
    rs_reentry_locked.locked_len = 1;
    rs_reentry_locked.waiting_mask = 0b011;

    for (label, test_rs, test_cfg, test_goals) in [
        ("opening-11x11", rs_stock, cfg_canon, goals11),
        (
            "post-turn3-12-cooldowns",
            PackedRoundState::open(b_after_t3, 4),
            cfg_4p_stress,
            goals11_4p,
        ),
        (
            "reentry-marks-and-locked",
            rs_reentry_locked,
            cfg_canon,
            goals11,
        ),
    ] {
        let state_bytes = test_rs.to_packed_bytes_with_goals(&test_cfg, &test_goals);
        let lean_pb = automatafl_lean::PackedBoard::from_bytes(&state_bytes)
            .expect("Lean PackedBoard::from_bytes");
        let n = test_rs.board.size;
        for fy in 0..=n {
            for fx in 0..=n {
                let mut prop_lo = 0u64;
                let mut prop_hi = 0u64;
                let mut exec_lo = 0u64;
                let mut exec_hi = 0u64;
                let ok = unsafe {
                    automatafl_client_legal_targets(
                        state_bytes.as_ptr(),
                        state_bytes.len(),
                        0,
                        fx,
                        fy,
                        &mut prop_lo,
                        &mut prop_hi,
                        &mut exec_lo,
                        &mut exec_hi,
                    )
                };
                assert_eq!(ok, 1, "{label}: client_legal_targets should succeed");
                let l_prop = lean_pb.legal_targets_mask(0, (fx as i32, fy as i32), false);
                let l_exec = lean_pb.legal_targets_mask(0, (fx as i32, fy as i32), true);
                assert_eq!(
                    (prop_lo, prop_hi),
                    (l_prop.lo, l_prop.hi),
                    "{label} ({fx},{fy}): proposable target bitmask mismatch vs Lean"
                );
                assert_eq!(
                    (exec_lo, exec_hi),
                    (l_exec.lo, l_exec.hi),
                    "{label} ({fx},{fy}): executable target bitmask mismatch vs Lean"
                );
            }
        }
    }
}

#[test]
fn wasm_api_client_choose_move_and_step_and_legal_targets_gate() {
    let cfg = GameConfig::default();
    let rs = PackedRoundState::open(PackedBoard::stock_two_player(), 2);
    let state_bytes = rs.to_packed_bytes(&cfg);

    // 1. `automatafl_client_choose_move`: test all 4 bot kinds (0..=3) + determinism + error paths
    for bot_kind in 0..=3u8 {
        for seat in [0u8, 1u8] {
            let mut mv_a = [0u8; 5];
            let mut mv_b = [0u8; 5];
            let ok_a = unsafe {
                automatafl_client_choose_move(
                    state_bytes.as_ptr(),
                    state_bytes.len(),
                    bot_kind,
                    seat,
                    2026_11_01,
                    mv_a.as_mut_ptr(),
                )
            };
            let ok_b = unsafe {
                automatafl_client_choose_move(
                    state_bytes.as_ptr(),
                    state_bytes.len(),
                    bot_kind,
                    seat,
                    2026_11_01,
                    mv_b.as_mut_ptr(),
                )
            };
            assert_eq!(ok_a, 1);
            assert_eq!(ok_b, 1);
            assert_eq!(
                mv_a, mv_b,
                "bot_kind={bot_kind} seat={seat} must be 100% deterministic for fixed seed"
            );
            assert_eq!(mv_a[0], seat);
            let pm = PackedMove::new(
                mv_a[0],
                PackedCoord::new(mv_a[1], mv_a[2]),
                PackedCoord::new(mv_a[3], mv_a[4]),
            );
            assert!(
                rs.board.move_legal(rs.marks, pm),
                "bot_kind={bot_kind} returned illegal move {pm:?}"
            );
        }
    }

    // Non-waiting seat returns 0
    let mut dummy_mv = [0u8; 5];
    assert_eq!(
        unsafe {
            automatafl_client_choose_move(
                state_bytes.as_ptr(),
                state_bytes.len(),
                0,
                3,
                42,
                dummy_mv.as_mut_ptr(),
            )
        },
        0
    );
    // Null or malformed state buffer returns 0
    assert_eq!(
        unsafe {
            automatafl_client_choose_move(
                core::ptr::null(),
                state_bytes.len(),
                0,
                0,
                42,
                dummy_mv.as_mut_ptr(),
            )
        },
        0
    );
    assert_eq!(
        unsafe {
            automatafl_client_choose_move(
                state_bytes.as_ptr(),
                12,
                0,
                0,
                42,
                dummy_mv.as_mut_ptr(),
            )
        },
        0
    );

    // 2. `automatafl_client_legal_targets`: O(1) single-pass bitboard verification
    let mut prop_lo = 0u64;
    let mut prop_hi = 0u64;
    let mut exec_lo = 0u64;
    let mut exec_hi = 0u64;
    let ok_t = unsafe {
        automatafl_client_legal_targets(
            state_bytes.as_ptr(),
            state_bytes.len(),
            0,
            3,
            1,
            &mut prop_lo,
            &mut prop_hi,
            &mut exec_lo,
            &mut exec_hi,
        )
    };
    assert_eq!(ok_t, 1);
    let prop_bb = BitBoard128([prop_lo, prop_hi]);
    let exec_bb = BitBoard128([exec_lo, exec_hi]);
    assert_eq!(prop_bb.count_ones(), 20, "(3,1) on 11x11 has 20 rook squares");
    assert_eq!(exec_bb.count_ones(), 11, "(3,1) on 11x11 has 11 unblocked squares");
    assert_eq!(exec_bb.without(prop_bb), BitBoard128::EMPTY);

    // Automaton square (5, 5) cannot be moved
    let ok_auto = unsafe {
        automatafl_client_legal_targets(
            state_bytes.as_ptr(),
            state_bytes.len(),
            0,
            5,
            5,
            &mut prop_lo,
            &mut prop_hi,
            &mut exec_lo,
            &mut exec_hi,
        )
    };
    assert_eq!(ok_auto, 1);
    assert_eq!((prop_lo, prop_hi, exec_lo, exec_hi), (0, 0, 0, 0));

    // Chain-continuation pass-through & marked intermediate vacuum square:
    // Piece A at (1, 1), Piece B at (3, 1), mark at vacuum (2, 1), locked move from (3, 1) -> (3, 3).
    // From (1, 1) along +X:
    // - (2, 1) is marked -> neither proposable nor executable
    // - (3, 1) is occupied by Piece B, which is in `locked` sources -> passable! Both proposable and executable!
    // - (4, 1) is vacuum beyond (3, 1) -> both proposable and executable!
    let mut b_chain = PackedBoard::empty(5, 4, 4, true);
    b_chain.place(PackedCoord::new(1, 1), ATT);
    b_chain.place(PackedCoord::new(3, 1), REP);
    let mut rs_chain = PackedRoundState::open(b_chain, 2);
    rs_chain.marks.set(b_chain.idx(PackedCoord::new(2, 1)));
    rs_chain.locked[0] = PackedMove::new(1, PackedCoord::new(3, 1), PackedCoord::new(3, 3));
    rs_chain.locked_len = 1;
    rs_chain.waiting_mask = 0b01;
    let chain_bytes = rs_chain.to_packed_bytes(&cfg);
    let (chain_prop, chain_exec) =
        client_legal_targets(&chain_bytes, 0, 1, 1).expect("chain legal targets");
    assert!(!chain_prop.get(b_chain.idx(PackedCoord::new(2, 1))));
    assert!(!chain_exec.get(b_chain.idx(PackedCoord::new(2, 1))));
    assert!(chain_prop.get(b_chain.idx(PackedCoord::new(3, 1))));
    assert!(chain_exec.get(b_chain.idx(PackedCoord::new(3, 1))));
    assert!(chain_prop.get(b_chain.idx(PackedCoord::new(4, 1))));
    assert!(chain_exec.get(b_chain.idx(PackedCoord::new(4, 1))));

    // Malformed state buffer zeroes output pointers and returns 0
    prop_lo = 0xDEAD_BEEF;
    prop_hi = 0xDEAD_BEEF;
    exec_lo = 0xDEAD_BEEF;
    exec_hi = 0xDEAD_BEEF;
    assert_eq!(
        unsafe {
            automatafl_client_legal_targets(
                core::ptr::null(),
                0,
                0,
                0,
                0,
                &mut prop_lo,
                &mut prop_hi,
                &mut exec_lo,
                &mut exec_hi,
            )
        },
        0
    );
    assert_eq!((prop_lo, prop_hi, exec_lo, exec_hi), (0, 0, 0, 0));

    // 3. `automatafl_client_step`: test clash re-entry (`status = 1`), clean resolution (`status = 2`),
    //    and a client-side local Vs-Bot game loop over 7x7 Blitz.
    let clash_moves = [0u8, 3, 1, 3, 3, 1, 3, 9, 3, 3];
    let mut step_out = [0u8; 384];
    let needed_len = unsafe {
        automatafl_client_step(
            state_bytes.as_ptr(),
            state_bytes.len(),
            clash_moves.as_ptr(),
            clash_moves.len(),
            core::ptr::null_mut(),
            0,
        )
    };
    assert!(needed_len > 60);
    let written_len = unsafe {
        automatafl_client_step(
            state_bytes.as_ptr(),
            state_bytes.len(),
            clash_moves.as_ptr(),
            clash_moves.len(),
            step_out.as_mut_ptr(),
            step_out.len(),
        )
    };
    assert_eq!(written_len, needed_len);
    assert_eq!(step_out[0], 1, "expected status=1 (Again) on clash");
    assert_eq!(step_out[1], 255);
    let (again_rs, _, _) =
        PackedRoundState::from_packed_bytes(&step_out[2..written_len]).expect("unpack Again state");
    assert!(again_rs.marks.get(rs.board.idx(PackedCoord::new(3, 3))));

    // Local Vs-Bot multi-turn client loop on 7x7 Blitz using C-ABI functions
    let cfg_blitz = GameConfig {
        preset: BoardPreset::Blitz7x7,
        ..GameConfig::default()
    };
    let mut cur_bytes =
        PackedRoundState::open(PackedBoard::from_config(&cfg_blitz), 2).to_packed_bytes(&cfg_blitz);
    let mut resolved_turns = 0u32;
    for step_idx in 0..16u64 {
        let (cur_rs, _, _) =
            PackedRoundState::from_packed_bytes(&cur_bytes).expect("decode current client state");
        let mut moves_buf = Vec::with_capacity(10);
        for seat in 0..2u8 {
            if cur_rs.is_waiting(seat) {
                let mut mv5 = [0u8; 5];
                let ok = unsafe {
                    automatafl_client_choose_move(
                        cur_bytes.as_ptr(),
                        cur_bytes.len(),
                        if seat == 0 { 0 } else { 1 },
                        seat,
                        1000 + step_idx * 17 + seat as u64,
                        mv5.as_mut_ptr(),
                    )
                };
                assert_eq!(ok, 1, "client bot must choose move for waiting seat {seat}");
                moves_buf.extend_from_slice(&mv5);
            }
        }
        let n_out = unsafe {
            automatafl_client_step(
                cur_bytes.as_ptr(),
                cur_bytes.len(),
                moves_buf.as_ptr(),
                moves_buf.len(),
                step_out.as_mut_ptr(),
                step_out.len(),
            )
        };
        assert!(n_out > 62);
        assert!(step_out[0] == 1 || step_out[0] == 2);
        if step_out[0] == 2 {
            resolved_turns += 1;
        }
        cur_bytes = step_out[2..n_out].to_vec();
    }
    assert!(resolved_turns >= 4, "expected multiple resolved turns in client Vs-Bot loop");

    // 4. Edge-case robustness checks:
    // (a) `sz == 0` in packed state buffer must be rejected without division-by-zero panic
    let mut zero_sz_bytes = state_bytes.clone();
    zero_sz_bytes[48] = 0;
    assert_eq!(
        unsafe {
            automatafl_client_choose_move(
                zero_sz_bytes.as_ptr(),
                zero_sz_bytes.len(),
                0,
                0,
                42,
                dummy_mv.as_mut_ptr(),
            )
        },
        0
    );
    assert_eq!(
        unsafe {
            automatafl_client_legal_targets(
                zero_sz_bytes.as_ptr(),
                zero_sz_bytes.len(),
                0,
                0,
                0,
                &mut prop_lo,
                &mut prop_hi,
                &mut exec_lo,
                &mut exec_hi,
            )
        },
        0
    );
    assert_eq!(
        unsafe {
            automatafl_client_step(
                zero_sz_bytes.as_ptr(),
                zero_sz_bytes.len(),
                core::ptr::null(),
                0,
                step_out.as_mut_ptr(),
                step_out.len(),
            )
        },
        0
    );

    // (b) Non-axial (`(0,0) -> (1,2)`) and out-of-bounds (`(20,20) -> (20,21)`) locked moves
    //     in `packed_state` must never hang `blocked_by_mask` or panic `resolve_moves`
    let mut rs_bad_locked = PackedRoundState::open(PackedBoard::stock_two_player(), 2);
    rs_bad_locked.locked[0] = PackedMove::new(2, PackedCoord::new(0, 0), PackedCoord::new(1, 2));
    rs_bad_locked.locked[1] =
        PackedMove::new(3, PackedCoord::new(20, 20), PackedCoord::new(20, 21));
    rs_bad_locked.locked_len = 2;
    let bad_locked_bytes = rs_bad_locked.to_packed_bytes(&cfg);
    let valid_step_moves = [0u8, 3, 1, 3, 3, 1, 3, 9, 3, 7];
    let n_bad_locked = unsafe {
        automatafl_client_step(
            bad_locked_bytes.as_ptr(),
            bad_locked_bytes.len(),
            valid_step_moves.as_ptr(),
            valid_step_moves.len(),
            step_out.as_mut_ptr(),
            step_out.len(),
        )
    };
    assert!(n_bad_locked > 62, "step with non-axial/OOB locked moves must terminate cleanly");

    // (c) `> 32` move records in `moves_bytes` where the first 33 records are from non-waiting
    //     seats (`seat = 7`) and records #34 and #35 are valid moves from waiting seats `0` and `1`:
    let mut many_moves = Vec::with_capacity(35 * 5);
    for _ in 0..33 {
        many_moves.extend_from_slice(&[7u8, 3, 1, 3, 3]);
    }
    many_moves.extend_from_slice(&[0u8, 3, 1, 3, 3]);
    many_moves.extend_from_slice(&[1u8, 3, 9, 3, 7]);
    let n_many = unsafe {
        automatafl_client_step(
            state_bytes.as_ptr(),
            state_bytes.len(),
            many_moves.as_ptr(),
            many_moves.len(),
            step_out.as_mut_ptr(),
            step_out.len(),
        )
    };
    assert!(n_many > 62);
    let (many_rs, _, _) =
        PackedRoundState::from_packed_bytes(&step_out[2..n_many]).expect("unpack many-moves step");
    assert_eq!(
        many_rs.board.cell_at(PackedCoord::new(3, 3)),
        ATT,
        "move #34 after 33 non-waiting records must not be dropped"
    );

    // (d) Non-contiguous goal seats (`Seats 0 and 3`, `waiting_mask = 0b1001`) across all 4 bot engines:
    let mut goals_03 = PackedGoals::empty();
    goals_03.entries[0] = (PackedCoord::new(0, 0), 0);
    goals_03.entries[1] = (PackedCoord::new(10, 0), 0);
    goals_03.entries[2] = (PackedCoord::new(0, 10), 3);
    goals_03.entries[3] = (PackedCoord::new(10, 10), 3);
    goals_03.len = 4;
    let rs_03 = PackedRoundState::open_with_mask(PackedBoard::stock_two_player(), 0b1001);
    let bytes_03 = rs_03.to_packed_bytes_with_goals(&cfg, &goals_03);
    for bot_kind in 0..=3u8 {
        for seat in [0u8, 3u8] {
            let mut mv5 = [0u8; 5];
            let ok = unsafe {
                automatafl_client_choose_move(
                    bytes_03.as_ptr(),
                    bytes_03.len(),
                    bot_kind,
                    seat,
                    99,
                    mv5.as_mut_ptr(),
                )
            };
            assert_eq!(ok, 1, "bot_kind={bot_kind} must choose move for active seat {seat}");
            assert_eq!(mv5[0], seat);
        }
        for inactive_seat in [1u8, 2u8] {
            let ok = unsafe {
                automatafl_client_choose_move(
                    bytes_03.as_ptr(),
                    bytes_03.len(),
                    bot_kind,
                    inactive_seat,
                    99,
                    dummy_mv.as_mut_ptr(),
                )
            };
            assert_eq!(ok, 0, "bot_kind={bot_kind} must reject inactive seat {inactive_seat}");
        }
    }
}
