//! Property-based (`proptest`) differential testing, universal algebraic game invariants,
//! and fail-closed binary/wire fuzzing gates for `automatafl-lean`.

use automatafl_lean::*;
use proptest::prelude::*;

// ============================================================================
// §1. Rich `proptest` Strategies for Arbitrary Boards, Configs & Move Graphs
// ============================================================================

fn arb_board_size() -> impl Strategy<Value = u8> {
    prop::sample::select(vec![5u8, 7, 9, 11])
}

fn arb_tie_break() -> impl Strategy<Value = TieBreak> {
    prop_oneof![
        Just(TieBreak::Column),
        Just(TieBreak::Row),
        Just(TieBreak::Freeze),
        Just(TieBreak::Momentum {
            prefer_same_axis: true
        }),
        Just(TieBreak::Momentum {
            prefer_same_axis: false
        }),
    ]
}

fn arb_confluence_mode() -> impl Strategy<Value = ConfluenceMode> {
    prop_oneof![
        Just(ConfluenceMode::Conflict),
        Just(ConfluenceMode::BunchBefore),
        Just(ConfluenceMode::Annihilate),
    ]
}

fn arb_opt_dir() -> impl Strategy<Value = Option<Dir>> {
    prop_oneof![
        Just(None),
        Just(Some(Dir::Xp)),
        Just(Some(Dir::Xn)),
        Just(Some(Dir::Yp)),
        Just(Some(Dir::Yn)),
    ]
}

fn preset_for_size(n: u8) -> BoardPreset {
    match n {
        7 => BoardPreset::Blitz7,
        9 => BoardPreset::Tactical9,
        _ => BoardPreset::Classic11,
    }
}

fn arb_game_config_for_size(n: u8) -> impl Strategy<Value = GameConfig> {
    (
        arb_tie_break(),
        arb_confluence_mode(),
        any::<bool>(),
        0u8..=3,
        any::<bool>(),
        1u8..=3,
    )
        .prop_map(
            move |(
                tie_break,
                confluence,
                two_cycle_swaps,
                cooldown_turns,
                railgun_fling,
                automata_count,
            )| {
                GameConfig {
                    tie_break,
                    confluence,
                    preset: preset_for_size(n),
                    two_cycle_swaps,
                    cooldown_turns,
                    railgun_fling,
                    automata_count,
                }
            },
        )
}

fn mask_to_board(mut m: Bitmask128, n: usize) -> Bitmask128 {
    let total = n * n;
    if total < 64 {
        m.lo &= (1u64 << total) - 1;
        m.hi = 0;
    } else if total == 64 {
        m.hi = 0;
    } else if total < 128 {
        m.hi &= (1u64 << (total - 64)) - 1;
    }
    m
}

/// Generate disjoint `(rep, att)` bitmasks on an `n × n` board with `auto` cleared,
/// spanning sparse, medium, and dense piece configurations.
fn arb_disjoint_masks(n: u8, auto: Coord) -> impl Strategy<Value = (Bitmask128, Bitmask128)> {
    let sz = n as usize;
    let auto_idx = (auto.1 as usize) * sz + (auto.0 as usize);
    (
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        0u8..3,
    )
        .prop_map(move |(r_lo, r_hi, a_lo, a_hi, d_lo, d_hi, density)| {
            let (eff_r_lo, eff_r_hi, eff_a_lo, eff_a_hi) = match density {
                // Sparse (~12-25% occupancy)
                0 => (r_lo & d_lo, r_hi & d_hi, a_lo & d_lo, a_hi & d_hi),
                // Medium (~25-50% occupancy)
                1 => (r_lo & (d_lo | a_lo), r_hi & (d_hi | a_hi), a_lo, a_hi),
                // Arbitrary full bitmask
                _ => (r_lo, r_hi, a_lo, a_hi),
            };
            let mut rep = mask_to_board(
                Bitmask128 {
                    lo: eff_r_lo,
                    hi: eff_r_hi,
                },
                sz,
            );
            let mut att = mask_to_board(
                Bitmask128 {
                    lo: eff_a_lo,
                    hi: eff_a_hi,
                },
                sz,
            );
            rep.clear_bit(auto_idx);
            att.clear_bit(auto_idx);
            att.lo &= !rep.lo;
            att.hi &= !rep.hi;
            (rep, att)
        })
}

/// Strategy producing an arbitrary valid `PackedBoard` across board sizes `{5, 7, 9, 11}`,
/// arbitrary disjoint `rep` and `att` bitmasks, arbitrary `auto`, `marks` (`0..=8`),
/// `cooldowns` (`0..=8` with `remaining in 1..=4`), `last_dir`, 2P/4P `goals`, and `GameConfig`.
fn arb_packed_board() -> impl Strategy<Value = PackedBoard> {
    arb_board_size().prop_flat_map(|n| {
        let ni = n as i32;
        (
            0..ni,
            0..ni,
            arb_game_config_for_size(n),
            any::<bool>(),
            arb_opt_dir(),
            any::<bool>(),
            prop::collection::vec((0..ni, 0..ni), 0..=8),
            prop::collection::vec(((0..ni, 0..ni), 1u32..=4), 0..=8),
        )
            .prop_flat_map(
                move |(ax, ay, cfg, col_rule, last_dir, four_player, raw_marks, raw_cds)| {
                    let auto = (ax, ay);
                    arb_disjoint_masks(n, auto).prop_map(move |(rep, att)| {
                        let sz = n as usize;
                        let mut cells = vec![VAC; sz * sz];
                        for y in 0..sz {
                            for x in 0..sz {
                                let idx = y * sz + x;
                                if (x as i32, y as i32) == auto {
                                    cells[idx] = AUTO;
                                } else if rep.test_bit(idx) {
                                    cells[idx] = REP;
                                } else if att.test_bit(idx) {
                                    cells[idx] = ATT;
                                }
                            }
                        }
                        let board = Board {
                            n: sz,
                            cells,
                            auto,
                            col_rule,
                        };
                        let mut cds: Vec<CooldownEntry> = Vec::new();
                        for &(c, rem) in &raw_cds {
                            if !cds.iter().any(|&(existing, _)| existing == c) {
                                cds.push((c, rem));
                            }
                        }
                        let (goals, waiting) = if four_player {
                            (PackedBoard::stock_goals4_for_size(sz), vec![0, 1, 2, 3])
                        } else {
                            (PackedBoard::stock_goals2_for_size(sz), vec![0, 1])
                        };
                        PackedBoard::pack_with_goals(
                            &board, &cfg, &goals, &waiting, &raw_marks, &[], &cds, last_dir,
                        )
                    })
                },
            )
    })
}

