//! Comprehensive conformance, release-gate, R2 variant, and R1.2 `PackedBoard` equivalence tests
//! for `automatafl-lean`.

use automatafl_lean::*;

#[test]
fn stock_opening_and_goals_match_spec() {
    assert!(available());
    let b = stock_two_player().expect("stock_two_player");
    assert_eq!(b.n, 11);
    assert_eq!(b.auto, (5, 5));
    assert!(b.col_rule);
    assert_eq!(b.cell_at((5, 5)), AUTO);
    assert_eq!(b.cell_at((4, 0)), REP);
    assert_eq!(b.cell_at((3, 1)), ATT);

    let goals = stock_goals2(11).expect("stock_goals2");
    assert_eq!(
        goals,
        vec![
            ((0, 0), 0),
            ((10, 0), 0),
            ((0, 10), 1),
            ((10, 10), 1),
        ]
    );
}

#[test]
fn automaton_actually_moves_in_all_profiles() {
    // Direct equivalent of `v8n/tests/release_profile_gates.rs::automaton_actually_moves`
    let mut b = Board::empty_with_size(5, true);
    let start = b.auto;
    assert_eq!(start, (2, 2));
    // Lone attractor two squares in +Y: Priority 3 (towardAttractor)
    b.set_cell((2, 4), ATT);

    let s = sense(&b).expect("sense");
    assert_eq!(s.offset, (0, 1), "automaton should choose +Y toward attractor");

    let after = automaton_step(&b).expect("automaton_step");
    assert_eq!(after.auto, (2, 3));
    assert_eq!(after.cell_at((2, 3)), AUTO);
    assert_eq!(after.cell_at(start), VAC);
}

#[test]
fn two_player_game_reaches_winner_in_four_rounds() {
    // Direct equivalent of `v8n/tests/win_condition.rs::a_two_player_game_reaches_a_winner`
    let mut b = Board::empty_with_size(5, true);
    b.set_cell((0, 2), REP);
    b.set_cell((4, 1), REP);

    let goals = stock_goals2(5).expect("stock_goals2(5)");
    assert_eq!(
        goals,
        vec![((0, 0), 0), ((4, 0), 0), ((0, 4), 1), ((4, 4), 1)]
    );

    let idle = [
        Move {
            who: 0,
            frm: (0, 1),
            to: (1, 1),
        },
        Move {
            who: 1,
            frm: (2, 1),
            to: (3, 1),
        },
    ];

    let expected_walk = [(3, 2), (4, 2), (4, 3), (4, 4)];
    for (round_idx, &expected) in expected_walk.iter().enumerate() {
        let out = round(&b, &goals, &[], &[], &[0, 1], &idle).expect("round");
        match out {
            RoundOutcome::Resolved { board, win } => {
                assert_eq!(
                    board.auto, expected,
                    "round {round_idx}: unexpected automaton position"
                );
                if round_idx < 3 {
                    assert_eq!(win, None, "game ended early at round {round_idx}");
                } else {
                    assert_eq!(win, Some(1), "seat 1 should win on round 3 at (4,4)");
                }
                b = board;
            }
            RoundOutcome::Again { .. } => panic!("idle moves must not conflict"),
        }
    }
}

#[test]
fn conformance_audit_pins_two_cycle_inclusive_path_and_ruling_d() {
    // 1. Two-cycle stays put by default
    let mut cyc = Board::empty_with_size(5, true);
    cyc.set_cell((4, 4), AUTO);
    cyc.set_cell((2, 2), VAC);
    cyc.set_cell((0, 0), ATT);
    cyc.set_cell((0, 1), REP);

    let cyc_moves = [
        Move {
            who: 0,
            frm: (0, 0),
            to: (0, 1),
        },
        Move {
            who: 1,
            frm: (0, 1),
            to: (0, 0),
        },
    ];
    let after_cyc = resolve_mid(&cyc, &[], &cyc_moves).expect("resolve_mid 2-cycle");
    assert_eq!(after_cyc.cell_at((0, 0)), ATT, "2-cycle must stay put");
    assert_eq!(after_cyc.cell_at((0, 1)), REP, "2-cycle must stay put");

    // 2. Inclusive path check blocks landing on a stationary occupant
    let mut blk = Board::empty_with_size(5, true);
    blk.set_cell((4, 4), AUTO);
    blk.set_cell((2, 2), VAC);
    blk.set_cell((0, 0), ATT);
    blk.set_cell((0, 3), REP);
    let blk_moves = [Move {
        who: 0,
        frm: (0, 0),
        to: (0, 3),
    }];
    let after_blk = resolve_mid(&blk, &[], &blk_moves).expect("resolve_mid blocked");
    assert_eq!(after_blk.cell_at((0, 0)), ATT, "blocked mover stays at origin");
    assert_eq!(
        after_blk.cell_at((0, 3)),
        REP,
        "stationary occupant is not overwritten"
    );

    // 3. Ruling (D): automaton square banned as source only, legal as destination proposal
    let stock = stock_two_player().expect("stock");
    assert!(
        !move_legal(
            &stock,
            &[],
            &Move {
                who: 0,
                frm: (5, 5),
                to: (5, 4),
            }
        )
        .unwrap()
    );
    assert!(
        move_legal(
            &stock,
            &[],
            &Move {
                who: 0,
                frm: (5, 4),
                to: (5, 5),
            }
        )
        .unwrap()
    );

    // 4. Proposable vs Live targets gap on the stock opening at (3, 1)
    let prop = legal_targets(&stock, &[], 0, (3, 1)).expect("legal_targets");
    let live = executable_targets(&stock, &[], &[], 0, (3, 1)).expect("executable_targets");
    assert_eq!(prop.len(), 20);
    assert_eq!(live.len(), 11);
}

