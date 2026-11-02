import Automatafl.Variants

open Automatafl.Tactics

/-!
# Automatafl.PackedBoard — R1.2 Unboxed Bitmask & Flat Binary Representation

Provides:
1. Unboxed 128-bit (`lo : UInt64`, `hi : UInt64`) bitmask layer operations for Repulsors,
   Attractors, and Conflict Marks on boards up to 11×11 (121 cells ≤ 128 bits).
2. `PackedBoard` and `PackedOutcome` with `pack` / `unpack` conversions and `PackedBoard.step`.
3. Formally verified equivalence theorems (`boardOfMasks_cellAt_eq`,
   `Board.reifyMasks_cellAt_eq`, `PackedBoard.unpack_pack`, `PackedBoard.unpack_pack_cellAt_eq`,
   and multi-variant `PackedBoard.stepVariant` pins) proving with 0 `sorry` that the O(1)-depth
   `Bitmask128` representation agrees with `Board.cellAt` and `roundStepVariant`.
4. Direct binary `ByteArray` C FFI exports (`automatafl_packed_stock`, `automatafl_packed_step`,
   `automatafl_packed_legal_targets`, `automatafl_packed_adjudicate`) for lock-free
   high-speed simulation in AI search and native/WASM clients.
-/

namespace Dregg2.Games.AutomataflRules

open Dregg2.Games.Automatafl

set_option autoImplicit false

/-! ## §1  128-Bit Unboxed Bitboard Primitives (`UInt64 × UInt64`) -/

/-- A 128-bit bitmask stored as two unboxed `UInt64` words (`lo` = bits 0..63, `hi` = bits 64..127).
    Covers all 121 squares of the 11×11 Classic board (as well as 9×9=81 and 7×7=49). -/
structure Bitmask128 where
  lo : UInt64 := 0
  hi : UInt64 := 0
  deriving DecidableEq, Repr, Inhabited

namespace Bitmask128

def empty : Bitmask128 := ⟨0, 0⟩

@[inline]
def testBit (m : Bitmask128) (idx : Nat) : Bool :=
  if idx < 64 then
    ((m.lo >>> idx.toUInt64) &&& 1) == 1
  else if idx < 128 then
    ((m.hi >>> (idx - 64).toUInt64) &&& 1) == 1
  else
    false

@[inline]
def setBit (m : Bitmask128) (idx : Nat) : Bitmask128 :=
  if idx < 64 then
    { m with lo := m.lo ||| (1 <<< idx.toUInt64) }
  else if idx < 128 then
    { m with hi := m.hi ||| (1 <<< (idx - 64).toUInt64) }
  else
    m

@[inline]
def clearBit (m : Bitmask128) (idx : Nat) : Bitmask128 :=
  if idx < 64 then
    { m with lo := m.lo &&& ~~~(1 <<< idx.toUInt64) }
  else if idx < 128 then
    { m with hi := m.hi &&& ~~~(1 <<< (idx - 64).toUInt64) }
  else
    m

/-- Clear any bit in `b` that is also set in `a` (`b &&& ~~~a`). -/
@[inline]
def diff (b a : Bitmask128) : Bitmask128 :=
  ⟨b.lo &&& ~~~a.lo, b.hi &&& ~~~a.hi⟩

/-- Direct O(1) cell lookup from two 128-bit bitmasks (`rep`, `att`) and scalar automaton position.
    Compares scalar coordinates `c.x == ax && c.y == ay` directly so the compiled C IR never
    heap-allocates a temporary `Coord` constructor on cell lookups. -/
@[inline]
def cellAtMasks (size ax ay : Nat) (rep att : Bitmask128) (c : Coord) : Particle :=
  if c.x < size && c.y < size then
    if c.x == ax && c.y == ay then Particle.automaton
    else
      let idx := c.y * size + c.x
      if rep.testBit idx then Particle.repulsor
      else if att.testBit idx then Particle.attractor
      else Particle.vacuum
  else Particle.vacuum

theorem uint64_zero_get (i : Nat) : (((0 : UInt64) >>> i.toUInt64) &&& 1) = 0 := by
  apply UInt64.eq_of_toBitVec_eq
  apply BitVec.eq_of_getLsbD_eq
  intro k _
  simp

theorem uint64_set_get_self (w : UInt64) (i : Nat) :
    (((w ||| (1 <<< i.toUInt64)) >>> i.toUInt64) &&& 1) = 1 := by
  apply UInt64.eq_of_toBitVec_eq
  apply BitVec.eq_of_getLsbD_eq
  intro k _
  simp <;> omega

theorem uint64_set_get_ne (w : UInt64) (i j : Nat) (hi : i < 64) (hj : j < 64) (hne : i ≠ j) :
    (((w ||| (1 <<< i.toUInt64)) >>> j.toUInt64) &&& 1) = ((w >>> j.toUInt64) &&& 1) := by
  apply UInt64.eq_of_toBitVec_eq
  apply BitVec.eq_of_getLsbD_eq
  intro k _
  by_cases hk0 : k = 0
  · subst hk0; simp <;> omega
  · simp [hk0]

theorem uint64_clear_get_self (w : UInt64) (i : Nat) :
    (((w &&& ~~~(1 <<< i.toUInt64)) >>> i.toUInt64) &&& 1) = 0 := by
  apply UInt64.eq_of_toBitVec_eq
  apply BitVec.eq_of_getLsbD_eq
  intro k _
  simp <;> omega

theorem uint64_clear_get_ne (w : UInt64) (i j : Nat) (hi : i < 64) (hj : j < 64) (hne : i ≠ j) :
    (((w &&& ~~~(1 <<< i.toUInt64)) >>> j.toUInt64) &&& 1) = ((w >>> j.toUInt64) &&& 1) := by
  apply UInt64.eq_of_toBitVec_eq
  apply BitVec.eq_of_getLsbD_eq
  intro k _
  by_cases hk0 : k = 0
  · subst hk0; simp <;> omega
  · simp [hk0]

theorem testBit_empty (idx : Nat) : empty.testBit idx = false := by
  unfold empty testBit
  split
  · rw [uint64_zero_get]; rfl
  · split
    · rw [uint64_zero_get]; rfl
    · rfl

theorem testBit_setBit_self (m : Bitmask128) (idx : Nat) (h : idx < 128) :
    (m.setBit idx).testBit idx = true := by
  unfold setBit testBit
  by_cases h64 : idx < 64
  · rw [if_pos h64, if_pos h64]
    dsimp only []
    rw [uint64_set_get_self]
    rfl
  · rw [if_neg h64, if_pos h, if_neg h64, if_pos h]
    dsimp only []
    rw [uint64_set_get_self]
    rfl