/// Collect all coordinates on `pb` holding a movable piece (`REP` or `ATT`).
fn piece_coords(pb: &PackedBoard) -> Vec<Coord> {
    let n = pb.size() as i32;
    let mut out = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let p = pb.cell_at((x, y));
            if p == REP || p == ATT {
                out.push((x, y));
            }
        }
    }
    out
}

/// Collect all rook-aligned in-bounds coordinates distinct from `c`.
fn rook_neighbors(n: i32, c: Coord) -> Vec<Coord> {
    let mut out = Vec::with_capacity((2 * n) as usize);
    for x in 0..n {
        if x != c.0 {
            out.push((x, c.1));
        }
    }
    for y in 0..n {
        if y != c.1 {
            out.push((c.0, y));
        }
    }
    out
}

/// Smart move-set generator producing both random legal rook moves from actual pieces
/// on `pb` AND structured adversarial move graphs:
/// - Multi-piece caterpillar chains (`A -> B, B -> C, C -> D`)
/// - 2-cycles (`A <-> B`) and 3-cycles (`A -> B -> C -> A`)
/// - Source forks (`A -> B` and `A -> C` by different seats)
/// - Destination collisions (`A -> D` and `B -> D`)
/// - Moves targeting `auto`, moves through occupied squares, moves from/to `marks`,
///   and moves from `cooldowns`.
fn arb_moves_for_board(pb: &PackedBoard) -> BoxedStrategy<Vec<Move>> {
    let pb_copy = *pb;
    let n = pb_copy.size() as i32;
    let waiting = pb_copy.waiting();
    let seats = if waiting.is_empty() {
        vec![0, 1]
    } else {
        waiting
    };
    (0u8..8, prop::collection::vec(any::<u16>(), 1..=8))
        .prop_map(move |(pattern, seeds)| {
            let pieces = piece_coords(&pb_copy);
            let marks = pb_copy.marks();
            let cds = pb_copy.cooldowns();
            let auto = pb_copy.auto_coord();
            let mut moves = Vec::new();

            let pick_seat = |idx: usize| -> u32 { seats[idx % seats.len()] };

            match pattern {
                // 0: Multi-piece caterpillar chain (A -> B, B -> C, C -> D)
                0 => {
                    let start = if !pieces.is_empty() {
                        pieces[seeds[0] as usize % pieces.len()]
                    } else {
                        ((seeds[0] as i32).rem_euclid(n), (seeds[0] as i32 / 7).rem_euclid(n))
                    };
                    let mut cur = start;
                    for (step_idx, &s) in seeds.iter().take(3).enumerate() {
                        // Try to chain through another piece on the same row/col if available,
                        // otherwise pick a rook neighbor.
                        let aligned_pieces: Vec<Coord> = pieces
                            .iter()
                            .copied()
                            .filter(|&q| {
                                q != cur
                                    && (q.0 == cur.0 || q.1 == cur.1)
                                    && !moves.iter().any(|m: &Move| m.frm == q)
                            })
                            .collect();
                        let next = if !aligned_pieces.is_empty() && step_idx + 1 < 3 {
                            aligned_pieces[s as usize % aligned_pieces.len()]
                        } else {
                            let r = rook_neighbors(n, cur);
                            r[s as usize % r.len()]
                        };
                        moves.push(Move {
                            who: pick_seat(step_idx),
                            frm: cur,
                            to: next,
                        });
                        cur = next;
                    }
                }
                // 1: 2-cycle (A <-> B) between two rook-aligned pieces or squares
                1 => {
                    let mut found_pair = None;
                    for &p1 in &pieces {
                        for &p2 in &pieces {
                            if p1 != p2 && (p1.0 == p2.0 || p1.1 == p2.1) {
                                found_pair = Some((p1, p2));
                                break;
                            }
                        }
                        if found_pair.is_some() {
                            break;
                        }
                    }
                    let (a, b) = found_pair.unwrap_or_else(|| {
                        let a = pieces.first().copied().unwrap_or((0, 0));
                        let r = rook_neighbors(n, a);
                        (a, r[seeds[0] as usize % r.len()])
                    });
                    moves.push(Move {
                        who: pick_seat(0),
                        frm: a,
                        to: b,
                    });
                    moves.push(Move {
                        who: pick_seat(1),
                        frm: b,
                        to: a,
                    });
                }
                // 2: 3-cycle (A -> B -> C -> A) along a shared row or column
                2 => {
                    let y = pieces
                        .first()
                        .map(|c| c.1)
                        .unwrap_or((seeds[0] as i32).rem_euclid(n));
                    let x0 = 0;
                    let x1 = 1 % n;
                    let x2 = 2 % n;
                    let (a, b, c) = ((x0, y), (x1, y), (x2, y));
                    moves.push(Move {
                        who: pick_seat(0),
                        frm: a,
                        to: b,
                    });
                    moves.push(Move {
                        who: pick_seat(1),
                        frm: b,
                        to: c,
                    });
                    moves.push(Move {
                        who: pick_seat(2 % seats.len()),
                        frm: c,
                        to: a,
                    });
                }
                // 3: Source fork (A -> B and A -> C by different seats) + non-clashing move
                3 => {
                    let a = pieces
                        .get(seeds[0] as usize % pieces.len().max(1))
                        .copied()
                        .unwrap_or((0, 0));
                    let r = rook_neighbors(n, a);
                    let b = r[seeds[0] as usize % r.len()];
                    let c = r[(seeds[0] as usize + 1) % r.len()];
                    moves.push(Move {
                        who: pick_seat(0),
                        frm: a,
                        to: b,
                    });
                    moves.push(Move {
                        who: pick_seat(1),
                        frm: a,
                        to: c,
                    });
                    if pieces.len() > 1 && seats.len() > 2 {
                        let other = pieces[(seeds[0] as usize + 1) % pieces.len()];
                        if other != a && other != b && other != c {
                            let ro = rook_neighbors(n, other);
                            let dst = ro[seeds[1 % seeds.len()] as usize % ro.len()];
                            moves.push(Move {
                                who: pick_seat(2),
                                frm: other,
                                to: dst,
                            });
                        }
                    }
                }
                // 4: Destination collision (A -> D and B -> D from two pieces)
                4 => {
                    if pieces.len() >= 2 {
                        let a = pieces[seeds[0] as usize % pieces.len()];
                        let b = pieces[(seeds[0] as usize + 1) % pieces.len()];
                        // Intersection (a.0, b.1) or (b.0, a.1) is rook-reachable from both a and b!
                        let d = if (a.0, b.1) != a && (a.0, b.1) != b {
                            (a.0, b.1)
                        } else if (b.0, a.1) != a && (b.0, a.1) != b {
                            (b.0, a.1)
                        } else {
                            let r = rook_neighbors(n, a);
                            r[0]
                        };
                        if (d.0 == a.0 || d.1 == a.1) && d != a {
                            moves.push(Move {
                                who: pick_seat(0),
                                frm: a,
                                to: d,
                            });
                        }
                        if (d.0 == b.0 || d.1 == b.1) && d != b {
                            moves.push(Move {
                                who: pick_seat(1),
                                frm: b,
                                to: d,
                            });
                        }
                    }
                }
                // 5: Adversarial moves targeting `auto`, from `marks`, and from `cooldowns`
                5 => {
                    // Move targeting `auto` from a rook-aligned piece or square
                    let aligned_to_auto = pieces
                        .iter()
                        .copied()
                        .find(|&p| p != auto && (p.0 == auto.0 || p.1 == auto.1))
                        .unwrap_or_else(|| {
                            let r = rook_neighbors(n, auto);
                            r[seeds[0] as usize % r.len()]
                        });
                    moves.push(Move {
                        who: pick_seat(0),
                        frm: aligned_to_auto,
                        to: auto,
                    });
                    // Move from marked square if any
                    if let Some(&mk) = marks.first() {
                        let r = rook_neighbors(n, mk);
                        moves.push(Move {
                            who: pick_seat(1),
                            frm: mk,
                            to: r[seeds[0] as usize % r.len()],
                        });
                    }
                    // Move from cooled-down square if any
                    if let Some(&(cd_sq, _)) = cds.first() {
                        let r = rook_neighbors(n, cd_sq);
                        moves.push(Move {
                            who: pick_seat(0),
                            frm: cd_sq,
                            to: r[seeds[0] as usize % r.len()],
                        });
                    }
                }
                // 6: Identical moves submitted by two different seats (identical-move exception)
                6 => {
                    if let Some(&p) = pieces.first() {
                        let r = rook_neighbors(n, p);
                        let dst = r[seeds[0] as usize % r.len()];
                        moves.push(Move {
                            who: pick_seat(0),
                            frm: p,
                            to: dst,
                        });
                        if seats.len() > 1 {
                            moves.push(Move {
                                who: pick_seat(1),
                                frm: p,
                                to: dst,
                            });
                        }
                    }
                }
                // 7: Random legal/proposable rook moves from pieces on the board
                _ => {
                    for (i, &seed) in seeds.iter().take(seats.len()).enumerate() {
                        let seat = pick_seat(i);
                        if !pieces.is_empty() {
                            let frm = pieces[(seed as usize + i) % pieces.len()];
                            let mask = pb_copy.legal_targets_mask(seat, frm, (seed & 1) != 0);
                            let targs = mask.to_coords(pb_copy.size());
                            if !targs.is_empty() {
                                let to = targs[(seed as usize / 2) % targs.len()];
                                moves.push(Move {
                                    who: seat,
                                    frm,
                                    to,
                                });
                            } else {
                                let r = rook_neighbors(n, frm);
                                moves.push(Move {
                                    who: seat,
                                    frm,
                                    to: r[seed as usize % r.len()],
                                });
                            }
                        }
                    }
                }
            }
            moves
        })
        .boxed()
}