#[test]
fn r2_variants_presets_confluence_swaps_fling_cooldowns() {
    // 1. Board presets: 7x7 Blitz, 9x9 Tactical, 11x11 Classic
    let b7 = preset_board(BoardPreset::Blitz7).expect("blitz7");
    assert_eq!(b7.n, 7);
    assert_eq!(b7.auto, (3, 3));

    let b9 = preset_board(BoardPreset::Tactical9).expect("tactical9");
    assert_eq!(b9.n, 9);
    assert_eq!(b9.auto, (4, 4));

    let b11 = preset_board(BoardPreset::Classic11).expect("classic11");
    assert_eq!(b11, stock_two_player().unwrap());

    // 2. BunchBefore vs Annihilate on a 3x3 board
    let mut b3 = Board::empty_with_size(3, true);
    b3.set_cell((0, 0), REP);
    b3.set_cell((2, 2), ATT);
    let colliding = [
        Move {
            who: 0,
            frm: (0, 0),
            to: (2, 0),
        },
        Move {
            who: 1,
            frm: (2, 2),
            to: (2, 0),
        },
    ];

    let cfg_bunch = GameConfig {
        confluence: ConfluenceMode::BunchBefore,
        ..GameConfig::default()
    };
    let after_bunch = resolve_mid_cfg(&cfg_bunch, &b3, &[], &colliding).expect("bunchBefore");
    // Merge cell (2,0) stays empty; (0,0)->(2,0) stops 1 short at (1,0); (2,2)->(2,0) stops 1 short at (2,1)
    assert_eq!(after_bunch.cell_at((2, 0)), VAC);
    assert_eq!(after_bunch.cell_at((1, 0)), REP);
    assert_eq!(after_bunch.cell_at((2, 1)), ATT);

    let cfg_annihilate = GameConfig {
        confluence: ConfluenceMode::Annihilate,
        ..GameConfig::default()
    };
    let after_ann = resolve_mid_cfg(&cfg_annihilate, &b3, &[], &colliding).expect("annihilate");
    assert_eq!(after_ann.cell_at((0, 0)), VAC);
    assert_eq!(after_ann.cell_at((2, 2)), VAC);
    assert_eq!(after_ann.cell_at((2, 0)), VAC);

    // 3. two_cycle_swaps = true swaps adjacent pieces
    let mut swap_b = Board::empty_with_size(3, true);
    swap_b.set_cell((0, 0), REP);
    swap_b.set_cell((1, 0), ATT);
    let swap_moves = [
        Move {
            who: 0,
            frm: (0, 0),
            to: (1, 0),
        },
        Move {
            who: 1,
            frm: (1, 0),
            to: (0, 0),
        },
    ];
    let cfg_swap = GameConfig {
        two_cycle_swaps: true,
        ..GameConfig::default()
    };
    let after_swap = resolve_mid_cfg(&cfg_swap, &swap_b, &[], &swap_moves).expect("two_cycle_swaps");
    assert_eq!(after_swap.cell_at((0, 0)), ATT);
    assert_eq!(after_swap.cell_at((1, 0)), REP);

    // 4. railgun_fling = true slides automaton across empty cells
    let mut fling_b = Board::empty_with_size(5, true);
    fling_b.set_cell((2, 2), VAC);
    fling_b.set_cell((0, 2), AUTO);
    fling_b.set_cell((4, 2), ATT);
    let cfg_fling = GameConfig {
        railgun_fling: true,
        ..GameConfig::default()
    };
    let after_fling = automaton_step_cfg(&cfg_fling, &fling_b, None).expect("railgun_fling");
    assert_eq!(after_fling.auto, (3, 2));

    // 5. cooldown_turns = 2 records cooldowns and blocks moving a piece on cooldown
    let mut cd_b = Board::empty_with_size(3, true);
    cd_b.set_cell((0, 0), REP);
    let cfg_cd = GameConfig {
        cooldown_turns: 2,
        ..GameConfig::default()
    };
    let out1 = round_cfg(
        &cfg_cd,
        None,
        &cd_b,
        &[],
        &[],
        &[],
        &[0],
        &[],
        &[Move {
            who: 0,
            frm: (0, 0),
            to: (2, 0),
        }],
    )
    .expect("round_cfg 1");
    match out1 {
        VariantRoundOutcome::Resolved {
            board,
            cooldowns,
            ..
        } => {
            assert_eq!(board.cell_at((2, 0)), REP);
            assert_eq!(cooldowns, vec![((2, 0), 2)]);
            // Trying to move (2, 0) on the next round while on cooldown is filtered out as illegal!
            let out2 = round_cfg(
                &cfg_cd,
                None,
                &board,
                &[],
                &[],
                &[],
                &[0],
                &cooldowns,
                &[Move {
                    who: 0,
                    frm: (2, 0),
                    to: (0, 0),
                }],
            )
            .expect("round_cfg 2");
            match out2 {
                VariantRoundOutcome::Resolved {
                    board: b2,
                    cooldowns: cds2,
                    ..
                } => {
                    assert_eq!(
                        b2.cell_at((2, 0)),
                        REP,
                        "piece on cooldown must not move"
                    );
                    assert_eq!(cds2, vec![((2, 0), 1)], "cooldown ticks down from 2 to 1");
                }
                VariantRoundOutcome::Again { .. } => panic!("unexpected conflict"),
            }
        }
        VariantRoundOutcome::Again { .. } => panic!("unexpected conflict"),
    }
}

