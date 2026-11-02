//! Exploratory game-theoretic analysis (symmetry / zero first-player advantage, stage flatness)
//! and self-play match arena over [`PackedBoard`].

use crate::SimultaneousBot;
use crate::packed::{
    GameConfig, MoveList, PackedBoard, PackedCoord, PackedGoals, PackedMove, PackedRoundOutcome,
    PackedRoundState, validate_no_duplicate_seats,
};
use crate::regret::{RegretConfig, StageEquilibrium, solve_stage_equilibrium};

/// Report from an exploratory game-theoretic analysis of an opening position.
#[derive(Clone, Debug)]
pub struct OpeningAnalysisReport {
    /// Whether the board and goals are vertically reflection-symmetric (`y <-> size - 1 - y`).
    pub is_vertically_symmetric: bool,
    /// Whether `roundStep` commutes with vertical reflection on sampled move profiles
    /// (demonstrating zero first-player / seat-0 advantage).
    pub reflection_commutes: bool,
    /// Solved stage equilibrium of the opening position.
    pub stage_equilibrium: StageEquilibrium,
}

/// Reflect a coordinate vertically (`y -> size - 1 - y`).
#[inline]
pub const fn reflect_coord_y(c: PackedCoord, size: u8) -> PackedCoord {
    PackedCoord {
        x: c.x,
        y: (size - 1).saturating_sub(c.y),
    }
}

/// Reflect a move vertically and swap seat `0 <-> 1`.
#[inline]
pub const fn reflect_move_y(m: PackedMove, size: u8) -> PackedMove {
    PackedMove {
        who: m.who ^ 1,
        frm: reflect_coord_y(m.frm, size),
        to: reflect_coord_y(m.to, size),
    }
}

/// Reflect a [`PackedBoard`] vertically (`y -> size - 1 - y`).
pub fn reflect_board_y(b: &PackedBoard) -> PackedBoard {
    let s = b.size;
    let rev_auto = reflect_coord_y(b.automaton(), s);
    let mut out = PackedBoard::empty(s, rev_auto.x, rev_auto.y, b.col_rule);
    out.automata = crate::packed::BitBoard128::EMPTY;
    out.last_dx = b.last_dx;
    out.last_dy = -b.last_dy;
    for y in 0..s {
        for x in 0..s {
            let c = PackedCoord::new(x, y);
            let rc = reflect_coord_y(c, s);
            let code = b.cell_at(c);
            if code != crate::packed::VAC {
                out.place(rc, code);
            }
        }
    }
    for i in 0..b.cooldown_len as usize {
        let c = b.coord_of(b.cooldown_cells[i] as usize);
        let rc = reflect_coord_y(c, s);
        if out.in_bounds(rc) && (out.cooldown_len as usize) < crate::packed::MAX_COOLDOWNS {
            let idx = out.idx(rc);
            let k = out.cooldown_len as usize;
            out.cooldown_cells[k] = idx as u8;
            out.cooldown_rem[k] = b.cooldown_rem[i];
            out.cooldown_mask.set(idx);
            out.cooldown_len += 1;
        }
    }
    out
}