fn arb_board_and_moves() -> impl Strategy<Value = (PackedBoard, Vec<Move>)> {
    arb_packed_board().prop_flat_map(|pb| {
        let m_strat = arb_moves_for_board(&pb);
        (Just(pb), m_strat)
    })
}

/// Strategy producing a board and simultaneous move list where every move has a distinct `who`.
fn arb_board_and_distinct_seat_moves() -> impl Strategy<Value = (PackedBoard, Vec<Move>)> {
    arb_board_and_moves().prop_map(|(pb, raw_moves)| {
        let mut deduped = Vec::new();
        for (idx, mut m) in raw_moves.into_iter().enumerate() {
            if deduped.iter().any(|existing: &Move| existing.who == m.who) {
                m.who = (idx as u32) + 10;
            }
            deduped.push(m);
        }
        // Assign waiting seats so each distinct mover seat can participate
        let all_seats: Vec<u32> = pb
            .waiting()
            .into_iter()
            .chain(deduped.iter().map(|m| m.who))
            .collect();
        let mut unique_seats = Vec::new();
        for s in all_seats {
            if !unique_seats.contains(&s) {
                unique_seats.push(s);
            }
        }
        (pb.with_waiting(&unique_seats), deduped)
    })
}

fn sorted_coords(mut cs: Vec<Coord>) -> Vec<Coord> {
    cs.sort_by_key(|&(x, y)| (y, x));
    cs.dedup();
    cs
}

fn sorted_cooldowns(mut cds: Vec<CooldownEntry>) -> Vec<CooldownEntry> {
    cds.sort_by_key(|&((x, y), t)| (y, x, t));
    cds
}

fn sorted_moves(mut ms: Vec<Move>) -> Vec<Move> {
    ms.sort_by_key(|m| (m.who, m.frm.1, m.frm.0, m.to.1, m.to.0));
    ms
}

fn sorted_seats(mut ps: Vec<u32>) -> Vec<u32> {
    ps.sort_unstable();
    ps
}