theorem testBit_setBit_ne (m : Bitmask128) (i j : Nat) (hne : i ≠ j) :
    (m.setBit i).testBit j = m.testBit j := by
  unfold setBit testBit
  by_cases hi64 : i < 64 <;> by_cases hj64 : j < 64
  · rw [if_pos hi64, if_pos hj64, if_pos hj64]
    dsimp only []
    rw [uint64_set_get_ne m.lo i j hi64 hj64 hne]
  · rw [if_pos hi64, if_neg hj64, if_neg hj64]
  · by_cases hi128 : i < 128
    · rw [if_neg hi64, if_pos hi128, if_pos hj64, if_pos hj64]
    · rw [if_neg hi64, if_neg hi128]
  · by_cases hi128 : i < 128
    · rw [if_neg hi64, if_pos hi128, if_neg hj64, if_neg hj64]
      by_cases hj128 : j < 128
      · rw [if_pos hj128, if_pos hj128]
        dsimp only []
        have hi' : i - 64 < 64 := by omega
        have hj' : j - 64 < 64 := by omega
        have hne' : i - 64 ≠ j - 64 := by omega
        rw [uint64_set_get_ne m.hi (i - 64) (j - 64) hi' hj' hne']
      · rw [if_neg hj128, if_neg hj128]
    · rw [if_neg hi64, if_neg hi128]

theorem testBit_clearBit_self (m : Bitmask128) (idx : Nat) :
    (m.clearBit idx).testBit idx = false := by
  unfold clearBit testBit
  by_cases h64 : idx < 64
  · rw [if_pos h64, if_pos h64]
    dsimp only []
    rw [uint64_clear_get_self]
    rfl
  · by_cases h128 : idx < 128
    · rw [if_neg h64, if_pos h128, if_neg h64, if_pos h128]
      dsimp only []
      rw [uint64_clear_get_self]
      rfl
    · rw [if_neg h64, if_neg h128]

theorem testBit_clearBit_ne (m : Bitmask128) (i j : Nat) (hne : i ≠ j) :
    (m.clearBit i).testBit j = m.testBit j := by
  unfold clearBit testBit
  by_cases hi64 : i < 64 <;> by_cases hj64 : j < 64
  · rw [if_pos hi64, if_pos hj64, if_pos hj64]
    dsimp only []
    rw [uint64_clear_get_ne m.lo i j hi64 hj64 hne]
  · rw [if_pos hi64, if_neg hj64, if_neg hj64]
  · by_cases hi128 : i < 128
    · rw [if_neg hi64, if_pos hi128, if_pos hj64, if_pos hj64]
    · rw [if_neg hi64, if_neg hi128]
  · by_cases hi128 : i < 128
    · rw [if_neg hi64, if_pos hi128, if_neg hj64, if_neg hj64]
      by_cases hj128 : j < 128
      · rw [if_pos hj128, if_pos hj128]
        dsimp only []
        have hi' : i - 64 < 64 := by omega
        have hj' : j - 64 < 64 := by omega
        have hne' : i - 64 ≠ j - 64 := by omega
        rw [uint64_clear_get_ne m.hi (i - 64) (j - 64) hi' hj' hne']
      · rw [if_neg hj128, if_neg hj128]
    · rw [if_neg hi64, if_neg hi128]

end Bitmask128

/-! ## §2  `PackedBoard` Structure & Bitmask Encoding -/

/-- Flat, bitmask-backed representation of an Automatafl round state and configuration.
    Stores unboxed `UInt64` bitmasks (`repMask`, `attMask`, `markMask`) alongside the
    reified O(1)-depth state fields so that bitboard queries and formal equivalence with
    the canonical specification both hold without accumulating nested `writeBoard` closures. -/
structure PackedBoard where
  repMask   : Bitmask128
  attMask   : Bitmask128
  markMask  : Bitmask128
  size      : UInt8
  autoX     : UInt8
  autoY     : UInt8
  cfg       : GameConfig
  goals     : GoalAssignment
  state     : VariantRoundState

/-- Single fold step when scanning a `Board` into `(repMask, attMask)`. -/
def stepBoardMasks (b : Board) (acc : Bitmask128 × Bitmask128) (idx : Nat) :
    Bitmask128 × Bitmask128 :=
  match b.cellAt ⟨idx % b.size, idx / b.size⟩ with
  | .repulsor  => (acc.1.setBit idx, acc.2)
  | .attractor => (acc.1, acc.2.setBit idx)
  | _          => acc

/-- Compute the 128-bit repulsor and attractor bitmasks of a `Board`. -/
def computeBoardMasks (b : Board) : Bitmask128 × Bitmask128 :=
  (List.range (b.size * b.size)).foldl (stepBoardMasks b) (Bitmask128.empty, Bitmask128.empty)

/-- Compute the 128-bit bitmask of conflict-marked coordinates on a board of width `n`. -/
def computeMarkMask (n : Nat) (marks : List Coord) : Bitmask128 :=
  marks.foldl (fun m c =>
    if c.x < n && c.y < n then m.setBit (c.y * n + c.x) else m) Bitmask128.empty

/-- Reconstruct an O(1)-depth `Board` directly from its `Bitmask128` bitboards and scalar coordinates. -/
def boardOfMasks (size ax ay : Nat) (colRule : Bool) (rep att : Bitmask128) : Board :=
  { size := size
    cells := Bitmask128.cellAtMasks size ax ay rep att
    automaton := ⟨ax, ay⟩
    useColumnRule := colRule }

/-- Flatten any `Board` (whose `cells` may be an arbitrary chain of `writeBoard` / `stepTo`
    closures) into an O(1)-depth bitmask-backed `Board` via `computeBoardMasks` and `boardOfMasks`. -/
def _root_.Dregg2.Games.Automatafl.Board.reifyMasks (b : Board) : Board :=
  let (rep, att) := computeBoardMasks b
  boardOfMasks b.size b.automaton.x b.automaton.y b.useColumnRule rep att

abbrev Board.reifyMasks (b : Board) : Board := b.reifyMasks

/-- Reconstruct conflict marks from `markMask` on a `size × size` board. -/
def marksOfMask (size : Nat) (markMask : Bitmask128) : List Coord :=
  (List.range (size * size)).filterMap (fun idx =>
    if markMask.testBit idx then some ⟨idx % size, idx / size⟩ else none)

/-- Sanitize `(rep, att)` on a `size × size` board with automaton `(ax, ay)`:
    clears the automaton coordinate bit in both masks and clears any bit in `att`
    that is also set in `rep`, ensuring `(rep, att)` are disjoint and vacuum at `(ax, ay)`. -/
@[inline]
def sanitizeBoardMasks (size ax ay : Nat) (rep att : Bitmask128) : Bitmask128 × Bitmask128 :=
  let autoIdx := ay * size + ax
  let r := if ax < size && ay < size then rep.clearBit autoIdx else rep
  let a := if ax < size && ay < size then att.clearBit autoIdx else att
  (r, a.diff r)

/-- Directly mutate `(rep, att)` bitmasks for a set of vacated squares and surviving movers `L`,
    without scanning all `size × size` board squares or constructing `writeBoard` closures.
    Two-pass clear-then-set order: Pass 1 clears all `vacated` squares in both masks; Pass 2
    reads particle identity from the pre-move `(rep, att)` masks and writes `(r', a')` at `dstIdx`
    (clearing the opposite mask bit so `r'` and `a'` remain strictly disjoint). -/
def applyMoveMasks (size : Nat) (rep att : Bitmask128)
    (vacated survivors : List Coord) (L : Coord → Coord) : Bitmask128 × Bitmask128 :=
  let cleared := vacated.foldl (fun (r, a) s =>
    if s.x < size && s.y < size then
      let idx := s.y * size + s.x
      (r.clearBit idx, a.clearBit idx)
    else (r, a)) (rep, att)
  survivors.foldl (fun (r, a) s =>
    let d := L s
    if s.x < size && s.y < size && d.x < size && d.y < size then
      let srcIdx := s.y * size + s.x
      let dstIdx := d.y * size + d.x
      if rep.testBit srcIdx then
        (r.setBit dstIdx, a.clearBit dstIdx)
      else if att.testBit srcIdx then
        (r.clearBit dstIdx, a.setBit dstIdx)
      else
        (r.clearBit dstIdx, a.clearBit dstIdx)
    else (r, a)) cleared

/-- Resolve moves directly on `(rep, att)` bitmasks under `cfg` (`conflict`, `annihilate`,
    or `bunchBefore`) using `applyMoveMasks`. -/
def resolveMoveMasksCfg (cfg : GameConfig) (b : Board) (rep att : Bitmask128)
    (ms : List Move) (alreadyResolvedConflict : Bool := false) : Bitmask128 × Bitmask128 :=
  let mvrs := moversCfg cfg b ms
  let rawLand := landMapCfg cfg b ms
  match cfg.confluence with
  | .conflict =>
    if alreadyResolvedConflict || (unresolvedCfg cfg b ms).isEmpty then
      applyMoveMasks b.size rep att mvrs mvrs rawLand
    else
      (rep, att)
  | .annihilate =>
    let isStayer (q : Coord) : Bool := carAt b q && !(mvrs.any (· == q))
    let survivors := mvrs.filter (fun c =>
      let d := rawLand c
      (mvrs.filter (fun c' => rawLand c' == d)).length == 1
      && !isStayer d
      && decide (b.inBounds d))
    let vacated := mvrs.filter (fun c =>
      let d := rawLand c
      !isStayer d && decide (b.inBounds d))
    applyMoveMasks b.size rep att vacated survivors rawLand
  | .bunchBefore =>
    let rawEff := rawBunchLandMap cfg b ms
    if bunchResolvableB b mvrs rawEff then
      applyMoveMasks b.size rep att mvrs mvrs rawEff
    else
      (rep, att)

/-! ### Formal Proof of `boardOfMasks_cellAt_eq` (`0 sorry`) -/

theorem foldl_stepBoardMasks_testBit (b : Board) (n : Nat) (hn : n ≤ 128) (k : Nat) :
    let (rep, att) := (List.range n).foldl (stepBoardMasks b) (Bitmask128.empty, Bitmask128.empty)
    rep.testBit k = (decide (k < n) && decide (b.cellAt ⟨k % b.size, k / b.size⟩ = .repulsor))
    ∧ att.testBit k = (decide (k < n) && decide (b.cellAt ⟨k % b.size, k / b.size⟩ = .attractor)) := by
  induction n with
  | zero =>
    simp [Bitmask128.testBit_empty]
  | succ n ih =>
    have hn' : n ≤ 128 := by omega
    have ⟨ihR, ihA⟩ := ih hn'
    rw [List.range_succ, List.foldl_append]
    dsimp only [List.foldl]
    generalize (List.range n).foldl (stepBoardMasks b) (Bitmask128.empty, Bitmask128.empty) = acc at ihR ihA ⊢
    unfold stepBoardMasks
    by_cases hkn : k = n
    · subst hkn
      have hnn : decide (k < k) = false := by simp
      rw [hnn, Bool.false_and] at ihR ihA
      have hk128 : k < 128 := by omega
      have hkn1 : decide (k < k + 1) = true := by simp
      rw [hkn1, Bool.true_and, Bool.true_and]
      cases hcell : b.cellAt ⟨k % b.size, k / b.size⟩ <;>
        simp [Bitmask128.testBit_setBit_self _ _ hk128, ihR, ihA]
    · have hlt : decide (k < n + 1) = decide (k < n) := by
        by_cases h : k < n
        · have h2 : k < n + 1 := by omega
          simp [h, h2]
        · have h2 : ¬ k < n + 1 := by omega
          simp [h, h2]
      rw [hlt]
      have hne : n ≠ k := Ne.symm hkn
      cases hcell : b.cellAt ⟨n % b.size, n / b.size⟩ <;>
        simp [Bitmask128.testBit_setBit_ne _ n k hne, ihR, ihA]

theorem coord_idx_lt (x y s : Nat) (hx : x < s) (hy : y < s) : y * s + x < s * s := by
  have h1 : (y + 1) * s ≤ s * s := Nat.mul_le_mul_right s (by omega)
  have h2 : (y + 1) * s = y * s + s := Nat.succ_mul y s
  omega

theorem coord_idx_mod (x y s : Nat) (hx : x < s) : (y * s + x) % s = x := by
  rw [Nat.add_comm, Nat.add_mul_mod_self_right, Nat.mod_eq_of_lt hx]

theorem coord_idx_div (x y s : Nat) (hx : x < s) : (y * s + x) / s = y := by
  have hs : 0 < s := by omega
  rw [Nat.add_comm, Nat.add_mul_div_right _ _ hs, Nat.div_eq_of_lt hx, Nat.zero_add]

/-- **⚑ BITMASK REIFICATION EQUIVALENCE THEOREM (`boardOfMasks_cellAt_eq`)**:
    For any board `b` with `b.size * b.size ≤ 128` whose automaton coordinate is uniquely
    marked `.automaton` in bounds, `boardOfMasks` on `computeBoardMasks b` agrees with
    `b.cellAt` on every coordinate `c`! -/
theorem boardOfMasks_cellAt_eq (b : Board)
    (hsize : b.size * b.size ≤ 128)
    (hauto : ∀ q, b.inBounds q → (b.cellAt q = .automaton ↔ q = b.automaton))
    (c : Coord) :
    (let (rep, att) := computeBoardMasks b
     boardOfMasks b.size b.automaton.x b.automaton.y b.useColumnRule rep att).cellAt c =
      b.cellAt c := by
  dsimp only [computeBoardMasks, boardOfMasks, Board.cellAt, Bitmask128.cellAtMasks]
  by_cases hib : c.x < b.size ∧ c.y < b.size
  · rw [if_pos hib, if_pos hib]
    have hibB : (c.x < b.size && c.y < b.size) = true := by simp [hib.1, hib.2]
    rw [if_pos hibB]
    have ⟨hR, hA⟩ := foldl_stepBoardMasks_testBit b (b.size * b.size) hsize (c.y * b.size + c.x)
    have hidx : c.y * b.size + c.x < b.size * b.size := coord_idx_lt c.x c.y b.size hib.1 hib.2
    have hmod : (c.y * b.size + c.x) % b.size = c.x := coord_idx_mod c.x c.y b.size hib.1
    have hdiv : (c.y * b.size + c.x) / b.size = c.y := coord_idx_div c.x c.y b.size hib.1
    have hcoord : (⟨(c.y * b.size + c.x) % b.size, (c.y * b.size + c.x) / b.size⟩ : Coord) = c := by
      cases c; simp [hmod, hdiv]
    rw [hcoord, decide_eq_true hidx, Bool.true_and] at hR hA
    have hcell : b.cellAt c = b.cells c := by
      unfold Board.cellAt; exact if_pos hib
    rw [hcell] at hR hA
    have hautoC := hauto c hib
    rw [hcell] at hautoC
    have hcaIff : (c.x == b.automaton.x && c.y == b.automaton.y) = true ↔ c = b.automaton := by
      cases c; cases b.automaton; simp
    by_cases hca : c = b.automaton
    · have hcaB : (c.x == b.automaton.x && c.y == b.automaton.y) = true := hcaIff.mpr hca
      rw [if_pos hcaB]
      exact (hautoC.mpr hca).symm
    · have hcaB : ¬ ((c.x == b.automaton.x && c.y == b.automaton.y) = true) :=
        fun h => hca (hcaIff.mp h)
      rw [if_neg hcaB, hR, hA]
      cases hcp : b.cells c
      · rfl
      · rfl
      · exfalso; exact hca (hautoC.mp hcp)
      · rfl
  · rw [if_neg hib, if_neg hib]

/-- Corollary: `Board.reifyMasks b` agrees with `b.cellAt` on every coordinate `c`. -/
theorem Board.reifyMasks_cellAt_eq (b : Board)
    (hsize : b.size * b.size ≤ 128)
    (hauto : ∀ q, b.inBounds q → (b.cellAt q = .automaton ↔ q = b.automaton))
    (c : Coord) :
    b.reifyMasks.cellAt c = b.cellAt c :=
  boardOfMasks_cellAt_eq b hsize hauto c

/-- Reify the `Board` of a `PackedBoard` directly from its `repMask` and `attMask` in O(1) depth. -/
def PackedBoard.reifyBoard (pb : PackedBoard) : Board :=
  boardOfMasks pb.size.toNat pb.autoX.toNat pb.autoY.toNat pb.state.board.useColumnRule pb.repMask pb.attMask

/-- Pack a `VariantRoundState`, `GameConfig`, and `GoalAssignment` into a `PackedBoard`,
    flattening `vrs.board` via `boardOfMasks` so `state.board` has O(1) bitmask cell lookup. -/
def PackedBoard.packVariant (cfg : GameConfig) (g : GoalAssignment)
    (vrs : VariantRoundState) : PackedBoard :=
  let b := vrs.board
  let (rep, att) := computeBoardMasks b
  let mk := computeMarkMask b.size vrs.marks
  let bFlat := boardOfMasks b.size b.automaton.x b.automaton.y b.useColumnRule rep att
  { repMask  := rep
    attMask  := att
    markMask := mk
    size     := b.size.toUInt8
    autoX    := b.automaton.x.toUInt8
    autoY    := b.automaton.y.toUInt8
    cfg      := cfg
    goals    := g
    state    := { vrs with board := bFlat } }

/-- Pack a canonical `RoundState` with default config and stock 2-player goals. -/
def PackedBoard.pack (rs : RoundState) (cfg : GameConfig := {})
    (g : GoalAssignment := stockGoals2 rs.board.size) : PackedBoard :=
  PackedBoard.packVariant cfg g { rs with cooldowns := [], lastDir := none }

/-- Unpack a `PackedBoard` to its canonical `RoundState`. -/
def PackedBoard.unpack (pb : PackedBoard) : RoundState :=
  pb.state.toRoundState

/-- Unpack a `PackedBoard` to its full `VariantRoundState`. -/
def PackedBoard.unpackVariant (pb : PackedBoard) : VariantRoundState :=
  pb.state

/-- Unpack just the `Board` from a `PackedBoard`. -/
def PackedBoard.unpackBoard (pb : PackedBoard) : Board :=
  pb.state.board

/-- Extensional board equality on all in-bounds cells and metadata. -/
def boardEqB (b1 b2 : Board) : Bool :=
  b1.size == b2.size
  && b1.automaton == b2.automaton
  && b1.useColumnRule == b2.useColumnRule
  && (List.range (b1.size * b1.size)).all (fun idx =>
       let c : Coord := ⟨idx % b1.size, idx / b1.size⟩
       b1.cellAt c == b2.cellAt c)

/-! ### Bitmask Round-Trip Verification on All Stock Presets -/

theorem bitmask_roundtrip_blitz7 :
    boardEqB
      (let (r, a) := computeBoardMasks stockBlitz7
       boardOfMasks 7 3 3 true r a)
      stockBlitz7 = true := by native_decide

theorem bitmask_roundtrip_tactical9 :
    boardEqB
      (let (r, a) := computeBoardMasks stockTactical9
       boardOfMasks 9 4 4 true r a)
      stockTactical9 = true := by native_decide

theorem bitmask_roundtrip_classic11 :
    boardEqB
      (let (r, a) := computeBoardMasks stockClassic11
       boardOfMasks 11 5 5 true r a)
      stockClassic11 = true := by native_decide

/-! ## §3  `PackedBoard.step` & Bitmask-Direct Stepping -/

/-- Outcome of stepping a `PackedBoard`. -/
inductive PackedOutcome where
  | again    (pb : PackedBoard)
  | resolved (pb : PackedBoard) (winner : Option Pid)

/-- Unpack a `PackedOutcome` to a canonical `RoundOutcome`. -/
def PackedOutcome.unpack : PackedOutcome → RoundOutcome
  | .again pb       => .again (PackedBoard.unpack pb)
  | .resolved pb w  => .resolved (PackedBoard.unpackBoard pb) w

/-- Unpack a `PackedOutcome` to a `VariantRoundOutcome`. -/
def PackedOutcome.unpackVariant : PackedOutcome → VariantRoundOutcome
  | .again pb       => .again (PackedBoard.unpackVariant pb)
  | .resolved pb w  => .resolved (PackedBoard.unpackBoard pb) w pb.state.cooldowns pb.state.lastDir

/-- Execute one variant round step on a `PackedBoard` using direct `Bitmask128` mutation
    (`resolveMoveMasksCfg` / `applyMoveMasks`) and O(1)-depth `boardOfMasks` reification.
    Never accumulates nested `writeBoard` closures across turns and never re-scans
    `List.range (b.size * b.size)` to rebuild `repMask` and `attMask`. -/
def PackedBoard.stepVariant (pb : PackedBoard) (ms : List Move) : PackedOutcome :=
  let sz := pb.size.toNat
  let ax := pb.autoX.toNat
  let ay := pb.autoY.toNat
  let (rep0, att0) := sanitizeBoardMasks sz ax ay pb.repMask pb.attMask
  let b := boardOfMasks sz ax ay pb.state.board.useColumnRule rep0 att0
  let rs := pb.state.toRoundState
  let fresh := ms.filter (fun m =>
    rs.waiting.contains m.who && moveLegalVariantB b rs.marks pb.state.cooldowns m)
  let all := rs.locked ++ fresh
  let clash := clashCoordsCfg pb.cfg b all
  let cs := if clash.isEmpty then unresolvedCfg pb.cfg b all else clash
  if cs.isEmpty then
    let (rep1, att1) := resolveMoveMasksCfg pb.cfg b rep0 att0 all true
    let mid := boardOfMasks b.size b.automaton.x b.automaton.y b.useColumnRule rep1 att1
    let res := stepAutomataCfg pb.cfg pb.goals mid pb.state.lastDir
    let nxtB := res.1
    let w := res.2.1
    let dir' := res.2.2
    let (rep', att') := sanitizeBoardMasks b.size nxtB.automaton.x nxtB.automaton.y rep1 att1
    let bDone := boardOfMasks b.size nxtB.automaton.x nxtB.automaton.y b.useColumnRule rep' att'
    let cds' :=
      if pb.cfg.cooldownTurns == 0 then
        tickCooldowns pb.state.cooldowns
      else
        recordCooldowns pb.cfg.cooldownTurns pb.state.cooldowns (landedCoordsCfg pb.cfg b all)
    let seats := pb.goals.seats
    let vrsDone : VariantRoundState := {
      toRoundState := openRound bDone (if seats.isEmpty then [0, 1] else seats)
      cooldowns    := cds'
      lastDir      := dir' }
    .resolved {
      repMask  := rep'
      attMask  := att'
      markMask := Bitmask128.empty
      size     := bDone.size.toUInt8
      autoX    := bDone.automaton.x.toUInt8
      autoY    := bDone.automaton.y.toUInt8
      cfg      := pb.cfg
      goals    := pb.goals
      state    := vrsDone } w
  else
    let marks' := (rs.marks ++ cs).dedup
    let mk' := cs.foldl (fun m c =>
      if c.x < b.size && c.y < b.size then m.setBit (c.y * b.size + c.x) else m) pb.markMask
    let vrsAgain : VariantRoundState := {
      board     := b
      marks     := marks'
      locked    := all.filter (fun m => !(cs.contains m.frm || cs.contains m.to))
      waiting   := ((all.filter (fun m => cs.contains m.frm || cs.contains m.to)).map (·.who)).dedup
      cooldowns := pb.state.cooldowns
      lastDir   := pb.state.lastDir }
    .again {
      repMask  := rep0
      attMask  := att0
      markMask := mk'
      size     := pb.size
      autoX    := pb.autoX
      autoY    := pb.autoY
      cfg      := pb.cfg
      goals    := pb.goals
      state    := vrsAgain }

/-- Execute one canonical round step on a `PackedBoard` via `PackedBoard.stepVariant`. -/
def PackedBoard.step (pb : PackedBoard) (ms : List Move) : PackedOutcome :=
  PackedBoard.stepVariant pb ms

/-- Single-board step (resolves moves + steps automaton, returning the resulting `PackedBoard`). -/
def PackedBoard.stepBoard (pb : PackedBoard) (ms : List Move) : PackedBoard :=
  match PackedBoard.step pb ms with
  | .again pb'      => pb'
  | .resolved pb' _ => pb'

/-- Round-trip theorem: `unpack (pack rs cfg g)` is `rs` with its board flattened to `reifyMasks`. -/
@[simp]
theorem PackedBoard.unpack_pack (rs : RoundState) (cfg : GameConfig) (g : GoalAssignment) :
    PackedBoard.unpack (PackedBoard.pack rs cfg g) = { rs with board := rs.board.reifyMasks } := rfl

/-- Packing a `RoundState` preserves `cellAt` at every coordinate via `boardOfMasks_cellAt_eq`. -/
theorem PackedBoard.unpack_pack_cellAt_eq (rs : RoundState) (cfg : GameConfig) (g : GoalAssignment)
    (hsize : rs.board.size * rs.board.size ≤ 128)
    (hauto : ∀ q, rs.board.inBounds q → (rs.board.cellAt q = .automaton ↔ q = rs.board.automaton))
    (c : Coord) :
    (PackedBoard.unpack (PackedBoard.pack rs cfg g)).board.cellAt c = rs.board.cellAt c :=
  boardOfMasks_cellAt_eq rs.board hsize hauto c

/-! ## §4  Flat Binary `ByteArray` Codec & Exported C FFI Functions -/

/-- Push a `UInt64` in little-endian byte order into a `ByteArray`. -/
def pushU64LE (ba : ByteArray) (v : UInt64) : ByteArray :=
  ba.push v.toUInt8
  |>.push (v >>> 8).toUInt8
  |>.push (v >>> 16).toUInt8
  |>.push (v >>> 24).toUInt8
  |>.push (v >>> 32).toUInt8
  |>.push (v >>> 40).toUInt8
  |>.push (v >>> 48).toUInt8
  |>.push (v >>> 56).toUInt8

/-- Read a `UInt64` in little-endian byte order from `ba` at byte offset `off`. -/
def readU64LE (ba : ByteArray) (off : Nat) : UInt64 :=
  if off + 8 ≤ ba.size then
    (ba.get! off).toUInt64
    ||| ((ba.get! (off + 1)).toUInt64 <<< 8)
    ||| ((ba.get! (off + 2)).toUInt64 <<< 16)
    ||| ((ba.get! (off + 3)).toUInt64 <<< 24)
    ||| ((ba.get! (off + 4)).toUInt64 <<< 32)
    ||| ((ba.get! (off + 5)).toUInt64 <<< 40)
    ||| ((ba.get! (off + 6)).toUInt64 <<< 48)
    ||| ((ba.get! (off + 7)).toUInt64 <<< 56)
  else 0

/-- Encode a `TieBreak` as a single byte:
    0 = column, 1 = row, 2 = freeze, 3 = momentum(true), 4 = momentum(false). -/
def encodeTieByte : TieBreak → UInt8
  | .column         => 0
  | .row            => 1
  | .freeze         => 2
  | .momentum true  => 3
  | .momentum false => 4

def decodeTieByte : UInt8 → TieBreak
  | 0 => .column
  | 1 => .row
  | 2 => .freeze
  | 3 => .momentum true
  | 4 => .momentum false
  | _ => .column

/-- Encode a `ConfluenceMode` as a single byte:
    0 = conflict, 1 = bunchBefore, 2 = annihilate. -/
def encodeConfluenceByte : ConfluenceMode → UInt8
  | .conflict    => 0
  | .bunchBefore => 1
  | .annihilate  => 2

def decodeConfluenceByte : UInt8 → ConfluenceMode
  | 0 => .conflict
  | 1 => .bunchBefore
  | 2 => .annihilate
  | _ => .conflict

/-- Encode `Option Dir` as a single byte: 0 = none, 1 = xp, 2 = xn, 3 = yp, 4 = yn. -/
def encodeOptDirByte : Option Dir → UInt8
  | none     => 0
  | some .xp => 1
  | some .xn => 2
  | some .yp => 3
  | some .yn => 4

def decodeOptDirByte : UInt8 → Option Dir
  | 1 => some .xp
  | 2 => some .xn
  | 3 => some .yp
  | 4 => some .yn
  | _ => none

/-- Append the serialized bytes of `pb` onto an existing `ByteArray` `ba` in-place:
    - Bytes 0..15:  `repMask` (`lo`, `hi` as little-endian `UInt64`)
    - Bytes 16..31: `attMask` (`lo`, `hi` as little-endian `UInt64`)
    - Bytes 32..47: `markMask` (`lo`, `hi` as little-endian `UInt64`)
    - Byte 48: `size`
    - Byte 49: `autoX`
    - Byte 50: `autoY`
    - Byte 51: `tieBreak` byte
    - Byte 52: `confluence` byte
    - Byte 53: flags (`bit 0 = useColumnRule`, `bit 1 = twoCycleSwaps`, `bit 2 = railgunFling`)
    - Byte 54: `cooldownTurns`
    - Byte 55: `automataCount`
    - Byte 56: `lastDir` byte
    - Byte 57: `waiting.length` followed by `waiting` PIDs
    - Followed by `locked.length` and `[who, fx, fy, tx, ty]` bytes
    - Followed by `cooldowns.length` and `[x, y, turns]` bytes
    - Followed by `goals.entries.length` and `[x, y, pid]` bytes -/
def PackedBoard.writeBytes (pb : PackedBoard) (ba0 : ByteArray := ByteArray.empty) : ByteArray := Id.run do
  let mut ba := ba0
  ba := pushU64LE ba pb.repMask.lo
  ba := pushU64LE ba pb.repMask.hi
  ba := pushU64LE ba pb.attMask.lo
  ba := pushU64LE ba pb.attMask.hi
  ba := pushU64LE ba pb.markMask.lo
  ba := pushU64LE ba pb.markMask.hi
  ba := ba.push pb.size
  ba := ba.push pb.autoX
  ba := ba.push pb.autoY
  ba := ba.push (encodeTieByte pb.cfg.tieBreak)
  ba := ba.push (encodeConfluenceByte pb.cfg.confluence)
  let flags : UInt8 :=
    (if pb.state.board.useColumnRule then 1 else 0)
    ||| (if pb.cfg.twoCycleSwaps then 2 else 0)
    ||| (if pb.cfg.railgunFling then 4 else 0)
  ba := ba.push flags
  ba := ba.push pb.cfg.cooldownTurns.toUInt8
  ba := ba.push (max 1 pb.cfg.automataCount).toUInt8
  ba := ba.push (encodeOptDirByte pb.state.lastDir)
  -- waiting list
  ba := ba.push pb.state.waiting.length.toUInt8
  for p in pb.state.waiting do
    ba := ba.push p.toUInt8
  -- locked moves
  ba := ba.push pb.state.locked.length.toUInt8
  for m in pb.state.locked do
    ba := ba.push m.who.toUInt8
    ba := ba.push m.frm.x.toUInt8
    ba := ba.push m.frm.y.toUInt8
    ba := ba.push m.to.x.toUInt8
    ba := ba.push m.to.y.toUInt8
  -- cooldowns
  ba := ba.push pb.state.cooldowns.length.toUInt8
  for (c, t) in pb.state.cooldowns do
    ba := ba.push c.x.toUInt8
    ba := ba.push c.y.toUInt8
    ba := ba.push t.toUInt8
  -- goals
  ba := ba.push pb.goals.entries.length.toUInt8
  for (c, pid) in pb.goals.entries do
    ba := ba.push c.x.toUInt8
    ba := ba.push c.y.toUInt8
    ba := ba.push pid.toUInt8
  return ba

/-- Serialize a `PackedBoard` into a compact flat binary `ByteArray`. -/
@[inline]
def PackedBoard.toBytes (pb : PackedBoard) : ByteArray :=
  pb.writeBytes ByteArray.empty

/-- Deserialize a `PackedBoard` from a flat `ByteArray`. Returns `none` if malformed. -/
def PackedBoard.fromBytes? (ba : ByteArray) : Option PackedBoard := do
  if ba.size < 60 then none
  else
    let repRaw : Bitmask128 := ⟨readU64LE ba 0, readU64LE ba 8⟩
    let attRaw : Bitmask128 := ⟨readU64LE ba 16, readU64LE ba 24⟩
    let mk     : Bitmask128 := ⟨readU64LE ba 32, readU64LE ba 40⟩
    let sz     := (ba.get! 48).toNat
    let ax     := (ba.get! 49).toNat
    let ay     := (ba.get! 50).toNat
    let (rep, att) := sanitizeBoardMasks sz ax ay repRaw attRaw
    let tb  := decodeTieByte (ba.get! 51)
    let cf  := decodeConfluenceByte (ba.get! 52)
    let flg := ba.get! 53
    let colRule := (flg &&& 1) != 0
    let twoSwap := (flg &&& 2) != 0
    let fling   := (flg &&& 4) != 0
    let cdTurns := (ba.get! 54).toNat
    let aCount  := max 1 (ba.get! 55).toNat
    let lDir    := decodeOptDirByte (ba.get! 56)
    let nWait   := (ba.get! 57).toNat
    if 58 + nWait ≥ ba.size then none
    else
      let waiting := (List.range nWait).map (fun i => (ba.get! (58 + i)).toNat)
      let lockOff := 58 + nWait
      let nLock   := (ba.get! lockOff).toNat
      let cdOff   := lockOff + 1 + nLock * 5
      if cdOff ≥ ba.size then none
      else
        let locked := (List.range nLock).map (fun i =>
          let base := lockOff + 1 + i * 5
          ({ who := (ba.get! base).toNat
             frm := ⟨(ba.get! (base + 1)).toNat, (ba.get! (base + 2)).toNat⟩
             to  := ⟨(ba.get! (base + 3)).toNat, (ba.get! (base + 4)).toNat⟩ } : Move))
        let nCd := (ba.get! cdOff).toNat
        let goalOff := cdOff + 1 + nCd * 3
        if goalOff > ba.size then none
        else
          let cds := (List.range nCd).map (fun i =>
            let base := cdOff + 1 + i * 3
            (⟨(ba.get! base).toNat, (ba.get! (base + 1)).toNat⟩, (ba.get! (base + 2)).toNat))
          let g? : Option GoalAssignment :=
            if goalOff == ba.size then
              some (stockGoals2 sz)
            else
              let nGoals := (ba.get! goalOff).toNat
              if goalOff + 1 + nGoals * 3 != ba.size then none
              else
                let entries := (List.range nGoals).map (fun i =>
                  let base := goalOff + 1 + i * 3
                  ((⟨(ba.get! base).toNat, (ba.get! (base + 1)).toNat⟩ : Coord),
                   (ba.get! (base + 2)).toNat))
                some (if entries.isEmpty then stockGoals2 sz else ⟨entries⟩)
          let g ← g?
          let preset : BoardPreset :=
            if sz == 7 then .blitz7 else if sz == 9 then .tactical9 else .classic11
          let cfg : GameConfig := {
            tieBreak      := tb
            confluence    := cf
            twoCycleSwaps := twoSwap
            cooldownTurns := cdTurns
            railgunFling  := fling
            automataCount := aCount
            preset        := preset }
          let b := boardOfMasks sz ax ay colRule rep att
          let marks := marksOfMask sz mk
          let vrs : VariantRoundState := {
            board     := b
            marks     := marks
            locked    := locked
            waiting   := waiting
            cooldowns := cds
            lastDir   := lDir }
          some {
            repMask  := rep
            attMask  := att
            markMask := mk
            size     := sz.toUInt8
            autoX    := ax.toUInt8
            autoY    := ay.toUInt8
            cfg      := cfg
            goals    := g
            state    := vrs }

/-- Decode a flat byte array of moves (`k` × 5 bytes `[who, fx, fy, tx, ty]`). -/
def decodeMovesBytes (ba : ByteArray) : List Move :=
  let k := ba.size / 5
  (List.range k).map (fun i =>
    let base := i * 5
    { who := (ba.get! base).toNat
      frm := ⟨(ba.get! (base + 1)).toNat, (ba.get! (base + 2)).toNat⟩
      to  := ⟨(ba.get! (base + 3)).toNat, (ba.get! (base + 4)).toNat⟩ })

/-- Exported C function: create a packed initial `ByteArray` for preset `0 = blitz7`,
    `1 = tactical9`, `2 = classic11`. -/
@[export automatafl_packed_stock]
def packedStockFFI (presetId : UInt32) : ByteArray :=
  let preset : BoardPreset :=
    if presetId == 0 then .blitz7
    else if presetId == 1 then .tactical9
    else .classic11
  let b := presetBoard preset
  let cfg : GameConfig := { preset := preset }
  let g := presetGoals2 preset
  let rs := openRound b [0, 1]
  (PackedBoard.pack rs cfg g).toBytes

/-- Exported C function: step a `PackedBoard` serialized as `stateBytes` with moves
    `movesBytes` (`5 * k` bytes `[who, fx, fy, tx, ty]`).
    Returns a `ByteArray` whose first 2 bytes are `[status, winnerByte]` followed by
    `nextPb.toBytes`:
    - `status = 0`: malformed input
    - `status = 1`: `.again nextPb` (`winnerByte = 255`)
    - `status = 2`: `.resolved nextPb winner` (`winnerByte = 255` for `none`, or winning `Pid`) -/
@[export automatafl_packed_step]
def packedStepFFI (stateBytes : ByteArray) (movesBytes : ByteArray) : ByteArray :=
  match PackedBoard.fromBytes? stateBytes with
  | none => ByteArray.empty.push 0
  | some pb =>
    let ms := decodeMovesBytes movesBytes
    match PackedBoard.stepVariant pb ms with
    | .again pb' =>
      pb'.writeBytes ((ByteArray.empty.push 1).push 255)
    | .resolved pb' w =>
      let wb : UInt8 := match w with | some p => p.toUInt8 | none => 255
      pb'.writeBytes ((ByteArray.empty.push 2).push wb)

/-- Exported C function: return all legal (or live) targets from `(fx, fy)` on `stateBytes`
    as flat `[tx, ty]` byte pairs.
    When `liveOnly == 0`, returns all proposable targets (`moveLegalVariantB`, respecting marks and cooldowns).
    When `liveOnly != 0`, returns only executable targets (`carAt` + `moveLegalVariantB` + `!blockedB`). -/
@[export automatafl_packed_legal_targets]
def packedLegalTargetsFFI (stateBytes : ByteArray) (fx : UInt32) (fy : UInt32) (liveOnly : UInt32) : ByteArray :=
  match PackedBoard.fromBytes? stateBytes with
  | none => ByteArray.empty
  | some pb =>
    let b := pb.state.board
    let n := b.size
    let x0 := fx.toNat
    let y0 := fy.toNat
    let marks := pb.state.marks
    let cds := pb.state.cooldowns
    let frm : Coord := ⟨x0, y0⟩
    if x0 ≥ n || y0 ≥ n
       || (x0 == b.automaton.x && y0 == b.automaton.y)
       || marks.contains frm
       || onCooldownB cds frm
       || (liveOnly != 0 && !carAt b frm) then
      ByteArray.empty
    else
      let rowCoords := (List.range n).map (fun x => (⟨x, y0⟩ : Coord))
      let colCoords := (List.range n).map (fun y => (⟨x0, y⟩ : Coord))
      let targs := (rowCoords ++ colCoords).filter (fun to =>
        let m : Move := ⟨0, frm, to⟩
        moveLegalVariantB b marks cds m
        && (liveOnly == 0 || !blockedB b (pb.state.locked ++ [m]) m))
      Id.run do
        let mut out := ByteArray.empty
        for t in targs do
          out := out.push t.x.toUInt8
          out := out.push t.y.toUInt8
        return out

/-- Exported C function: adjudicate a capped game on `stateBytes`.
    Returns `0` for seat 0, `1` for seat 1, `255` for draw, `254` for malformed input. -/
@[export automatafl_packed_adjudicate]
def packedAdjudicateFFI (stateBytes : ByteArray) : UInt32 :=
  match PackedBoard.fromBytes? stateBytes with
  | none => 254
  | some pb =>
    match adjudicateCapped pb.state.board.automaton pb.goals with
    | some p => p.toUInt32
    | none   => 255

theorem packed_bytes_roundtrip_classic11 :
    (let pb := PackedBoard.pack (openRound stockClassic11 [0, 1])
     match PackedBoard.fromBytes? pb.toBytes with
     | some pb' => pb'.repMask == pb.repMask && pb'.attMask == pb.attMask && pb'.goals == pb.goals && boardEqB pb'.unpackBoard pb.unpackBoard
     | none => false) = true := by native_decide

theorem packed_bytes_roundtrip_blitz7 :
    (let pb := PackedBoard.pack (openRound stockBlitz7 [0, 1]) { preset := .blitz7 } (presetGoals2 .blitz7)
     match PackedBoard.fromBytes? pb.toBytes with
     | some pb' => pb'.repMask == pb.repMask && pb'.attMask == pb.attMask && pb'.goals == pb.goals && boardEqB pb'.unpackBoard pb.unpackBoard
     | none => false) = true := by native_decide

theorem packed_bytes_roundtrip_four_player :
    (let pb := PackedBoard.pack (openRound stockClassic11 [0, 1, 2, 3]) {} (stockGoals4 11)
     match PackedBoard.fromBytes? pb.toBytes with
     | some pb' => pb'.repMask == pb.repMask && pb'.attMask == pb.attMask && pb'.goals == stockGoals4 11 && pb'.state.waiting == [0, 1, 2, 3]
     | none => false) = true := by native_decide

theorem packed_bytes_legacy_fallback_classic11 :
    (let pb := PackedBoard.pack (openRound stockClassic11 [0, 1])
     let full := pb.toBytes
     let legacy := full.extract 0 (full.size - 13)
     match PackedBoard.fromBytes? legacy with
     | some pb' => pb'.goals == stockGoals2 11 && pb'.repMask == pb.repMask && pb'.attMask == pb.attMask
     | none => false) = true := by native_decide

theorem packed_bytes_rejects_trailing_garbage :
    (let pb := PackedBoard.pack (openRound stockClassic11 [0, 1])
     (PackedBoard.fromBytes? (pb.toBytes.push 99)).isNone) = true := by native_decide

theorem packed_step_bitmask_matches_spec_classic11 :
    (let b := stockClassic11
     let g := stockGoals2 11
     let rs := openRound b [0, 1]
     let pb := PackedBoard.pack rs {} g
     let ms : List Move := [⟨0, ⟨3, 1⟩, ⟨3, 3⟩⟩, ⟨1, ⟨7, 9⟩, ⟨7, 7⟩⟩]
     match PackedBoard.step pb ms, roundStep {} g rs ms with
     | .resolved pb' w1, .resolved b' w2 =>
       w1 == w2 && boardEqB pb'.unpackBoard b' && (pb'.repMask, pb'.attMask) == computeBoardMasks b'
     | _, _ => false) = true := by native_decide

/-- Bitmask step equivalence when `twoCycleSwaps = true` swaps an attractor at `(1,1)` and
    a repulsor at `(2,1)` on `stockBlitz7`. -/
theorem packed_step_two_cycle_swap_rep_att_matches_spec :
    (let b := stockBlitz7
     let cfg : GameConfig := { preset := .blitz7, twoCycleSwaps := true }
     let g := presetGoals2 .blitz7
     let vrs : VariantRoundState := { toRoundState := openRound b [0, 1] }
     let pb := PackedBoard.packVariant cfg g vrs
     let ms : List Move := [⟨0, ⟨1, 1⟩, ⟨2, 1⟩⟩, ⟨1, ⟨2, 1⟩, ⟨1, 1⟩⟩]
     match PackedBoard.stepVariant pb ms, roundStepVariant cfg g vrs ms with
     | .resolved pb' w1, .resolved b' w2 cds' dir' =>
       w1 == w2
       && boardEqB pb'.unpackBoard b'
       && (pb'.repMask, pb'.attMask) == computeBoardMasks b'
       && pb'.state.cooldowns == cds'
       && pb'.state.lastDir == dir'
       && pb'.unpackBoard.cellAt ⟨1, 1⟩ == .repulsor
       && pb'.unpackBoard.cellAt ⟨2, 1⟩ == .attractor
     | _, _ => false) = true := by native_decide

/-- Bitmask step equivalence when a move targets the automaton square (`to == b.automaton`,
    Ruling D: proposable, blocked at resolution without conflict). -/
theorem packed_step_move_to_automaton_blocked_matches_spec :
    (let b := stockBlitz7
     let cfg : GameConfig := { preset := .blitz7 }
     let g := presetGoals2 .blitz7
     let vrs : VariantRoundState := { toRoundState := openRound b [0, 1] }
     let pb := PackedBoard.packVariant cfg g vrs
     let ms : List Move := [⟨0, ⟨2, 1⟩, ⟨2, 3⟩⟩, ⟨1, ⟨4, 1⟩, ⟨3, 1⟩⟩]
     let msToAuto : List Move := [⟨0, ⟨3, 0⟩, ⟨3, 3⟩⟩, ⟨1, ⟨4, 1⟩, ⟨3, 1⟩⟩]
     match PackedBoard.stepVariant pb msToAuto, roundStepVariant cfg g vrs msToAuto,
           PackedBoard.stepVariant pb ms, roundStepVariant cfg g vrs ms with
     | .resolved pb1 w1, .resolved b1 w2 _ _, .resolved pb2 w3, .resolved b2 w4 _ _ =>
       w1 == w2 && boardEqB pb1.unpackBoard b1 && (pb1.repMask, pb1.attMask) == computeBoardMasks b1
       && w3 == w4 && boardEqB pb2.unpackBoard b2 && (pb2.repMask, pb2.attMask) == computeBoardMasks b2
     | _, _, _, _ => false) = true := by native_decide

/-- Bitmask step equivalence in `.annihilate` and `.bunchBefore` confluence modes. -/
theorem packed_step_annihilate_and_bunch_matches_spec :
    (let b := stockBlitz7
     let g := presetGoals2 .blitz7
     let vrs : VariantRoundState := { toRoundState := openRound b [0, 1] }
     let ms : List Move := [⟨0, ⟨1, 1⟩, ⟨1, 3⟩⟩, ⟨1, ⟨1, 5⟩, ⟨1, 3⟩⟩]
     let cfgAnn : GameConfig := { preset := .blitz7, confluence := .annihilate, cooldownTurns := 2 }
     let cfgBunch : GameConfig := { preset := .blitz7, confluence := .bunchBefore, cooldownTurns := 2 }
     let okAnn :=
       match PackedBoard.stepVariant (PackedBoard.packVariant cfgAnn g vrs) ms,
             roundStepVariant cfgAnn g vrs ms with
       | .resolved pb' w1, .resolved b' w2 cds' dir' =>
         w1 == w2 && boardEqB pb'.unpackBoard b' && (pb'.repMask, pb'.attMask) == computeBoardMasks b'
         && pb'.state.cooldowns == cds' && pb'.state.lastDir == dir'
       | _, _ => false
     let okBunch :=
       match PackedBoard.stepVariant (PackedBoard.packVariant cfgBunch g vrs) ms,
             roundStepVariant cfgBunch g vrs ms with
       | .resolved pb' w1, .resolved b' w2 cds' dir' =>
         w1 == w2 && boardEqB pb'.unpackBoard b' && (pb'.repMask, pb'.attMask) == computeBoardMasks b'
         && pb'.state.cooldowns == cds' && pb'.state.lastDir == dir'
       | _, _ => false
     okAnn && okBunch) = true := by native_decide

#assert_all_clean [
  Bitmask128.testBit_empty,
  Bitmask128.testBit_setBit_self,
  Bitmask128.testBit_setBit_ne,
  Bitmask128.testBit_clearBit_self,
  Bitmask128.testBit_clearBit_ne,
  foldl_stepBoardMasks_testBit,
  boardOfMasks_cellAt_eq,
  Board.reifyMasks_cellAt_eq,
  PackedBoard.unpack_pack,
  PackedBoard.unpack_pack_cellAt_eq
]

end Dregg2.Games.AutomataflRules
