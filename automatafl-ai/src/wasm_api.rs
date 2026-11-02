//! Compact Client-Side WASM / C-ABI Exports for `automatafl-ai`.
//!
//! Exposes `#[unsafe(no_mangle)] pub unsafe extern "C"` entrypoints over the flat binary
//! `PackedRoundState` (`PackedBoard.toBytes`) format so browser (`wasm32-unknown-unknown`)
//! and native clients can offload AI move search, O(1) bitboard legal/executable target
//! calculation, and local move/round stepping with zero server round-trips.

use crate::packed::{BitBoard128, PackedCoord, PackedMove, PackedRoundState};
use crate::{BotEngine, BotKind, SimultaneousBot};

/// Safe Rust wrapper around [`automatafl_client_choose_move`]: decode `packed_state`, run
/// [`BotEngine`] of `bot_kind` (`0 = Greedy`, `1 = Minimax`, `2 = DecoupledUct`,
/// `3 = RegretMatching`) for `seat` with `seed`, and return the chosen [`PackedMove`].
pub fn client_choose_move(
    packed_state: &[u8],
    bot_kind: u8,
    seat: u8,
    seed: u64,
) -> Option<PackedMove> {
    let (rs, cfg, goals) = PackedRoundState::from_packed_bytes(packed_state).ok()?;
    let mut engine = BotEngine::new(BotKind::from_u8(bot_kind), seed);
    engine.choose_move(&rs, &goals, &cfg, seat)
}

/// Safe Rust wrapper around [`automatafl_client_legal_targets`]: decode `packed_state` and
/// compute `(proposable, executable)` [`BitBoard128`] target masks from `(fx, fy)` via
/// Lean's `automatafl_packed_legal_targets` C FFI.
pub fn client_legal_targets(
    packed_state: &[u8],
    seat: u8,
    fx: u8,
    fy: u8,
) -> Option<(BitBoard128, BitBoard128)> {
    let (rs, _, _) = PackedRoundState::from_packed_bytes(packed_state).ok()?;
    Some(rs.legal_target_masks(seat, PackedCoord::new(fx, fy)))
}

/// Safe Rust wrapper around [`automatafl_client_step`]: validate `packed_state` and delegate
/// directly to `lean/Automatafl/PackedBoard.lean::packedStepFFI` (`automatafl_lean::packed_step_raw`).
pub fn client_step(packed_state: &[u8], moves_bytes: &[u8]) -> Result<Vec<u8>, String> {
    if moves_bytes.len() % 5 != 0 {
        return Err("moves_bytes length must be a multiple of 5".to_string());
    }
    let (mut rs, cfg, goals) = PackedRoundState::from_packed_bytes(packed_state)?;
    let mut clean_locked = [PackedMove::default(); crate::packed::MAX_PLAYERS];
    let mut clean_len = 0usize;
    for i in 0..rs.locked_len as usize {
        let lm = rs.locked[i];
        if rs.board.move_legal(BitBoard128::EMPTY, lm) {
            clean_locked[clean_len] = lm;
            clean_len += 1;
        }
    }
    rs.locked = clean_locked;
    rs.locked_len = clean_len as u8;
    let clean_state = rs.to_packed_bytes_with_goals(&cfg, &goals);

    let mut filtered_moves = Vec::with_capacity(moves_bytes.len().min(125));
    for chunk in moves_bytes.chunks_exact(5) {
        let m = PackedMove::new(
            chunk[0],
            PackedCoord::new(chunk[1], chunk[2]),
            PackedCoord::new(chunk[3], chunk[4]),
        );
        if rs.is_waiting(m.who)
            && rs.board.move_legal(rs.marks, m)
            && filtered_moves.len() + 5 <= 125
        {
            filtered_moves.extend_from_slice(chunk);
        }
    }

    let outcome = automatafl_lean::packed_step_raw(&clean_state, &filtered_moves)?;
    match outcome {
        automatafl_lean::PackedOutcome::Again(next_pb) => {
            let payload = next_pb.as_bytes();
            let mut out = Vec::with_capacity(2 + payload.len());
            out.push(1);
            out.push(255);
            out.extend_from_slice(payload);
            Ok(out)
        }
        automatafl_lean::PackedOutcome::Resolved { board: next_pb, win } => {
            let payload = next_pb.as_bytes();
            let mut out = Vec::with_capacity(2 + payload.len());
            out.push(2);
            out.push(win.map(|w| w as u8).unwrap_or(255));
            out.extend_from_slice(payload);
            Ok(out)
        }
    }
}