#[test]
fn packed_board_equivalence_with_wire_oracle() {
    for preset in [
        BoardPreset::Blitz7,
        BoardPreset::Tactical9,
        BoardPreset::Classic11,
    ] {
        let pb = PackedBoard::stock(preset);
        let wb = preset_board(preset).expect("preset_board");
        assert_eq!(pb.to_board(), wb, "PackedBoard::stock mismatch for {preset:?}");
        assert_eq!(pb.adjudicate(), None, "opening board must adjudicate to draw");
    }

    // Legal targets bitmask equivalence on 11x11 Classic stock board
    let pb11 = PackedBoard::stock(BoardPreset::Classic11);
    let wb11 = stock_two_player().unwrap();
    let prop_mask = pb11.legal_targets_mask(0, (3, 1), false);
    let live_mask = pb11.legal_targets_mask(0, (3, 1), true);
    assert_eq!(prop_mask.count_ones(), 20);
    assert_eq!(live_mask.count_ones(), 11);
    assert_eq!(
        prop_mask.to_coords(11),
        legal_targets(&wb11, &[], 0, (3, 1)).unwrap()
    );
    assert_eq!(
        live_mask.to_coords(11),
        executable_targets(&wb11, &[], &[], 0, (3, 1)).unwrap()
    );

    // Step equivalence on a clean round AND a conflicted round
    let clean_moves = [
        Move {
            who: 0,
            frm: (3, 1),
            to: (3, 3),
        },
        Move {
            who: 1,
            frm: (7, 9),
            to: (7, 7),
        },
    ];
    let goals11 = stock_goals2(11).unwrap();
    let wire_clean = round(&wb11, &goals11, &[], &[], &[0, 1], &clean_moves).unwrap();
    let packed_clean = pb11.step(&clean_moves).unwrap();
    match (wire_clean, packed_clean) {
        (
            RoundOutcome::Resolved {
                board: wb_after,
                win: w_win,
            },
            PackedOutcome::Resolved {
                board: pb_after,
                win: p_win,
            },
        ) => {
            assert_eq!(pb_after.to_board(), wb_after);
            assert_eq!(p_win, w_win);
        }
        other => panic!("expected both to resolve cleanly, got {other:?}"),
    }

    // Conflicted round (both players fork (3, 1))
    let fork_moves = [
        Move {
            who: 0,
            frm: (3, 1),
            to: (3, 3),
        },
        Move {
            who: 1,
            frm: (3, 1),
            to: (3, 4),
        },
    ];
    let wire_fork = round(&wb11, &goals11, &[], &[], &[0, 1], &fork_moves).unwrap();
    let packed_fork = pb11.step(&fork_moves).unwrap();
    match (wire_fork, packed_fork) {
        (
            RoundOutcome::Again {
                marks: w_marks,
                locked: w_locked,
                waiting: w_waiting,
            },
            PackedOutcome::Again(pb_again),
        ) => {
            assert_eq!(pb_again.marks(), w_marks);
            assert_eq!(pb_again.locked(), w_locked);
            assert_eq!(pb_again.waiting(), w_waiting);
        }
        other => panic!("expected both to return Again, got {other:?}"),
    }
}

#[test]
fn tiebreak_modes_column_row_freeze_momentum() {
    // Symmetric board with attractors at (4, 2) [+X] and (2, 4) [+Y], automaton at (2, 2).
    // Both +X and +Y have Priority 3 (towardAttractor) at distance 2.
    let mut b = Board::empty_with_size(5, true);
    b.set_cell((4, 2), ATT);
    b.set_cell((2, 4), ATT);

    // 1. Column tie-break prefers Y (vertical): steps to (2, 3)
    let cfg_col = GameConfig {
        tie_break: TieBreak::Column,
        ..GameConfig::default()
    };
    let b_col = automaton_step_cfg(&cfg_col, &b, None).expect("column");
    assert_eq!(b_col.auto, (2, 3));

    // 2. Row tie-break prefers X (horizontal): steps to (3, 2)
    let cfg_row = GameConfig {
        tie_break: TieBreak::Row,
        ..GameConfig::default()
    };
    let b_row = automaton_step_cfg(&cfg_row, &b, None).expect("row");
    assert_eq!(b_row.auto, (3, 2));

    // 3. Freeze tie-break stays put when tied: remains at (2, 2)
    let cfg_freeze = GameConfig {
        tie_break: TieBreak::Freeze,
        ..GameConfig::default()
    };
    let b_freeze = automaton_step_cfg(&cfg_freeze, &b, None).expect("freeze");
    assert_eq!(b_freeze.auto, (2, 2));

    // 4. Momentum (prefer_same_axis = true) with last_dir = XP prefers +X -> (3, 2)
    let cfg_mom_same = GameConfig {
        tie_break: TieBreak::Momentum {
            prefer_same_axis: true,
        },
        ..GameConfig::default()
    };
    let b_mom_x = automaton_step_cfg(&cfg_mom_same, &b, Some(Dir::Xp)).expect("momentum same XP");
    assert_eq!(b_mom_x.auto, (3, 2));

    // 5. Momentum (prefer_same_axis = false -> prefer orthogonal axis) with last_dir = XP prefers +Y -> (2, 3)
    let cfg_mom_orth = GameConfig {
        tie_break: TieBreak::Momentum {
            prefer_same_axis: false,
        },
        ..GameConfig::default()
    };
    let b_mom_y = automaton_step_cfg(&cfg_mom_orth, &b, Some(Dir::Xp)).expect("momentum orth XP");
    assert_eq!(b_mom_y.auto, (2, 3));
}

