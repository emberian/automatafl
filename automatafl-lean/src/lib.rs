//! `automatafl-lean` — Zero-twin Rust FFI bindings to the formally verified Lean 4 Automatafl core.
//!
//! This crate compiles the C code emitted directly by Lean 4 (`lean/.lake/build/ir/Automatafl/*.c`)
//! for both native targets and `wasm32-unknown-unknown`, exposing a two-tier ABI:
//! 1. **Wire Oracle (`rules` module)**: `@[export] dregg_automatafl_rules` string wire protocol
//!    supporting all canonical verbs (`stock`, `goals`, `sense`, `step`, `mid`, `turn`,
//!    `adjudicate`, `dist`, `legal`, `targets`, `livetargets`, `clash`, `round`) and R2 variant
//!    verbs (`preset`, `stepcfg`, `midcfg`, `roundcfg`).
//! 2. **Unboxed PackedBoard (`packed` module)**: `@[export] automatafl_packed_*` binary bitmask
//!    protocol (`Bitmask128`, `PackedBoard`, `PackedOutcome`) proven equivalent to `Board.cellAt`
//!    and `roundStep` / `roundStepVariant` via `boardOfMasks_cellAt_eq`, `Board.reifyMasks_cellAt_eq`,
//!    `PackedBoard.unpack_pack`, and `PackedBoard.unpack_pack_cellAt_eq`.
//!
//! No game rule is implemented or duplicated in Rust.

use std::alloc::{Layout, alloc, dealloc};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod board;
pub mod config;
pub mod packed;
pub mod rules;

pub use board::{
    ATT, AUTO, AutomatonSense, Board, Coord, DIRS, Decision, Move, N7, N9, N11, REP, Raycast, VAC,
    XN, XP, YN, YP,
};
pub use config::{
    BoardPreset, ConfluenceMode, CooldownEntry, Dir, GameConfig, TieBreak,
};
pub use packed::{
    Bitmask128, MAX_PACKED_BYTES, PackedBoard, PackedOutcome, packed_adjudicate_raw,
    packed_legal_targets_raw, packed_step_raw,
};
pub use rules::{
    RoundOutcome, Standing, VariantRoundOutcome, adjudicate_capped, apply_turn, automaton_step,
    automaton_step_cfg, available, clash, executable_targets, legal_targets, move_legal,
    preset_board, resolve_mid, resolve_mid_cfg, round, round_cfg, round_is_clean, rules_wire,
    sense, standing, stock_goals2, stock_two_player, turn,
};

// ============================================================================
// Rust-Backed Memory Allocator for the Lean C Runtime (`lean_shim.c`)
// ============================================================================

const HEADER_ALIGN: usize = 16;
const HEADER_SIZE: usize = 16;

static LIVE_LEAN_ALLOCS: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_FFI_CALLS: AtomicUsize = AtomicUsize::new(0);

struct ActiveFfiGuard;

impl ActiveFfiGuard {
    #[inline]
    fn enter() -> Self {
        ACTIVE_FFI_CALLS.fetch_add(1, Ordering::Acquire);
        Self
    }
}

impl Drop for ActiveFfiGuard {
    #[inline]
    fn drop(&mut self) {
        ACTIVE_FFI_CALLS.fetch_sub(1, Ordering::Release);
    }
}

