//! Standalone WebAssembly (`wasm32-unknown-unknown`) self-test entrypoint for `automatafl-ai`.
//!
//! Exercises `automatafl_client_choose_move` (all 4 bot engines: Greedy, Minimax, DecoupledUct,
//! RegretMatching), `automatafl_client_legal_targets`, and `automatafl_client_step` inside a
//! real WASM VM with zero OS RNG or host imports.

use automatafl_ai::*;

#[unsafe(no_mangle)]
pub extern "C" fn run_wasm_ai_self_test() -> u32 {
    let cfg = GameConfig::default();
    let rs = PackedRoundState::open(PackedBoard::stock_two_player(), 2);
    let state_bytes = rs.to_packed_bytes(&cfg);

    // 1. Verify O(1) bitboard legal targets on 11x11 Classic opening
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
    if ok_t != 1 {
        return 10;
    }
    let prop_bb = BitBoard128([prop_lo, prop_hi]);
    let exec_bb = BitBoard128([exec_lo, exec_hi]);
    if prop_bb.count_ones() != 20 || exec_bb.count_ones() != 11 {
        return 11;
    }

    // 2. Verify all 4 bot engines (Greedy, Minimax, DecoupledUct, RegretMatching)
    for bot_kind in 0..=3u8 {
        let mut mv = [0u8; 5];
        let ok = unsafe {
            automatafl_client_choose_move(
                state_bytes.as_ptr(),
                state_bytes.len(),
                bot_kind,
                0,
                2026_11_01,
                mv.as_mut_ptr(),
            )
        };
        if ok != 1 || mv[0] != 0 {
            return 20 + u32::from(bot_kind);
        }
        let pm = PackedMove::new(
            mv[0],
            PackedCoord::new(mv[1], mv[2]),
            PackedCoord::new(mv[3], mv[4]),
        );
        if !rs.board.move_legal(rs.marks, pm) {
            return 30 + u32::from(bot_kind);
        }
    }

    // 3. Play 6 local client steps on 7x7 Blitz via `automatafl_client_step`
    let cfg_blitz = GameConfig {
        preset: BoardPreset::Blitz7x7,
        ..GameConfig::default()
    };
    let mut cur_bytes =
        PackedRoundState::open(PackedBoard::from_config(&cfg_blitz), 2).to_packed_bytes(&cfg_blitz);
    let mut step_out = [0u8; 384];
    for step_idx in 0..6u64 {
        let mut moves_buf = [0u8; 10];
        for seat in 0..2u8 {
            let ok = unsafe {
                automatafl_client_choose_move(
                    cur_bytes.as_ptr(),
                    cur_bytes.len(),
                    seat, // Greedy vs Minimax
                    seat,
                    900 + step_idx * 31 + u64::from(seat),
                    moves_buf[usize::from(seat) * 5..].as_mut_ptr(),
                )
            };
            if ok != 1 {
                return 40;
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
        if n_out <= 62 || (step_out[0] != 1 && step_out[0] != 2) {
            return 41;
        }
        cur_bytes = step_out[2..n_out].to_vec();
    }

    // 4. Verify edge-case robustness inside wasm32:
    // (a) `sz == 0` buffer rejected cleanly (`0`)
    let mut bad_sz = state_bytes.clone();
    bad_sz[48] = 0;
    let mut mv = [0u8; 5];
    if unsafe {
        automatafl_client_choose_move(bad_sz.as_ptr(), bad_sz.len(), 0, 0, 1, mv.as_mut_ptr())
    } != 0
    {
        return 50;
    }

    // (b) Non-axial `(0,0)->(1,2)` and OOB `(20,20)->(20,21)` locked moves do not hang or trap
    let mut rs_bad_locked = PackedRoundState::open(PackedBoard::stock_two_player(), 2);
    rs_bad_locked.locked[0] = PackedMove::new(2, PackedCoord::new(0, 0), PackedCoord::new(1, 2));
    rs_bad_locked.locked[1] =
        PackedMove::new(3, PackedCoord::new(20, 20), PackedCoord::new(20, 21));
    rs_bad_locked.locked_len = 2;
    let bad_locked_bytes = rs_bad_locked.to_packed_bytes(&cfg);
    let step_moves = [0u8, 3, 1, 3, 3, 1, 3, 9, 3, 7];
    let n_bad = unsafe {
        automatafl_client_step(
            bad_locked_bytes.as_ptr(),
            bad_locked_bytes.len(),
            step_moves.as_ptr(),
            step_moves.len(),
            step_out.as_mut_ptr(),
            step_out.len(),
        )
    };
    if n_bad <= 62 {
        return 51;
    }

    1
}

fn main() {
    let code = run_wasm_ai_self_test();
    assert_eq!(code, 1, "wasm_ai_self_test failed with code {code}");
}