fn boards_semantically_eq(a: &PackedBoard, b: &PackedBoard) -> bool {
    if a.size != b.size
        || a.repulsors != b.repulsors
        || a.attractors != b.attractors
        || a.automata != b.automata
        || a.auto_x != b.auto_x
        || a.auto_y != b.auto_y
        || a.col_rule != b.col_rule
        || a.last_dx != b.last_dx
        || a.last_dy != b.last_dy
        || a.cooldown_mask != b.cooldown_mask
        || a.cooldown_len != b.cooldown_len
    {
        return false;
    }
    for i in 0..a.cooldown_len as usize {
        let cell = a.cooldown_cells[i];
        let rem = a.cooldown_rem[i];
        let mut found = false;
        for j in 0..b.cooldown_len as usize {
            if b.cooldown_cells[j] == cell && b.cooldown_rem[j] == rem {
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

/// Run exploratory game-theoretic symmetry and stage-equilibrium analysis on `board`.
///
/// Because Automatafl is simultaneous-move, neither seat moves "first"; furthermore, on the
/// stock two-player board (`stockTwoPlayer`) with `stockGoals2`, the involution
/// `(x, y, seat) <-> (x, size - 1 - y, 1 - seat)` is an exact isomorphism of `roundStep`,
/// `automatonStepCfg`, `winOnEntry`, and `adjudicateCapped`, forcing the opening game value
/// to `0.0`.
pub fn analyze_opening_symmetry(
    board: &PackedBoard,
    goals: &PackedGoals,
    cfg: &GameConfig,
) -> OpeningAnalysisReport {
    let reflected = reflect_board_y(board);
    let is_vertically_symmetric = boards_semantically_eq(&reflected, board);

    let rs = PackedRoundState::open(*board, 2);
    let mut m0_list = MoveList::new();
    board.generate_tactical_moves(rs.marks, &[], 0, cfg, &mut m0_list);

    let mut reflection_commutes = true;
    let sample_len = m0_list.len().min(20);
    let slice = &m0_list.as_slice()[..sample_len];

    'check: for (i, &m0) in slice.iter().enumerate() {
        for &base_m1 in slice.iter().skip(i / 2).take(4) {
            let m1 = PackedMove::new(1, base_m1.frm, base_m1.to);
            // Mirror profile: Seat 0 plays reflected(m1), Seat 1 plays reflected(m0)
            let rm0 = reflect_move_y(m1, board.size);
            let rm1 = reflect_move_y(m0, board.size);

            let out_a = rs.round_step(cfg, goals, &[m0, m1]);
            let out_b = rs.round_step(cfg, goals, &[rm0, rm1]);

            match (out_a, out_b) {
                (
                    PackedRoundOutcome::Resolved {
                        board: ba,
                        win: wa,
                    },
                    PackedRoundOutcome::Resolved {
                        board: bb,
                        win: wb,
                    },
                ) => {
                    if !boards_semantically_eq(&reflect_board_y(&ba), &bb)
                        || wa.map(|w| w ^ 1) != wb
                    {
                        reflection_commutes = false;
                        break 'check;
                    }
                }
                (PackedRoundOutcome::Again(ra), PackedRoundOutcome::Again(rb)) => {
                    if ra.marks.count_ones() != rb.marks.count_ones()
                        || ra.locked_len != rb.locked_len
                    {
                        reflection_commutes = false;
                        break 'check;
                    }
                }
                _ => {
                    reflection_commutes = false;
                    break 'check;
                }
            }
        }
    }

    let eq = solve_stage_equilibrium(&rs, goals, cfg, &RegretConfig::default());
    OpeningAnalysisReport {
        is_vertically_symmetric,
        reflection_commutes,
        stage_equilibrium: eq,
    }
}

/// Summary of a complete simultaneous-move self-play game between two bots.
#[derive(Clone, Debug)]
pub struct MatchReport {
    /// Final board at game end.
    pub final_board: PackedBoard,
    /// Outright `winOnEntry` winner (`Some(0)` or `Some(1)`), if any.
    pub entry_winner: Option<u8>,
    /// Final winner including `adjudicateCapped` if the turn cap was reached.
    pub adjudicated_winner: Option<u8>,
    /// Number of resolved turns played.
    pub turns_played: u16,
    /// Total number of clash re-entry sub-rounds across the match.
    pub clash_subrounds: u32,
}

/// Play a full game between `bot0` (Seat 0) and `bot1` (Seat 1) over [`PackedBoard`],
/// enforcing strict fog-of-war (neither bot sees the other's uncommitted move) and
/// verifying no duplicate-seat moves or infinite clash loops occur.
pub fn play_simultaneous_match(
    bot0: &mut dyn SimultaneousBot,
    bot1: &mut dyn SimultaneousBot,
    cfg: &GameConfig,
) -> MatchReport {
    let mut board = PackedBoard::from_config(cfg);
    let goals = PackedGoals::stock_two_player(board.size);
    let max_turns = cfg.max_turns.max(1);
    let max_clash_per_turn = (board.size as u32) * (board.size as u32) + 1;

    let mut turns_played = 0u16;
    let mut clash_subrounds = 0u32;

    while turns_played < max_turns {
        let mut rs = PackedRoundState::open(board, 2);
        let mut turn_clashes = 0u32;

        loop {
            assert!(
                turn_clashes <= max_clash_per_turn,
                "clash re-entry exceeded proven n^2 termination bound ({max_clash_per_turn})"
            );

            // Each waiting bot receives ONLY an immutable reference to the public `PackedRoundState`
            let m0_opt = if rs.is_waiting(0) {
                bot0.choose_move(&rs, &goals, cfg, 0)
            } else {
                None
            };
            let m1_opt = if rs.is_waiting(1) {
                bot1.choose_move(&rs, &goals, cfg, 1)
            } else {
                None
            };

            let mut subs = [PackedMove::default(); 2];
            let mut subs_len = 0usize;
            if let Some(m0) = m0_opt {
                assert_eq!(m0.who, 0, "Seat 0 bot returned move with wrong seat ID");
                assert!(
                    rs.board.move_legal(rs.marks, m0),
                    "Seat 0 bot returned illegal move: {m0:?}"
                );
                subs[subs_len] = m0;
                subs_len += 1;
            }
            if let Some(m1) = m1_opt {
                assert_eq!(m1.who, 1, "Seat 1 bot returned move with wrong seat ID");
                assert!(
                    rs.board.move_legal(rs.marks, m1),
                    "Seat 1 bot returned illegal move: {m1:?}"
                );
                subs[subs_len] = m1;
                subs_len += 1;
            }

            validate_no_duplicate_seats(&subs[..subs_len])
                .expect("duplicate seat submission in match");

            match rs.round_step(cfg, &goals, &subs[..subs_len]) {
                PackedRoundOutcome::Resolved {
                    board: next_board,
                    win,
                } => {
                    board = next_board;
                    turns_played += 1;
                    if win.is_some() {
                        return MatchReport {
                            final_board: board,
                            entry_winner: win,
                            adjudicated_winner: win,
                            turns_played,
                            clash_subrounds,
                        };
                    }
                    break;
                }
                PackedRoundOutcome::Again(next_rs) => {
                    assert!(
                        next_rs.marks.count_ones() > rs.marks.count_ones(),
                        "clash round must strictly grow marks (AutomataflTermination.roundStep_marks_strict_growth)"
                    );
                    rs = next_rs;
                    turn_clashes += 1;
                    clash_subrounds += 1;
                }
            }
        }
    }

    let adjudicated_winner = board.adjudicate_capped(&goals);
    MatchReport {
        final_board: board,
        entry_winner: None,
        adjudicated_winner,
        turns_played,
        clash_subrounds,
    }
}