#[test]
fn packed_board_variant_equivalence() {
    // Verify PackedBoard::pack + PackedBoard::step matches round_cfg across BunchBefore + Cooldowns + Fling
    let b = preset_board(BoardPreset::Blitz7).unwrap();
    let goals = stock_goals2(7).unwrap();
    let cfg = GameConfig {
        tie_break: TieBreak::Row,
        confluence: ConfluenceMode::BunchBefore,
        preset: BoardPreset::Blitz7,
        two_cycle_swaps: true,
        cooldown_turns: 2,
        railgun_fling: true,
        automata_count: 1,
    };
    let pb = PackedBoard::pack(&b, &cfg, &[0, 1], &[], &[], &[], None);
    assert_eq!(pb.config(), cfg);
    assert_eq!(pb.to_board(), b);

    // Colliding moves on 7x7 Blitz: under BunchBefore, no Again occurs; round resolves immediately!
    let moves = [
        Move {
            who: 0,
            frm: (2, 0),
            to: (2, 2),
        },
        Move {
            who: 1,
            frm: (0, 2),
            to: (2, 2),
        },
    ];
    let wire_out = round_cfg(&cfg, None, &b, &goals, &[], &[], &[0, 1], &[], &moves).unwrap();
    let packed_out = pb.step(&moves).unwrap();
    match (wire_out, packed_out) {
        (
            VariantRoundOutcome::Resolved {
                board: wb_after,
                win: w_win,
                cooldowns: w_cd,
                ..
            },
            PackedOutcome::Resolved {
                board: pb_after,
                win: p_win,
            },
        ) => {
            assert_eq!(pb_after.to_board(), wb_after);
            assert_eq!(p_win, w_win);
            assert_eq!(pb_after.cooldowns(), w_cd);
        }
        other => panic!("expected both BunchBefore steps to resolve, got {other:?}"),
    }
}

#[test]
fn fail_closed_on_malformed_wire() {
    assert_eq!(rules_wire(""), "0");
    assert_eq!(rules_wire("unknown_verb 1 2 3"), "0");
    assert_eq!(rules_wire("dist"), "0");
    assert_eq!(rules_wire("adjudicate 5"), "0");
}

#[test]
fn bunch_before_caterpillar_merge_conserves_pieces_and_is_permutation_invariant() {
    // 1. 3-mover caterpillar + merge collision on a 3x3 board:
    // m1: (0,1)->(0,0) [caterpillar follower behind (0,0)]
    // m2: (0,0)->(1,0) [collides at (1,0)]
    // m3: (2,0)->(1,0) [collides at (1,0)]
    let mut b3 = Board::empty_with_size(3, true);
    b3.set_cell((0, 0), REP);
    b3.set_cell((0, 1), REP);
    b3.set_cell((2, 0), ATT);

    let m1 = Move {
        who: 0,
        frm: (0, 1),
        to: (0, 0),
    };
    let m2 = Move {
        who: 0,
        frm: (0, 0),
        to: (1, 0),
    };
    let m3 = Move {
        who: 1,
        frm: (2, 0),
        to: (1, 0),
    };

    let cfg_bunch = GameConfig {
        confluence: ConfluenceMode::BunchBefore,
        ..GameConfig::default()
    };

    let perms = [
        [m1, m2, m3],
        [m1, m3, m2],
        [m2, m1, m3],
        [m2, m3, m1],
        [m3, m1, m2],
        [m3, m2, m1],
    ];
    for (idx, p) in perms.iter().enumerate() {
        let after = resolve_mid_cfg(&cfg_bunch, &b3, &[], p).expect("bunchBefore caterpillar");
        assert_eq!(
            after.cell_at((1, 0)),
            VAC,
            "perm {idx}: contested merge square (1,0) must remain empty"
        );
        assert_eq!(
            after.cell_at((0, 0)),
            REP,
            "perm {idx}: (0,0) must not be destroyed"
        );
        assert_eq!(
            after.cell_at((0, 1)),
            REP,
            "perm {idx}: caterpillar follower (0,1) must back up to (0,1) and not overwrite (0,0)"
        );
        assert_eq!(
            after.cell_at((2, 0)),
            ATT,
            "perm {idx}: (2,0) must stay at (2,0)"
        );
    }

    // 2. Multi-cell caterpillar slide into blocked leader:
    // On a 4x4 board, s1=(0,2)->(0,0) slides 2 cells toward s2=(0,0), while s2=(0,0)->(1,0) and
    // s3=(2,0)->(1,0) collide at (1,0). s2 stays at (0,0), so s1 bunches 1 cell behind s2 at (0,1)!
    let mut b4 = Board::empty_with_size(4, true);
    b4.set_cell((3, 3), AUTO);
    b4.set_cell((2, 2), VAC);
    b4.set_cell((0, 0), REP);
    b4.set_cell((0, 2), ATT);
    b4.set_cell((2, 0), REP);
    let slide_caterpillar = [
        Move {
            who: 0,
            frm: (0, 2),
            to: (0, 0),
        },
        Move {
            who: 0,
            frm: (0, 0),
            to: (1, 0),
        },
        Move {
            who: 1,
            frm: (2, 0),
            to: (1, 0),
        },
    ];
    let after4 = resolve_mid_cfg(&cfg_bunch, &b4, &[], &slide_caterpillar).expect("slide bunch");
    assert_eq!(after4.cell_at((1, 0)), VAC, "contested square stays empty");
    assert_eq!(after4.cell_at((0, 0)), REP, "leader stays at (0,0)");
    assert_eq!(after4.cell_at((2, 0)), REP, "other collider stays at (2,0)");
    assert_eq!(
        after4.cell_at((0, 1)),
        ATT,
        "follower sliding from (0,2)->(0,0) bunches at (0,1) right behind (0,0)"
    );
    assert_eq!(after4.cell_at((0, 2)), VAC, "follower vacated (0,2)");

    // 3. Cooldowns in BunchBefore mode are placed on the actual bunched landing squares,
    // not the empty contested merge square!
    let mut b3_cd = Board::empty_with_size(3, true);
    b3_cd.set_cell((0, 0), REP);
    b3_cd.set_cell((2, 2), ATT);
    let cfg_bunch_cd = GameConfig {
        confluence: ConfluenceMode::BunchBefore,
        cooldown_turns: 2,
        ..GameConfig::default()
    };
    let colliding = [
        Move {
            who: 0,
            frm: (0, 0),
            to: (2, 0),
        },
        Move {
            who: 1,
            frm: (2, 2),
            to: (2, 0),
        },
    ];
    let out = round_cfg(
        &cfg_bunch_cd,
        None,
        &b3_cd,
        &[],
        &[],
        &[],
        &[0, 1],
        &[],
        &colliding,
    )
    .expect("round_cfg bunch cooldown");
    match out {
        VariantRoundOutcome::Resolved { cooldowns, .. } => {
            assert_eq!(
                cooldowns,
                vec![((1, 0), 2), ((2, 1), 2)],
                "cooldowns must be placed on actual bunched squares (1,0) and (2,1), not empty (2,0)"
            );
        }
        VariantRoundOutcome::Again { .. } => panic!("bunchBefore must not return Again"),
    }
}