// ============================================================================
// §2. Cross-Oracle Property Tests (Lean Wire Oracle vs Unboxed `PackedBoard`)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn prop_packed_step_matches_wire_oracle((pb, moves) in arb_board_and_moves()) {
        let cfg = pb.config();
        let last_dir = pb.last_dir();
        let board = pb.to_board();
        let goals = pb.goals();
        let marks = pb.marks();
        let locked = pb.locked();
        let waiting = pb.waiting();
        let cooldowns = pb.cooldowns();

        let packed_out = pb.step(&moves).expect("PackedBoard::step must succeed");
        let wire_out = round_cfg(
            &cfg,
            last_dir,
            &board,
            &goals,
            &marks,
            &locked,
            &waiting,
            &cooldowns,
            &moves,
        )
        .expect("round_cfg wire oracle must succeed");

        match (packed_out, wire_out) {
            (
                PackedOutcome::Again(pb_again),
                VariantRoundOutcome::Again {
                    marks: w_marks,
                    locked: w_locked,
                    waiting: w_waiting,
                    cooldowns: w_cds,
                    last_dir: w_dir,
                },
            ) => {
                prop_assert_eq!(pb_again.auto_coord(), board.auto);
                prop_assert_eq!(pb_again.to_board(), board);
                prop_assert_eq!(pb_again.marks(), sorted_coords(w_marks));
                prop_assert_eq!(pb_again.locked(), w_locked);
                prop_assert_eq!(pb_again.waiting(), w_waiting);
                prop_assert_eq!(pb_again.cooldowns(), w_cds);
                prop_assert_eq!(pb_again.last_dir(), w_dir);
            }
            (
                PackedOutcome::Resolved {
                    board: pb_res,
                    win: p_win,
                },
                VariantRoundOutcome::Resolved {
                    board: w_board,
                    win: w_win,
                    cooldowns: w_cds,
                    last_dir: w_dir,
                },
            ) => {
                prop_assert_eq!(p_win, w_win);
                prop_assert_eq!(pb_res.to_board(), w_board.clone());
                prop_assert_eq!(pb_res.auto_coord(), w_board.auto);
                prop_assert!(pb_res.marks().is_empty());
                prop_assert!(pb_res.locked().is_empty());
                prop_assert_eq!(pb_res.cooldowns(), w_cds);
                prop_assert_eq!(pb_res.last_dir(), w_dir);
            }
            (p, w) => {
                prop_assert!(
                    false,
                    "PackedBoard::step vs round_cfg outcome mismatch:\npacked = {p:?}\nwire   = {w:?}"
                );
            }
        }
    }

    #[test]
    fn prop_packed_legal_targets_and_adjudicate_match_wire_oracle(pb in arb_packed_board()) {
        let b = pb.to_board();
        let n = pb.size();
        let marks = pb.marks();
        let locked = pb.locked();
        let cds = pb.cooldowns();
        let goals = pb.goals();

        // Adjudication equivalence
        let wire_adj = adjudicate_capped(&b, &goals).expect("adjudicate_capped");
        prop_assert_eq!(pb.adjudicate(), wire_adj);

        // Legal & executable targets equivalence for every square on the board
        for y in 0..(n as i32) {
            for x in 0..(n as i32) {
                let frm = (x, y);
                let on_cd = cds.iter().any(|&(c, t)| c == frm && t > 0);
                let cell = pb.cell_at(frm);
                let has_car = cell == REP || cell == ATT;

                let prop_mask = pb.legal_targets_mask(0, frm, false);
                let live_mask = pb.legal_targets_mask(0, frm, true);

                // Live targets must always be a subset of proposable targets
                prop_assert_eq!(live_mask.bitand(prop_mask), live_mask);

                if on_cd {
                    prop_assert_eq!(prop_mask, Bitmask128::ZERO);
                    prop_assert_eq!(live_mask, Bitmask128::ZERO);
                } else {
                    let wire_prop = legal_targets(&b, &marks, 0, frm).expect("legal_targets");
                    prop_assert_eq!(prop_mask.to_coords(n), wire_prop);

                    if has_car {
                        let wire_live =
                            executable_targets(&b, &marks, &locked, 0, frm).expect("executable_targets");
                        prop_assert_eq!(live_mask.to_coords(n), wire_live);
                    } else {
                        prop_assert_eq!(live_mask, Bitmask128::ZERO);
                    }
                }
            }
        }
    }

    #[test]
    fn prop_multi_turn_rollout_packed_matches_wire_oracle(
        pb0 in arb_packed_board(),
        turn_seeds in prop::collection::vec((0u8..8, any::<u16>()), 6..=10)
    ) {
        let mut pb = pb0;
        let cfg = pb.config();
        let goals = pb.goals();
        let mut wb = pb.to_board();
        let mut w_marks = pb.marks();
        let mut w_locked = pb.locked();
        let mut w_waiting = pb.waiting();
        let mut w_cds = pb.cooldowns();
        let mut w_last_dir = pb.last_dir();

        for (step_idx, &(pat, seed)) in turn_seeds.iter().enumerate() {
            let n = pb.size() as i32;
            let pieces = piece_coords(&pb);
            let mut moves = Vec::new();

            if pat == 0 && pieces.len() >= 2 && w_waiting.len() >= 2 {
                // Force a fork conflict on pieces[0] to exercise conflict re-entry sub-rounds
                let frm = pieces[seed as usize % pieces.len()];
                let r = rook_neighbors(n, frm);
                moves.push(Move {
                    who: w_waiting[0],
                    frm,
                    to: r[seed as usize % r.len()],
                });
                moves.push(Move {
                    who: w_waiting[1],
                    frm,
                    to: r[(seed as usize + 1) % r.len()],
                });
            } else {
                for (si, &seat) in w_waiting.iter().enumerate() {
                    if !pieces.is_empty() {
                        let frm = pieces[(seed as usize + si + step_idx) % pieces.len()];
                        let mask = pb.legal_targets_mask(seat, frm, true);
                        let targs = mask.to_coords(pb.size());
                        if !targs.is_empty() {
                            let to = targs[(seed as usize + si) % targs.len()];
                            if moves.iter().all(|m: &Move| m.frm != frm && m.to != to) {
                                moves.push(Move { who: seat, frm, to });
                            }
                        }
                    }
                }
            }

            let p_out = pb.step(&moves).expect("rollout PackedBoard::step");
            let w_out = round_cfg(
                &cfg,
                w_last_dir,
                &wb,
                &goals,
                &w_marks,
                &w_locked,
                &w_waiting,
                &w_cds,
                &moves,
            )
            .expect("rollout round_cfg");

            match (p_out, w_out) {
                (
                    PackedOutcome::Again(pb_next),
                    VariantRoundOutcome::Again {
                        marks: next_marks,
                        locked: next_locked,
                        waiting: next_waiting,
                        cooldowns: next_cds,
                        last_dir: next_dir,
                    },
                ) => {
                    w_marks = sorted_coords(next_marks);
                    w_locked = next_locked;
                    w_waiting = next_waiting;
                    w_cds = next_cds;
                    w_last_dir = next_dir;
                    prop_assert_eq!(pb_next.to_board(), wb.clone());
                    prop_assert_eq!(pb_next.marks(), w_marks.clone());
                    prop_assert_eq!(pb_next.locked(), w_locked.clone());
                    prop_assert_eq!(pb_next.waiting(), w_waiting.clone());
                    prop_assert_eq!(pb_next.cooldowns(), w_cds.clone());
                    prop_assert_eq!(pb_next.last_dir(), w_last_dir);
                    pb = pb_next;
                }
                (
                    PackedOutcome::Resolved {
                        board: pb_next,
                        win: p_win,
                    },
                    VariantRoundOutcome::Resolved {
                        board: next_wb,
                        win: w_win,
                        cooldowns: next_cds,
                        last_dir: next_dir,
                    },
                ) => {
                    prop_assert_eq!(p_win, w_win);
                    prop_assert_eq!(pb_next.to_board(), next_wb.clone());
                    prop_assert_eq!(pb_next.cooldowns(), next_cds.clone());
                    prop_assert_eq!(pb_next.last_dir(), next_dir);
                    wb = next_wb;
                    w_marks.clear();
                    w_locked.clear();
                    w_waiting = pb_next.waiting();
                    w_cds = next_cds;
                    w_last_dir = next_dir;
                    pb = pb_next;
                }
                (p, w) => {
                    prop_assert!(
                        false,
                        "rollout step {step_idx} outcome mismatch:\npacked = {p:?}\nwire   = {w:?}"
                    );
                }
            }
        }
    }
}

