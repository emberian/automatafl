//! Standalone WASM self-test entrypoint for executing `automatafl-lean` inside a real
//! WebAssembly runtime (`wasm32-unknown-unknown` under Node.js / V8).

use automatafl_lean::*;

#[unsafe(no_mangle)]
pub extern "C" fn run_wasm_self_test() -> u32 {
    // 1. Stock presets & PackedBoard roundtrip
    let pb11 = PackedBoard::stock(BoardPreset::Classic11);
    let wb11 = match stock_two_player() {
        Ok(b) => b,
        Err(_) => return 10,
    };
    if pb11.to_board() != wb11 {
        return 11;
    }

    // 2. Legal & live targets on 11x11 opening
    let prop_mask = pb11.legal_targets_mask(0, (3, 1), false);
    let live_mask = pb11.legal_targets_mask(0, (3, 1), true);
    if prop_mask.count_ones() != 20 || live_mask.count_ones() != 11 {
        return 12;
    }

    // 3. PackedBoard step vs wire round
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
    let goals11 = match stock_goals2(11) {
        Ok(g) => g,
        Err(_) => return 13,
    };
    let wire_out = match round(&wb11, &goals11, &[], &[], &[0, 1], &moves) {
        Ok(o) => o,
        Err(_) => return 14,
    };
    let packed_out = match pb11.step(&moves) {
        Ok(o) => o,
        Err(_) => return 15,
    };
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
            if pb_after.to_board() != wb_after || p_win != w_win {
                return 16;
            }
        }
        _ => return 17,
    }

    // 4. BunchBefore 3-mover caterpillar + merge collision conservation & permutation invariance
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
    for p in [[m1, m2, m3], [m3, m2, m1]] {
        let after = match resolve_mid_cfg(&cfg_bunch, &b3, &[], &p) {
            Ok(b) => b,
            Err(_) => return 18,
        };
        if after.cell_at((1, 0)) != VAC
            || after.cell_at((0, 0)) != REP
            || after.cell_at((0, 1)) != REP
            || after.cell_at((2, 0)) != ATT
        {
            return 19;
        }
    }

    // 5. Cooldowns on legal_targets_mask for both live_only=true and live_only=false
    let mut b5 = Board::empty_with_size(5, true);
    b5.set_cell((2, 0), REP);
    let cfg_cd = GameConfig {
        cooldown_turns: 2,
        ..GameConfig::default()
    };
    let pb_cd = PackedBoard::pack(&b5, &cfg_cd, &[0, 1], &[], &[], &[((2, 0), 2)], None);
    if pb_cd.legal_targets_mask(0, (2, 0), true) != Bitmask128::ZERO
        || pb_cd.legal_targets_mask(0, (2, 0), false) != Bitmask128::ZERO
    {
        return 20;
    }

    // 6. 4-player PackedBoard goals & waiting seats preservation across step
    let pb4 = pb11.with_four_player_goals();
    if pb4.waiting() != vec![0, 1, 2, 3] || pb4.goals() != PackedBoard::stock_goals4_for_size(11) {
        return 22;
    }
    match pb4.step(&moves) {
        Ok(PackedOutcome::Resolved { board: pb4_after, .. }) => {
            if pb4_after.waiting() != vec![0, 1, 2, 3]
                || pb4_after.goals() != PackedBoard::stock_goals4_for_size(11)
            {
                return 23;
            }
        }
        _ => return 24,
    }

    // 7. Verify 0 leaked transient allocations in WASM linear memory
    if live_lean_allocs() != 0 {
        return 21;
    }

    1
}

fn main() {
    let code = run_wasm_self_test();
    assert_eq!(code, 1, "wasm_self_test failed with code {code}");
}