#[test]
fn packed_board_legal_targets_mask_respects_cooldowns_for_both_live_and_proposable() {
    let mut b = Board::empty_with_size(5, true);
    b.set_cell((2, 0), REP);
    b.set_cell((0, 2), ATT);
    let cfg = GameConfig {
        cooldown_turns: 2,
        ..GameConfig::default()
    };
    let pb = PackedBoard::pack(&b, &cfg, &[0, 1], &[], &[], &[((2, 0), 2)], None);

    // (2, 0) is on active cooldown: BOTH live_only=true and live_only=false must return ZERO!
    assert_eq!(
        pb.legal_targets_mask(0, (2, 0), true),
        Bitmask128::ZERO,
        "live_only=true must return 0 targets for square on cooldown"
    );
    assert_eq!(
        pb.legal_targets_mask(0, (2, 0), false),
        Bitmask128::ZERO,
        "live_only=false must also return 0 targets for square on cooldown"
    );

    // (0, 2) is NOT on cooldown: both masks should be non-empty
    assert!(pb.legal_targets_mask(0, (0, 2), true).count_ones() > 0);
    assert!(pb.legal_targets_mask(0, (0, 2), false).count_ones() > 0);
}

#[test]
fn packed_board_four_player_goals_preservation_and_step() {
    // 1. 4-player stock Classic11 board: goals and waiting seats [0, 1, 2, 3]
    let pb4 = PackedBoard::stock(BoardPreset::Classic11).with_four_player_goals();
    let expected_goals11 = vec![((0, 0), 0), ((10, 0), 1), ((10, 10), 2), ((0, 10), 3)];
    assert_eq!(pb4.goals(), expected_goals11);
    assert_eq!(pb4.waiting(), vec![0, 1, 2, 3]);

    // All 4 seats submit distinct non-conflicting moves
    let moves_4p = [
        Move {
            who: 0,
            frm: (3, 1),
            to: (3, 3),
        },
        Move {
            who: 1,
            frm: (7, 1),
            to: (7, 3),
        },
        Move {
            who: 2,
            frm: (7, 9),
            to: (7, 7),
        },
        Move {
            who: 3,
            frm: (3, 9),
            to: (3, 7),
        },
    ];
    let wb11 = stock_two_player().unwrap();
    let wire_out = round(&wb11, &expected_goals11, &[], &[], &[0, 1, 2, 3], &moves_4p).unwrap();
    let packed_out = pb4.step(&moves_4p).unwrap();
    match (wire_out, packed_out) {
        (
            RoundOutcome::Resolved {
                board: wb_after,
                win: w_win,
            },
            PackedOutcome::Resolved {
                board: pb_after,
                win: p_win,
            },
        ) => {
            assert_eq!(pb_after.to_board(), wb_after);
            assert_eq!(p_win, w_win);
            assert_eq!(
                pb_after.goals(),
                expected_goals11,
                "4-player goals must be preserved across PackedBoard::step"
            );
            assert_eq!(
                pb_after.waiting(),
                vec![0, 1, 2, 3],
                "next round after Resolved must open with all 4 active seats [0, 1, 2, 3]"
            );
        }
        other => panic!("expected 4-player clean round to resolve, got {other:?}"),
    }

    // 2. 4-player conflict re-entry: seats 2 & 3 fork (3, 9) while seats 0 & 1 lock in cleanly
    let fork_4p = [
        Move {
            who: 0,
            frm: (3, 1),
            to: (3, 3),
        },
        Move {
            who: 1,
            frm: (7, 1),
            to: (7, 3),
        },
        Move {
            who: 2,
            frm: (3, 9),
            to: (3, 7),
        },
        Move {
            who: 3,
            frm: (3, 9),
            to: (3, 6),
        },
    ];
    let pb_again = match pb4.step(&fork_4p).unwrap() {
        PackedOutcome::Again(pb_again) => {
            assert_eq!(pb_again.marks(), vec![(3, 9)]);
            assert_eq!(pb_again.waiting(), vec![2, 3]);
            assert_eq!(pb_again.locked().len(), 2);
            assert_eq!(pb_again.goals(), expected_goals11);
            pb_again
        }
        other => panic!("expected Again on 4-player fork, got {other:?}"),
    };
    let reentry_23 = [
        Move {
            who: 2,
            frm: (7, 9),
            to: (7, 7),
        },
        Move {
            who: 3,
            frm: (6, 9),
            to: (6, 7),
        },
    ];
    match pb_again.step(&reentry_23).unwrap() {
        PackedOutcome::Resolved { board: pb_done, win } => {
            assert_eq!(win, None);
            assert_eq!(pb_done.goals(), expected_goals11);
            assert_eq!(pb_done.waiting(), vec![0, 1, 2, 3]);
        }
        other => panic!("expected re-entry to resolve cleanly, got {other:?}"),
    }

    // 3. 4-player corner win detection distinguishes Seat 2 ((4,4)) and Seat 3 ((0,4)) from 2-player Seat 1
    let mut b5_seat2 = Board::empty_with_size(5, true);
    b5_seat2.set_cell((2, 2), VAC);
    b5_seat2.set_cell((4, 3), AUTO);
    b5_seat2.set_cell((4, 1), REP); // repulsor at distance 2 in -Y pushes automaton +Y into (4, 4)
    let pb5_seat2 = PackedBoard::pack(
        &b5_seat2,
        &GameConfig::default(),
        &[0, 1],
        &[],
        &[],
        &[],
        None,
    )
    .with_four_player_goals();
    assert_eq!(pb5_seat2.waiting(), vec![0, 1, 2, 3]);
    match pb5_seat2.step(&[]).unwrap() {
        PackedOutcome::Resolved { board, win } => {
            assert_eq!(board.auto_coord(), (4, 4));
            assert_eq!(
                win,
                Some(2),
                "Seat 2 owns corner (4,4) in 4-player mode (not Seat 1)"
            );
        }
        other => panic!("expected Seat 2 win resolution, got {other:?}"),
    }

    let mut b5_seat3 = Board::empty_with_size(5, true);
    b5_seat3.set_cell((2, 2), VAC);
    b5_seat3.set_cell((0, 3), AUTO);
    b5_seat3.set_cell((0, 1), REP); // repulsor at distance 2 in -Y pushes automaton +Y into (0, 4)
    let pb5_seat3 = PackedBoard::pack(
        &b5_seat3,
        &GameConfig::default(),
        &[0, 1],
        &[],
        &[],
        &[],
        None,
    )
    .with_four_player_goals();
    match pb5_seat3.step(&[]).unwrap() {
        PackedOutcome::Resolved { board, win } => {
            assert_eq!(board.auto_coord(), (0, 4));
            assert_eq!(
                win,
                Some(3),
                "Seat 3 owns corner (0,4) in 4-player mode (not Seat 1)"
            );
        }
        other => panic!("expected Seat 3 win resolution, got {other:?}"),
    }

    // 4. Backwards compatibility & canonicalization: legacy buffer without trailing goals bytes
    // decodes with stockGoals2 and compares equal (==) to PackedBoard::stock(Classic11)
    let pb2 = PackedBoard::stock(BoardPreset::Classic11);
    let pb2_bytes = pb2.as_bytes();
    let legacy_len = pb2_bytes.len() - (1 + 4 * 3);
    let legacy_pb = PackedBoard::from_bytes(&pb2_bytes[..legacy_len])
        .expect("legacy buffer without goals trailer must still decode");
    assert_eq!(
        legacy_pb.goals(),
        PackedBoard::stock_goals2_for_size(11),
        "legacy buffer without goals trailer must fall back to stockGoals2"
    );
    assert_eq!(
        legacy_pb, pb2,
        "legacy 2P buffer must canonicalize to equal PackedBoard::stock(Classic11)"
    );

    // Empty goals slice in pack_with_goals also canonicalizes to stockGoals2
    let empty_goals_pb = PackedBoard::pack_with_goals(
        &pb2.to_board(),
        &pb2.config(),
        &[],
        &[0, 1],
        &[],
        &[],
        &[],
        None,
    );
    assert_eq!(empty_goals_pb, pb2);

    // Trailing garbage bytes after goals section must be rejected
    let mut trailing = pb4.as_bytes().to_vec();
    trailing.push(0x99);
    assert!(
        PackedBoard::from_bytes(&trailing).is_none(),
        "PackedBoard::from_bytes must reject trailing garbage bytes after goals section"
    );

    // 5. Calling with_goals mid-round (when marks/locked are non-empty) preserves waiting seats
    let mid_round_reconfigured = pb_again.with_goals(&expected_goals11);
    assert_eq!(
        mid_round_reconfigured.waiting(),
        vec![2, 3],
        "with_goals mid-round must preserve active waiting seats [2, 3] instead of resetting to [0, 1, 2, 3]"
    );

    // 6. Non-contiguous duplicate seat order in custom goals matches Lean's last-occurrence List.dedup
    let interleaved_goals = vec![
        ((0, 0), 0),
        ((0, 10), 1),
        ((10, 10), 1),
        ((10, 0), 0),
    ];
    let pb_interleaved = pb2.with_goals(&interleaved_goals);
    assert_eq!(
        pb_interleaved.waiting(),
        vec![1, 0],
        "with_goals on fresh round must match Lean GoalAssignment.seats last-occurrence dedup [1, 0]"
    );
    match pb_interleaved.step(&[]).unwrap() {
        PackedOutcome::Resolved { board: pb_next, .. } => {
            assert_eq!(
                pb_next.waiting(),
                vec![1, 0],
                "Lean stepVariant and Rust with_goals must agree on waiting seat order [1, 0]"
            );
        }
        other => panic!("expected clean empty step to resolve, got {other:?}"),
    }
}