// ============================================================================
// §3. Universal Algebraic & Structural Game Invariants (`proptest`)
// ============================================================================

fn transpose_coord(c: Coord) -> Coord {
    (c.1, c.0)
}

fn transpose_move(m: Move) -> Move {
    Move {
        who: m.who,
        frm: transpose_coord(m.frm),
        to: transpose_coord(m.to),
    }
}

fn transpose_opt_dir(d: Option<Dir>) -> Option<Dir> {
    match d {
        None => None,
        Some(Dir::Xp) => Some(Dir::Yp),
        Some(Dir::Yp) => Some(Dir::Xp),
        Some(Dir::Xn) => Some(Dir::Yn),
        Some(Dir::Yn) => Some(Dir::Xn),
    }
}

fn transpose_tie_break(tb: TieBreak, last_dir: Option<Dir>) -> TieBreak {
    match tb {
        TieBreak::Column => TieBreak::Row,
        TieBreak::Row => TieBreak::Column,
        TieBreak::Freeze => TieBreak::Freeze,
        TieBreak::Momentum { prefer_same_axis } => {
            if last_dir.is_some() {
                TieBreak::Momentum { prefer_same_axis }
            } else {
                // When last_dir == None, Momentum(true) falls back to Column and Momentum(false) falls back to Row
                TieBreak::Momentum {
                    prefer_same_axis: !prefer_same_axis,
                }
            }
        }
    }
}

fn transpose_board(b: &Board) -> Board {
    let n = b.n;
    let mut cells = vec![VAC; n * n];
    for y in 0..n {
        for x in 0..n {
            cells[x * n + y] = b.cell_at((x as i32, y as i32));
        }
    }
    Board {
        n,
        cells,
        auto: transpose_coord(b.auto),
        col_rule: !b.col_rule,
    }
}