/// C-ABI / WASM export: given a packed state buffer (`to_packed_bytes`), `bot_kind: u8`
/// (`0 = Greedy`, `1 = Minimax`, `2 = DecoupledUct`, `3 = RegretMatching`), `seat: u8`,
/// and `seed: u64`, runs [`BotEngine`] on the client and writes the chosen 5-byte move
/// `[who, fx, fy, tx, ty]` to `out_move_ptr`.
///
/// Returns `1` if a legal move was found, or `0` if none (or if input is malformed).
///
/// # Safety
/// - `state_ptr` must point to at least `state_len` readable bytes (when `state_len > 0`).
/// - If non-null, `out_move_ptr` must point to at least 5 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn automatafl_client_choose_move(
    state_ptr: *const u8,
    state_len: usize,
    bot_kind: u8,
    seat: u8,
    seed: u64,
    out_move_ptr: *mut u8,
) -> u8 {
    if state_ptr.is_null() || state_len < 60 {
        return 0;
    }
    let state_slice = unsafe { core::slice::from_raw_parts(state_ptr, state_len) };
    let Some(mv) = client_choose_move(state_slice, bot_kind, seat, seed) else {
        return 0;
    };
    if !out_move_ptr.is_null() {
        unsafe {
            *out_move_ptr.add(0) = mv.who;
            *out_move_ptr.add(1) = mv.frm.x;
            *out_move_ptr.add(2) = mv.frm.y;
            *out_move_ptr.add(3) = mv.to.x;
            *out_move_ptr.add(4) = mv.to.y;
        }
    }
    1
}

/// C-ABI / WASM export: given a packed state buffer, `seat: u8`, `fx: u8`, `fy: u8`,
/// writes both the `proposable` (`BitBoard128`) and `executable` (`BitBoard128`) target
/// bitmasks (`lo: u64, hi: u64` each) in a single O(1) bitboard pass so client UI piece
/// selection requires zero server round-trips.
///
/// Returns `1` if `state_ptr` decoded validly, or `0` if malformed (zeroing any non-null
/// output pointers).
///
/// # Safety
/// - `state_ptr` must point to at least `state_len` readable bytes (when `state_len > 0`).
/// - Each non-null `out_*` pointer must be a valid, aligned pointer to a writable `u64`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn automatafl_client_legal_targets(
    state_ptr: *const u8,
    state_len: usize,
    seat: u8,
    fx: u8,
    fy: u8,
    out_proposable_lo: *mut u64,
    out_proposable_hi: *mut u64,
    out_executable_lo: *mut u64,
    out_executable_hi: *mut u64,
) -> u8 {
    let write_outputs = |prop: BitBoard128, exec: BitBoard128| unsafe {
        if !out_proposable_lo.is_null() {
            *out_proposable_lo = prop.0[0];
        }
        if !out_proposable_hi.is_null() {
            *out_proposable_hi = prop.0[1];
        }
        if !out_executable_lo.is_null() {
            *out_executable_lo = exec.0[0];
        }
        if !out_executable_hi.is_null() {
            *out_executable_hi = exec.0[1];
        }
    };

    if state_ptr.is_null() || state_len < 60 {
        write_outputs(BitBoard128::EMPTY, BitBoard128::EMPTY);
        return 0;
    }
    let state_slice = unsafe { core::slice::from_raw_parts(state_ptr, state_len) };
    let Some((prop, exec)) = client_legal_targets(state_slice, seat, fx, fy) else {
        write_outputs(BitBoard128::EMPTY, BitBoard128::EMPTY);
        return 0;
    };
    write_outputs(prop, exec);
    1
}

/// C-ABI / WASM export: given a packed state buffer and moves buffer (`5 * k` bytes
/// `[who, fx, fy, tx, ty]`), runs [`PackedRoundState::round_step`] on the client for
/// instant local Vs-Bot games and client-side move forecasts.
///
/// Writes `[status, winner_byte]` followed by `next_rs.to_packed_bytes_with_goals` into
/// `out_ptr` (up to `out_cap` bytes) and returns the total output byte length (`0` on
/// malformed state input):
/// - `status = 1, winner_byte = 255`: clash re-entry (`.again`)
/// - `status = 2, winner_byte = win.unwrap_or(255)`: clean resolution (`.resolved`)
///
/// # Safety
/// - `state_ptr` must point to at least `state_len` readable bytes (when `state_len > 0`).
/// - `moves_ptr` must point to at least `moves_len` readable bytes (when `moves_len > 0`).
/// - If non-null and `out_cap > 0`, `out_ptr` must point to at least `out_cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn automatafl_client_step(
    state_ptr: *const u8,
    state_len: usize,
    moves_ptr: *const u8,
    moves_len: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> usize {
    if state_ptr.is_null() || state_len < 60 {
        if !out_ptr.is_null() && out_cap > 0 {
            unsafe {
                *out_ptr = 0;
            }
        }
        return 0;
    }
    let state_slice = unsafe { core::slice::from_raw_parts(state_ptr, state_len) };
    let moves_slice: &[u8] = if moves_ptr.is_null() || moves_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(moves_ptr, moves_len) }
    };

    let Ok(reply) = client_step(state_slice, moves_slice) else {
        if !out_ptr.is_null() && out_cap > 0 {
            unsafe {
                *out_ptr = 0;
            }
        }
        return 0;
    };

    let total_len = reply.len();
    let copy_len = total_len.min(out_cap);
    if !out_ptr.is_null() && copy_len > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(reply.as_ptr(), out_ptr, copy_len);
        }
    }
    total_len
}