#[test]
fn lean_runtime_zero_memory_leaks_across_repeated_steps_and_queries() {
    // Warm up all lazy Lean module/closed constants first
    let pb = PackedBoard::stock(BoardPreset::Classic11);
    let wb = stock_two_player().unwrap();
    let goals = stock_goals2(11).unwrap();
    let moves = [
        Move {
            who: 0,
            frm: (3, 1),
            to: (3, 3),
        },
        Move {
            who: 1,
            frm: (7, 9),
            to: (7, 7),
        },
    ];
    let _ = pb.step(&moves).unwrap();
    let _ = pb.legal_targets_mask(0, (3, 1), true);
    let _ = pb.legal_targets_mask(0, (3, 1), false);
    let _ = pb.adjudicate();
    let _ = round(&wb, &goals, &[], &[], &[0, 1], &moves).unwrap();

    let baseline = live_lean_allocs();
    assert_eq!(
        baseline, 0,
        "all non-persistent Lean heap objects must be freed after FFI calls"
    );
    for _ in 0..50 {
        let out = pb.step(&moves).unwrap();
        if let PackedOutcome::Resolved { board: pb2, .. } = out {
            let _ = pb2.legal_targets_mask(0, (3, 3), true);
            let _ = pb2.legal_targets_mask(0, (3, 3), false);
            let _ = pb2.adjudicate();
        }
        let _ = round(&wb, &goals, &[], &[], &[0, 1], &moves).unwrap();
    }
    assert_eq!(
        live_lean_allocs(),
        0,
        "Lean C runtime shim must free all temporary heap objects without leaking"
    );
}