fn transpose_packed_board(pb: &PackedBoard) -> PackedBoard {
    let b_t = transpose_board(&pb.to_board());
    let mut cfg_t = pb.config();
    cfg_t.tie_break = transpose_tie_break(cfg_t.tie_break, pb.last_dir());
    if matches!(cfg_t.tie_break, TieBreak::Momentum { .. }) && pb.last_dir().is_none() {
        // Keep automata_count = 1 when testing initial None fallback on Momentum
        cfg_t.automata_count = 1;
    }
    let goals_t: Vec<(Coord, u32)> = pb
        .goals()
        .into_iter()
        .map(|(c, s)| (transpose_coord(c), s))
        .collect();
    let marks_t: Vec<Coord> = pb.marks().into_iter().map(transpose_coord).collect();
    let locked_t: Vec<Move> = pb.locked().into_iter().map(transpose_move).collect();
    let cds_t: Vec<CooldownEntry> = pb
        .cooldowns()
        .into_iter()
        .map(|(c, t)| (transpose_coord(c), t))
        .collect();
    let dir_t = transpose_opt_dir(pb.last_dir());
    PackedBoard::pack_with_goals(
        &b_t,
        &cfg_t,
        &goals_t,
        &pb.waiting(),
        &marks_t,
        &locked_t,
        &cds_t,
        dir_t,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// 3.1 Piece Conservation & Bitmask Disjointness / Bounds Invariant
    #[test]
    fn prop_piece_conservation_and_bounds((pb, moves) in arb_board_and_moves()) {
        let before_rep = pb.rep_mask().count_ones();
        let before_att = pb.att_mask().count_ones();
        let sz = pb.size();
        let before_auto = pb.auto_coord();
        let before_auto_idx = (before_auto.1 as usize) * sz + (before_auto.0 as usize);

        prop_assert!(pb.rep_mask().bitand(pb.att_mask()).is_empty());
        prop_assert!(!pb.rep_mask().test_bit(before_auto_idx));
        prop_assert!(!pb.att_mask().test_bit(before_auto_idx));

        let out = pb.step(&moves).expect("step must succeed");
        let after_pb = match out {
            PackedOutcome::Again(next) => next,
            PackedOutcome::Resolved { board: next, .. } => next,
        };

        let after_auto = after_pb.auto_coord();
        let after_auto_idx = (after_auto.1 as usize) * sz + (after_auto.0 as usize);

        // Bounds & Disjointness
        prop_assert!(after_pb.in_bounds(after_auto));
        prop_assert_eq!(after_pb.cell_at(after_auto), AUTO);
        prop_assert!(after_pb.rep_mask().bitand(after_pb.att_mask()).is_empty());
        prop_assert!(!after_pb.rep_mask().test_bit(after_auto_idx));
        prop_assert!(!after_pb.att_mask().test_bit(after_auto_idx));

        // No out-of-bounds bits >= sz * sz
        for bit in (sz * sz)..128 {
            prop_assert!(!after_pb.rep_mask().test_bit(bit));
            prop_assert!(!after_pb.att_mask().test_bit(bit));
            prop_assert!(!after_pb.mark_mask().test_bit(bit));
        }

        // Mode-specific piece count conservation
        match pb.config().confluence {
            ConfluenceMode::Conflict | ConfluenceMode::BunchBefore => {
                prop_assert_eq!(
                    after_pb.rep_mask().count_ones(),
                    before_rep,
                    "Repulsor count must be strictly conserved in {:?}",
                    pb.config().confluence
                );
                prop_assert_eq!(
                    after_pb.att_mask().count_ones(),
                    before_att,
                    "Attractor count must be strictly conserved in {:?}",
                    pb.config().confluence
                );
            }
            ConfluenceMode::Annihilate => {
                prop_assert!(after_pb.rep_mask().count_ones() <= before_rep);
                prop_assert!(after_pb.att_mask().count_ones() <= before_att);
            }
        }
    }

    /// 3.2 Simultaneous Move-List Permutation Invariance
    #[test]
    fn prop_simultaneous_move_permutation_invariance(
        (pb, moves) in arb_board_and_distinct_seat_moves(),
        perm_seed in any::<u64>()
    ) {
        let mut shuffled = moves.clone();
        // Deterministic Fisher-Yates shuffle from perm_seed
        let len = shuffled.len();
        let mut state = perm_seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        for i in (1..len).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (state as usize) % (i + 1);
            shuffled.swap(i, j);
        }

        let out_orig = pb.step(&moves).expect("step orig");
        let out_perm = pb.step(&shuffled).expect("step shuffled");

        match (out_orig, out_perm) {
            (PackedOutcome::Again(pb1), PackedOutcome::Again(pb2)) => {
                prop_assert_eq!(pb1.rep_mask(), pb2.rep_mask());
                prop_assert_eq!(pb1.att_mask(), pb2.att_mask());
                prop_assert_eq!(pb1.mark_mask(), pb2.mark_mask());
                prop_assert_eq!(pb1.auto_coord(), pb2.auto_coord());
                prop_assert_eq!(pb1.marks(), pb2.marks());
                prop_assert_eq!(pb1.cooldowns(), pb2.cooldowns());
                prop_assert_eq!(pb1.last_dir(), pb2.last_dir());
                prop_assert_eq!(sorted_moves(pb1.locked()), sorted_moves(pb2.locked()));
                prop_assert_eq!(sorted_seats(pb1.waiting()), sorted_seats(pb2.waiting()));
            }
            (
                PackedOutcome::Resolved { board: pb1, win: w1 },
                PackedOutcome::Resolved { board: pb2, win: w2 },
            ) => {
                prop_assert_eq!(w1, w2);
                prop_assert_eq!(pb1.rep_mask(), pb2.rep_mask());
                prop_assert_eq!(pb1.att_mask(), pb2.att_mask());
                prop_assert_eq!(pb1.mark_mask(), pb2.mark_mask());
                prop_assert_eq!(pb1.auto_coord(), pb2.auto_coord());
                prop_assert_eq!(pb1.to_board(), pb2.to_board());
                prop_assert_eq!(pb1.last_dir(), pb2.last_dir());
                prop_assert_eq!(pb1.waiting(), pb2.waiting());
                prop_assert_eq!(sorted_cooldowns(pb1.cooldowns()), sorted_cooldowns(pb2.cooldowns()));
            }
            (o1, o2) => {
                prop_assert!(
                    false,
                    "Permutation changed outcome variant:\norig = {o1:?}\nperm = {o2:?}"
                );
            }
        }
    }

    /// 3.3 D4 Diagonal Transpose / Reflection Equivariance `(x, y) <-> (y, x)`
    #[test]
    fn prop_d4_diagonal_transpose_equivariance((raw_pb, moves) in arb_board_and_moves()) {
        // When TieBreak::Momentum starts with last_dir == None and automata_count > 1,
        // normalize automata_count to 1 so the None fallback is purely swapped on the single step.
        let pb = if matches!(raw_pb.config().tie_break, TieBreak::Momentum { .. })
            && raw_pb.last_dir().is_none()
            && raw_pb.config().automata_count > 1
        {
            let mut cfg = raw_pb.config();
            cfg.automata_count = 1;
            PackedBoard::pack_with_goals(
                &raw_pb.to_board(),
                &cfg,
                &raw_pb.goals(),
                &raw_pb.waiting(),
                &raw_pb.marks(),
                &raw_pb.locked(),
                &raw_pb.cooldowns(),
                raw_pb.last_dir(),
            )
        } else {
            raw_pb
        };

        let pb_t = transpose_packed_board(&pb);
        let moves_t: Vec<Move> = moves.iter().copied().map(transpose_move).collect();

        let out = pb.step(&moves).expect("step original");
        let out_t = pb_t.step(&moves_t).expect("step transposed");

        match (out, out_t) {
            (PackedOutcome::Again(next_pb), PackedOutcome::Again(next_pb_t)) => {
                prop_assert_eq!(next_pb_t.to_board(), transpose_board(&next_pb.to_board()));
                prop_assert_eq!(next_pb_t.auto_coord(), transpose_coord(next_pb.auto_coord()));
                let marks_transposed = sorted_coords(
                    next_pb.marks().into_iter().map(transpose_coord).collect(),
                );
                prop_assert_eq!(next_pb_t.marks(), marks_transposed);
                let locked_transposed: Vec<Move> =
                    next_pb.locked().into_iter().map(transpose_move).collect();
                prop_assert_eq!(next_pb_t.locked(), locked_transposed);
                prop_assert_eq!(next_pb_t.waiting(), next_pb.waiting());
            }
            (
                PackedOutcome::Resolved { board: next_pb, win },
                PackedOutcome::Resolved { board: next_pb_t, win: win_t },
            ) => {
                prop_assert_eq!(win_t, win);
                prop_assert_eq!(next_pb_t.to_board(), transpose_board(&next_pb.to_board()));
                prop_assert_eq!(next_pb_t.auto_coord(), transpose_coord(next_pb.auto_coord()));
                prop_assert_eq!(next_pb_t.last_dir(), transpose_opt_dir(next_pb.last_dir()));
                let cds_transposed = sorted_cooldowns(
                    next_pb
                        .cooldowns()
                        .into_iter()
                        .map(|(c, t)| (transpose_coord(c), t))
                        .collect(),
                );
                prop_assert_eq!(sorted_cooldowns(next_pb_t.cooldowns()), cds_transposed);
            }
            (o, ot) => {
                prop_assert!(
                    false,
                    "Transpose changed outcome variant:\norig = {o:?}\ntransposed = {ot:?}"
                );
            }
        }
    }

    /// 3.4 Conflict Re-Entry Monotonicity, Non-Clashing Lock Preservation & Termination
    #[test]
    fn prop_conflict_reentry_monotonicity_and_termination((raw_pb, moves) in arb_board_and_moves()) {
        let mut cfg = raw_pb.config();
        cfg.confluence = ConfluenceMode::Conflict;
        let pb = PackedBoard::pack_with_goals(
            &raw_pb.to_board(),
            &cfg,
            &raw_pb.goals(),
            &raw_pb.waiting(),
            &raw_pb.marks(),
            &[],
            &raw_pb.cooldowns(),
            raw_pb.last_dir(),
        );

        let before_marks = pb.marks();
        let before_waiting = pb.waiting();
        let before_cds = pb.cooldowns();
        let b = pb.to_board();

        // Determine which fresh submissions were legal before stepping
        let fresh_legal: Vec<Move> = moves
            .iter()
            .copied()
            .filter(|m| {
                before_waiting.contains(&m.who)
                    && move_legal(&b, &before_marks, m).unwrap_or(false)
                    && !before_cds.iter().any(|&(c, t)| c == m.frm && t > 0)
            })
            .collect();

        match pb.step(&moves).expect("step conflict mode") {
            PackedOutcome::Again(next_pb) => {
                let after_marks = next_pb.marks();
                // Marks strictly grow on every conflict round
                prop_assert!(
                    after_marks.len() > before_marks.len(),
                    "marks must strictly grow on conflict: before={}, after={}",
                    before_marks.len(),
                    after_marks.len()
                );
                // Every prior mark is preserved
                for m_coord in &before_marks {
                    prop_assert!(after_marks.contains(m_coord));
                }
                // Newly added marks are the clash coordinates
                let new_marks: Vec<Coord> = after_marks
                    .iter()
                    .copied()
                    .filter(|c| !before_marks.contains(c))
                    .collect();
                prop_assert!(!new_marks.is_empty());

                // Non-clashing legal moves are preserved in next_pb.locked()
                let expected_locked: Vec<Move> = fresh_legal
                    .into_iter()
                    .filter(|m| !new_marks.contains(&m.frm) && !new_marks.contains(&m.to))
                    .collect();
                prop_assert_eq!(next_pb.locked(), expected_locked);

                // Repeated conflict re-entry MUST terminate in <= n^2 - before_marks.len() steps
                let max_remaining = pb.size() * pb.size() - before_marks.len();
                let mut cur_pb = next_pb;
                let mut steps = 1usize;
                let mut resolved = false;
                while steps <= max_remaining + 1 {
                    let wait = cur_pb.waiting();
                    // Try to provoke another fork conflict on an unmarked square if possible
                    let cur_marks = cur_pb.marks();
                    let n = cur_pb.size() as i32;
                    let mut fork_try = Vec::new();
                    if wait.len() >= 2 {
                        'find_unmarked: for y in 0..n {
                            for x in 0..n {
                                let c = (x, y);
                                if c != cur_pb.auto_coord() && !cur_marks.contains(&c) {
                                    let r: Vec<Coord> = rook_neighbors(n, c)
                                        .into_iter()
                                        .filter(|dst| !cur_marks.contains(dst))
                                        .collect();
                                    if r.len() >= 2 {
                                        fork_try.push(Move {
                                            who: wait[0],
                                            frm: c,
                                            to: r[0],
                                        });
                                        fork_try.push(Move {
                                            who: wait[1],
                                            frm: c,
                                            to: r[1],
                                        });
                                        break 'find_unmarked;
                                    }
                                }
                            }
                        }
                    }
                    match cur_pb.step(&fork_try).expect("re-entry step") {
                        PackedOutcome::Again(pb2) => {
                            prop_assert!(pb2.marks().len() > cur_pb.marks().len());
                            cur_pb = pb2;
                            steps += 1;
                        }
                        PackedOutcome::Resolved { .. } => {
                            resolved = true;
                            break;
                        }
                    }
                }
                prop_assert!(
                    resolved,
                    "Conflict re-entry failed to terminate within {} steps",
                    max_remaining + 1
                );
            }
            PackedOutcome::Resolved { board: next_pb, .. } => {
                prop_assert!(next_pb.marks().is_empty());
                prop_assert!(next_pb.locked().is_empty());
            }
        }
    }

    /// 3.5 Binary Codec Bijection (`pb.to_bytes()` -> `PackedBoard::from_bytes(&bytes)`)
    #[test]
    fn prop_binary_codec_bijection(pb in arb_packed_board()) {
        let bytes = pb.to_bytes();
        prop_assert_eq!(bytes.as_slice(), pb.as_bytes());
        let decoded = PackedBoard::from_bytes(&bytes).expect("valid PackedBoard must decode");
        prop_assert_eq!(decoded, pb);
        prop_assert_eq!(decoded.to_bytes(), bytes);
    }
}