/// Number of currently live non-persistent heap allocations in the Lean 4 runtime shim.
/// Waits only if another thread is actively mid-FFI call so parallel test threads do not
/// observe transient in-flight allocations from a sibling thread.
pub fn live_lean_allocs() -> usize {
    loop {
        let active_before = ACTIVE_FFI_CALLS.load(Ordering::SeqCst);
        let live = LIVE_LEAN_ALLOCS.load(Ordering::SeqCst);
        let active_after = ACTIVE_FFI_CALLS.load(Ordering::SeqCst);
        if live == 0 || (active_before == 0 && active_after == 0) {
            return live;
        }
        std::thread::yield_now();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn automatafl_rt_alloc(sz: usize) -> *mut u8 {
    let payload = sz.max(1);
    let total = HEADER_SIZE + payload;
    let Ok(layout) = Layout::from_size_align(total, HEADER_ALIGN) else {
        std::process::abort();
    };
    let raw = unsafe { alloc(layout) };
    if raw.is_null() {
        std::process::abort();
    }
    LIVE_LEAN_ALLOCS.fetch_add(1, Ordering::Relaxed);
    unsafe {
        *(raw as *mut usize) = total;
        raw.add(HEADER_SIZE)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn automatafl_rt_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    LIVE_LEAN_ALLOCS.fetch_sub(1, Ordering::Relaxed);
    unsafe {
        let raw = ptr.sub(HEADER_SIZE);
        let total = *(raw as *const usize);
        if let Ok(layout) = Layout::from_size_align(total, HEADER_ALIGN) {
            dealloc(raw, layout);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn automatafl_rt_mark_persistent() {
    LIVE_LEAN_ALLOCS.fetch_sub(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn automatafl_rt_panic(msg: *const c_char) -> ! {
    let text = if msg.is_null() {
        "Lean runtime panic"
    } else {
        unsafe { CStr::from_ptr(msg).to_str().unwrap_or("Lean runtime panic") }
    };
    panic!("automatafl-lean runtime panic: {text}");
}

// ============================================================================
// Low-Level FFI Declarations & Lock-Free Thread-Safe Wrappers
// ============================================================================

unsafe extern "C" {
    fn automatafl_lean_init();
    fn automatafl_lean_rules(
        in_ptr: *const u8,
        in_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> usize;
    fn automatafl_lean_packed_stock(preset: u8, out_ptr: *mut u8, out_cap: usize) -> usize;
    fn automatafl_lean_packed_step(
        state_ptr: *const u8,
        state_len: usize,
        moves_ptr: *const u8,
        moves_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> usize;
    fn automatafl_lean_packed_legal_targets(
        state_ptr: *const u8,
        state_len: usize,
        who: u8,
        fx: u8,
        fy: u8,
        live_only: u8,
        out_lo: *mut u64,
        out_hi: *mut u64,
    );
    fn automatafl_lean_packed_adjudicate(state_ptr: *const u8, state_len: usize) -> u32;
}

static LEAN_INIT: Once = Once::new();

#[inline]
fn ensure_lean_init() {
    LEAN_INIT.call_once(|| unsafe {
        automatafl_lean_init();
    });
}

pub(crate) fn ffi_rules_wire(wire: &str) -> String {
    ensure_lean_init();
    let _active = ActiveFfiGuard::enter();
    let mut stack_buf = [0u8; 4096];
    let n = unsafe {
        automatafl_lean_rules(
            wire.as_ptr(),
            wire.len(),
            stack_buf.as_mut_ptr(),
            stack_buf.len(),
        )
    };
    if n <= stack_buf.len() {
        String::from_utf8_lossy(&stack_buf[..n]).into_owned()
    } else {
        let mut heap_buf = vec![0u8; n];
        let n2 = unsafe {
            automatafl_lean_rules(
                wire.as_ptr(),
                wire.len(),
                heap_buf.as_mut_ptr(),
                heap_buf.len(),
            )
        };
        String::from_utf8_lossy(&heap_buf[..n2.min(n)]).into_owned()
    }
}

pub(crate) fn ffi_packed_stock(preset: u8, out: &mut [u8]) -> usize {
    ensure_lean_init();
    let _active = ActiveFfiGuard::enter();
    unsafe { automatafl_lean_packed_stock(preset, out.as_mut_ptr(), out.len()) }
}

pub(crate) fn ffi_packed_step(state: &[u8], moves: &[u8], out: &mut [u8]) -> usize {
    ensure_lean_init();
    let _active = ActiveFfiGuard::enter();
    unsafe {
        automatafl_lean_packed_step(
            state.as_ptr(),
            state.len(),
            moves.as_ptr(),
            moves.len(),
            out.as_mut_ptr(),
            out.len(),
        )
    }
}

pub(crate) fn ffi_packed_legal_targets(
    state: &[u8],
    who: u8,
    fx: u8,
    fy: u8,
    live_only: u8,
) -> (u64, u64) {
    ensure_lean_init();
    let _active = ActiveFfiGuard::enter();
    let mut lo = 0u64;
    let mut hi = 0u64;
    unsafe {
        automatafl_lean_packed_legal_targets(
            state.as_ptr(),
            state.len(),
            who,
            fx,
            fy,
            live_only,
            &mut lo,
            &mut hi,
        );
    }
    (lo, hi)
}

pub(crate) fn ffi_packed_adjudicate(state: &[u8]) -> u32 {
    ensure_lean_init();
    let _active = ActiveFfiGuard::enter();
    unsafe { automatafl_lean_packed_adjudicate(state.as_ptr(), state.len()) }
}