#[test]
fn multithreaded_lock_free_packed_step_and_rules_wire_stress() {
    use std::sync::{Arc, Barrier};
    use std::thread;

    const NUM_THREADS: usize = 8;
    const TURNS_PER_THREAD: usize = 60;

    let barrier = Arc::new(Barrier::new(NUM_THREADS));
    let mut handles = Vec::with_capacity(NUM_THREADS);

    for tid in 0..NUM_THREADS {
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let preset = match tid % 3 {
                0 => BoardPreset::Classic11,
                1 => BoardPreset::Tactical9,
                _ => BoardPreset::Blitz7,
            };
            barrier.wait();

            let mut pb = PackedBoard::stock(preset);
            for turn_idx in 0..TURNS_PER_THREAD {
                // Concurrent wire oracle query
                let wire_resp = rules_wire("goals 11");
                assert!(wire_resp.starts_with("1 "));

                // Query legal targets & adjudication on the packed board
                let b = pb.to_board();
                let n_i32 = b.n as i32;
                let mut chosen_moves = Vec::new();
                'seat_loop: for &seat in &[0u32, 1u32] {
                    for y in 0..n_i32 {
                        for x in 0..n_i32 {
                            let c = b.cell_at((x, y));
                            if c == REP || c == ATT {
                                let mask = pb.legal_targets_mask(seat, (x, y), true);
                                let targets = mask.to_coords(b.n);
                                if !targets.is_empty() {
                                    let pick = targets[(turn_idx + tid + seat as usize) % targets.len()];
                                    if chosen_moves.iter().all(|m: &Move| m.frm != (x, y) && m.to != pick) {
                                        chosen_moves.push(Move {
                                            who: seat,
                                            frm: (x, y),
                                            to: pick,
                                        });
                                        continue 'seat_loop;
                                    }
                                }
                            }
                        }
                    }
                }

                match pb.step(&chosen_moves).expect("concurrent PackedBoard::step") {
                    PackedOutcome::Resolved { board: next_pb, win } => {
                        let _ = next_pb.adjudicate();
                        if win.is_some() {
                            pb = PackedBoard::stock(preset);
                        } else {
                            pb = next_pb;
                        }
                    }
                    PackedOutcome::Again(again_pb) => {
                        // Resolve remaining waiting seats with empty move slice
                        if let PackedOutcome::Resolved { board: next_pb, .. } =
                            again_pb.step(&[]).expect("resolve Again")
                        {
                            pb = next_pb;
                        } else {
                            pb = PackedBoard::stock(preset);
                        }
                    }
                }
            }
        }));
    }

    for h in handles {
        h.join().expect("worker thread panicked during concurrent Lean FFI stress test");
    }

    assert_eq!(
        live_lean_allocs(),
        0,
        "all Lean heap allocations must be freed after 8-thread concurrent stress test"
    );
}