// ============================================================================
// §4. Fail-Closed Binary & Wire Fuzzing (`proptest`)
// ============================================================================

fn assert_sanitized_packed_board_invariants(pb: &PackedBoard) {
    let sz = pb.size();
    assert!(
        (1..=11).contains(&sz),
        "accepted PackedBoard size {sz} outside 1..=11"
    );
    let auto = pb.auto_coord();
    assert!(
        pb.in_bounds(auto),
        "accepted PackedBoard auto {auto:?} out of bounds for size {sz}"
    );
    assert_eq!(pb.cell_at(auto), AUTO);

    let auto_idx = (auto.1 as usize) * sz + (auto.0 as usize);
    assert!(
        !pb.rep_mask().test_bit(auto_idx),
        "rep_mask must have auto bit cleared"
    );
    assert!(
        !pb.att_mask().test_bit(auto_idx),
        "att_mask must have auto bit cleared"
    );
    assert!(
        pb.rep_mask().bitand(pb.att_mask()).is_empty(),
        "rep_mask and att_mask must be disjoint"
    );

    for bit in (sz * sz)..128 {
        assert!(!pb.rep_mask().test_bit(bit));
        assert!(!pb.att_mask().test_bit(bit));
        assert!(!pb.mark_mask().test_bit(bit));
    }

    for c in pb.marks() {
        assert!(pb.in_bounds(c), "mark {c:?} out of bounds");
    }
    for m in pb.locked() {
        assert!(pb.in_bounds(m.frm), "locked frm {:?} out of bounds", m.frm);
        assert!(pb.in_bounds(m.to), "locked to {:?} out of bounds", m.to);
    }
    for (c, rem) in pb.cooldowns() {
        assert!(pb.in_bounds(c), "cooldown {c:?} out of bounds");
        assert!(rem > 0, "cooldown remaining turns must be > 0");
    }
    for (c, _) in pb.goals() {
        assert!(pb.in_bounds(c), "goal {c:?} out of bounds");
    }
    assert!(pb.to_board().validate().is_ok());
    assert_eq!(PackedBoard::from_bytes(&pb.to_bytes()), Some(*pb));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// 4.1 Arbitrary random byte buffers (`0..512`) into `PackedBoard::from_bytes` and raw FFI
    #[test]
    fn prop_fuzz_random_bytes_and_raw_ffi(
        raw_state in prop::collection::vec(any::<u8>(), 0..512),
        raw_moves in prop::collection::vec(any::<u8>(), 0..128),
        who in any::<u8>(),
        fx in any::<u8>(),
        fy in any::<u8>(),
        live_only in any::<bool>(),
    ) {
        if let Some(pb) = PackedBoard::from_bytes(&raw_state) {
            assert_sanitized_packed_board_invariants(&pb);
        }

        let step_res = packed_step_raw(&raw_state, &raw_moves);
        if let Ok(outcome) = step_res {
            match outcome {
                PackedOutcome::Again(pb) => assert_sanitized_packed_board_invariants(&pb),
                PackedOutcome::Resolved { board: pb, .. } => {
                    assert_sanitized_packed_board_invariants(&pb);
                }
            }
        }

        let mask = packed_legal_targets_raw(&raw_state, who, fx, fy, live_only);
        if PackedBoard::from_bytes(&raw_state).is_none() {
            prop_assert_eq!(mask, Bitmask128::ZERO);
        }

        let adj = packed_adjudicate_raw(&raw_state);
        if PackedBoard::from_bytes(&raw_state).is_none() {
            prop_assert!(adj.is_err());
        }
    }

    /// 4.2 Bit-flipped and mutated valid `PackedBoard` byte buffers
    #[test]
    fn prop_fuzz_bit_flipped_packed_boards(
        pb in arb_packed_board(),
        flips in prop::collection::vec((0usize..512, 0u8..8), 1..=8),
        truncate_or_pad in -16isize..=16,
        raw_moves in prop::collection::vec(any::<u8>(), 0..50),
    ) {
        let mut bytes = pb.to_bytes();
        if truncate_or_pad < 0 {
            let new_len = bytes.len().saturating_sub((-truncate_or_pad) as usize);
            bytes.truncate(new_len);
        } else if truncate_or_pad > 0 {
            bytes.extend(std::iter::repeat_n(0xA5u8, truncate_or_pad as usize));
        }
        if !bytes.is_empty() {
            for &(idx, bit) in &flips {
                let pos = idx % bytes.len();
                bytes[pos] ^= 1u8 << bit;
            }
        }

        if let Some(decoded) = PackedBoard::from_bytes(&bytes) {
            assert_sanitized_packed_board_invariants(&decoded);
            // Stepping an accepted board with empty moves must also succeed and preserve invariants
            let stepped = decoded.step(&[]).expect("accepted PackedBoard must step cleanly");
            match stepped {
                PackedOutcome::Again(next) => assert_sanitized_packed_board_invariants(&next),
                PackedOutcome::Resolved { board: next, .. } => {
                    assert_sanitized_packed_board_invariants(&next);
                }
            }
        }

        let _ = packed_step_raw(&bytes, &raw_moves);
        let _ = packed_legal_targets_raw(&bytes, 0, 0, 0, true);
        let _ = packed_adjudicate_raw(&bytes);
    }

    /// 4.3 Arbitrary and corrupted wire strings into `rules_wire`
    #[test]
    fn prop_fuzz_rules_wire_arbitrary_and_corrupted_strings(
        arb_str in "\\PC{0,256}",
        verb_idx in 0usize..18,
        corrupt_tail in "[0-9a-zA-Z _\\-\\x00]{0,128}",
    ) {
        let out1 = rules_wire(&arb_str);
        prop_assert!(
            out1 == "0" || out1.starts_with("1 "),
            "unexpected rules_wire reply {out1:?}"
        );

        let verbs = [
            "stock",
            "preset",
            "goals",
            "sense",
            "step",
            "stepcfg",
            "mid",
            "midcfg",
            "turn",
            "adjudicate",
            "dist",
            "legal",
            "targets",
            "livetargets",
            "clash",
            "round",
            "roundcfg",
            "unknown",
        ];
        let cmd = format!("{} {}", verbs[verb_idx % verbs.len()], corrupt_tail);
        let out2 = rules_wire(&cmd);
        prop_assert!(
            out2 == "0" || out2.starts_with("1 "),
            "unexpected rules_wire reply {out2:?} for {cmd:?}"
        );
    }
}