#[test]
fn packed_board_edge_cases_two_cycle_swap_auto_target_and_dirty_masks() {
    let b7 = preset_board(BoardPreset::Blitz7).unwrap();
    let goals7 = PackedBoard::stock_goals2_for_size(7);

    // 1. twoCycleSwaps = true swapping Attractor at (1,1) and Repulsor at (2,1)
    let cfg_swap = GameConfig {
        preset: BoardPreset::Blitz7,
        two_cycle_swaps: true,
        ..GameConfig::default()
    };
    let pb_swap = PackedBoard::pack_with_goals(&b7, &cfg_swap, &goals7, &[0, 1], &[], &[], &[], None);
    assert_eq!(pb_swap.cell_at((1, 1)), ATT);
    assert_eq!(pb_swap.cell_at((2, 1)), REP);
    let swap_moves = [
        Move { who: 0, frm: (1, 1), to: (2, 1) },
        Move { who: 1, frm: (2, 1), to: (1, 1) },
    ];
    let pb_swap_out = pb_swap.step(&swap_moves).unwrap();
    let wire_swap_out =
        round_cfg(&cfg_swap, None, &b7, &goals7, &[], &[], &[0, 1], &[], &swap_moves).unwrap();
    match (pb_swap_out, wire_swap_out) {
        (
            PackedOutcome::Resolved { board: pb_next, win: w1 },
            VariantRoundOutcome::Resolved { board: b_next, win: w2, .. },
        ) => {
            assert_eq!(w1, w2);
            assert_eq!(pb_next.to_board(), b_next);
            assert_eq!(pb_next.cell_at((1, 1)), REP);
            assert_eq!(pb_next.cell_at((2, 1)), ATT);
            assert!(pb_next.rep_mask().bitand(pb_next.att_mask()).is_empty());
        }
        other => panic!("expected Resolved for two-cycle swap, got {other:?}"),
    }

    // 2. Move targeting the automaton square (to == b.auto = (3,3), Ruling D: blocked without conflict)
    let cfg_def = GameConfig {
        preset: BoardPreset::Blitz7,
        ..GameConfig::default()
    };
    let pb_def = PackedBoard::pack_with_goals(&b7, &cfg_def, &goals7, &[0, 1], &[], &[], &[], None);
    let auto_moves = [
        Move { who: 0, frm: (3, 0), to: (3, 3) },
        Move { who: 1, frm: (4, 1), to: (3, 1) },
    ];
    let pb_auto_out = pb_def.step(&auto_moves).unwrap();
    let wire_auto_out =
        round_cfg(&cfg_def, None, &b7, &goals7, &[], &[], &[0, 1], &[], &auto_moves).unwrap();
    match (pb_auto_out, wire_auto_out) {
        (
            PackedOutcome::Resolved { board: pb_next, win: w1 },
            VariantRoundOutcome::Resolved { board: b_next, win: w2, .. },
        ) => {
            assert_eq!(w1, w2);
            assert_eq!(pb_next.to_board(), b_next);
            assert_eq!(pb_next.cell_at((3, 0)), REP, "piece targeting automaton must stay at (3,0)");
            assert_ne!(pb_next.auto_coord(), (3, 3), "automaton should step away from (3,3)");
            assert_eq!(pb_next.cell_at((3, 3)), VAC, "old automaton square (3,3) must become vacuum");
        }
        other => panic!("expected Resolved for move targeting automaton, got {other:?}"),
    }

    // 3. Dirty raw bytes with repMask & attMask bits set at the automaton square (3,3) and overlapping at (2,1):
    //    Verify from_bytes and step sanitize the masks and never leave a phantom piece at (3,3) after the automaton steps.
    let mut dirty_bytes = pb_def.as_bytes().to_vec();
    let auto_idx = 3 * 7 + 3; // 24
    let overlap_idx = 1 * 7 + 2; // 9 (repulsor on Blitz7)
    // Set bit 24 in both repMask (bytes 0..8) and attMask (bytes 16..24), and bit 9 in attMask too
    let mut rep_lo = u64::from_le_bytes(dirty_bytes[0..8].try_into().unwrap());
    let mut att_lo = u64::from_le_bytes(dirty_bytes[16..24].try_into().unwrap());
    rep_lo |= 1u64 << auto_idx;
    att_lo |= (1u64 << auto_idx) | (1u64 << overlap_idx);
    dirty_bytes[0..8].copy_from_slice(&rep_lo.to_le_bytes());
    dirty_bytes[16..24].copy_from_slice(&att_lo.to_le_bytes());

    let pb_sanitized = PackedBoard::from_bytes(&dirty_bytes).expect("valid length dirty buffer");
    assert!(!pb_sanitized.rep_mask().test_bit(auto_idx));
    assert!(!pb_sanitized.att_mask().test_bit(auto_idx));
    assert!(pb_sanitized.rep_mask().bitand(pb_sanitized.att_mask()).is_empty());

    match pb_sanitized.step(&auto_moves).unwrap() {
        PackedOutcome::Resolved { board: pb_after, .. } => {
            assert_ne!(pb_after.auto_coord(), (3, 3));
            assert_eq!(
                pb_after.cell_at((3, 3)),
                VAC,
                "stepping sanitized buffer must never spawn a phantom piece at the old automaton square (3,3)"
            );
        }
        other => panic!("expected Resolved on sanitized board, got {other:?}"),
    }
}


