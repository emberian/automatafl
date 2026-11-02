import Automatafl.Rules
import Automatafl.Termination

open Automatafl.Tactics

/-!
# Automatafl.Variants — R2 `GameConfig` Generalizations & Variants

Implements configurable game rules over the canonical `AutomataflRules` substrate:
- **Tie-break modes**: `column`, `row`, `freeze`, `momentum (preferSameAxis : Bool)`
- **Confluence modes**:
  - `conflict` (canonical mark & re-enter)
  - `bunchBefore` (stop 1 cell short of collision without re-entry, per `MERGE_RESOLUTION_DESIGN.md`)
  - `annihilate` (colliding movers destroy each other)
- **Configuration fields**:
  - `twoCycleSwaps : Bool` (whether 2-cycles swap pieces or stay put)
  - `cooldownTurns : Nat` (turns a moved piece's landing square cannot be moved from)
  - `railgunFling : Bool` (automaton flings along its step ray through contiguous vacuum)
  - `automataCount : Nat` (support for multiple automata steps)
  - `preset : BoardPreset` (7×7 Blitz, 9×9 Tactical, 11×11 Classic)
-/

namespace Dregg2.Games.AutomataflRules

open Dregg2.Games.Automatafl

set_option autoImplicit false

/-! ## §1  Board Size Presets (7×7 Blitz, 9×9 Tactical, 11×11 Classic) -/

private def pRowR (y : Nat) (xs : List Nat) : List (Coord × Particle) :=
  xs.map (fun x => (⟨x, y⟩, Particle.repulsor))

private def pRowA (y : Nat) (xs : List Nat) : List (Coord × Particle) :=
  xs.map (fun x => (⟨x, y⟩, Particle.attractor))

/-- 7×7 Blitz opening board: symmetric layout with automaton at center `(3, 3)`. -/
def stockBlitz7 : Board :=
  mkBoard 7
    (pRowR 0 [0, 2, 3, 4, 6] ++
     pRowA 1 [1, 5] ++ pRowR 1 [2, 4] ++
     pRowA 2 [0, 6] ++
     pRowR 3 [0, 6] ++
     pRowA 4 [0, 6] ++
     pRowA 5 [1, 5] ++ pRowR 5 [2, 4] ++
     pRowR 6 [0, 2, 3, 4, 6])
    ⟨3, 3⟩

/-- 9×9 Tactical opening board: symmetric layout with automaton at center `(4, 4)`. -/
def stockTactical9 : Board :=
  mkBoard 9
    (pRowR 0 [0, 1, 3, 4, 5, 7, 8] ++
     pRowA 1 [2, 6] ++ pRowR 1 [3, 4, 5] ++
     pRowA 3 [0, 1, 7, 8] ++
     pRowR 4 [0, 1, 7, 8] ++
     pRowA 5 [0, 1, 7, 8] ++
     pRowA 7 [2, 6] ++ pRowR 7 [3, 4, 5] ++
     pRowR 8 [0, 1, 3, 4, 5, 7, 8])
    ⟨4, 4⟩

/-- 11×11 Classic opening board (`stockTwoPlayer`). -/
def stockClassic11 : Board := stockTwoPlayer

/-- Board for a given `BoardPreset`. -/
def presetBoard : BoardPreset → Board
  | .blitz7    => stockBlitz7
  | .tactical9 => stockTactical9
  | .classic11 => stockClassic11

/-- Two-player corner goals for a given `BoardPreset`. -/
def presetGoals2 (p : BoardPreset) : GoalAssignment :=
  stockGoals2 (presetBoard p).size

/-- Four-player corner goals for a given `BoardPreset`. -/
def presetGoals4 (p : BoardPreset) : GoalAssignment :=
  stockGoals4 (presetBoard p).size

theorem presetBoard_classic11_eq :
    presetBoard .classic11 = stockTwoPlayer := rfl

theorem presetBoard_automaton_inBounds (p : BoardPreset) :
    (presetBoard p).inBounds (presetBoard p).automaton := by
  cases p <;> decide

theorem blitz7_opening_freezes :
    (automatonStepCfg {} stockBlitz7).automaton = stockBlitz7.automaton := by native_decide

theorem tactical9_opening_freezes :
    (automatonStepCfg {} stockTactical9).automaton = stockTactical9.automaton := by native_decide

theorem blitz7_opening_adjudicates_draw :
    adjudicateCapped stockBlitz7.automaton (presetGoals2 .blitz7) = none := by native_decide

theorem tactical9_opening_adjudicates_draw :
    adjudicateCapped stockTactical9.automaton (presetGoals2 .tactical9) = none := by native_decide

/-! ## §2  Configurable Cycle & Walk Semantics (`twoCycleSwaps`) -/

/-- Whether a carrying source `c` vacates its square under `cfg`:
    - When `cfg.twoCycleSwaps = false`, 2-cycles are suppressed (`twoCyc E c = true ⇒ false`),
      matching canonical `leaves`.
    - When `cfg.twoCycleSwaps = true`, populated 2-cycles rotate (swap) just like `≥ 3`-cycles. -/
def leavesCfg (cfg : GameConfig) (E : Coord → Option Coord) (car : Coord → Bool) (sf : Nat) :
    Nat → Coord → Bool
  | 0,     _ => true
  | v + 1, c =>
    if !cfg.twoCycleSwaps && twoCyc E c then false
    else
      match stopWalk E car sf c with
      | none   => false
      | some d => if d == c then false else if car d then leavesCfg cfg E car sf v d else true

/-- Landing square of `c` under `cfg`. -/
def landOfCfg (cfg : GameConfig) (E : Coord → Option Coord) (car : Coord → Bool)
    (sf vf : Nat) (c : Coord) : Coord :=
  if leavesCfg cfg E car sf vf c then (stopWalk E car sf c).getD c else c

/-- Board-specialized landing map under `cfg`. -/
def landMapCfg (cfg : GameConfig) (b : Board) (ms : List Move) : Coord → Coord :=
  landOfCfg cfg (edgeMap b ms) (carAt b) (ms.length + 1) (ms.length + 1)

/-- Carrying sources that genuinely move under `cfg`. -/
def moversCfg (cfg : GameConfig) (b : Board) (ms : List Move) : List Coord :=
  moverList b ms (landMapCfg cfg b ms)

theorem leavesCfg_default (E : Coord → Option Coord) (car : Coord → Bool)
    (sf vf : Nat) (c : Coord) :
    leavesCfg {} E car sf vf c = leaves E car sf vf c := by
  induction vf generalizing c with
  | zero => rfl
  | succ v ih =>
    unfold leavesCfg leaves
    simp only [Bool.not_false, Bool.true_and]
    by_cases h2 : twoCyc E c = true
    · rw [if_pos h2, if_pos h2]
    · rw [if_neg h2, if_neg h2]
      cases hsw : stopWalk E car sf c with
      | none => rfl
      | some d =>
        dsimp only []
        rw [ih d]


theorem landOfCfg_default (E : Coord → Option Coord) (car : Coord → Bool)
    (sf vf : Nat) (c : Coord) :
    landOfCfg {} E car sf vf c = landOf E car sf vf c := by
  unfold landOfCfg landOf
  rw [leavesCfg_default]

theorem landMapCfg_default (b : Board) (ms : List Move) :
    landMapCfg {} b ms = landMap b ms := by
  funext c
  exact landOfCfg_default (edgeMap b ms) (carAt b) (ms.length + 1) (ms.length + 1) c

theorem moversCfg_default (b : Board) (ms : List Move) :
    moversCfg {} b ms = movers b ms := by
  unfold moversCfg movers
  rw [landMapCfg_default]

/-- Every mover under `cfg` carries a piece. -/
theorem carAt_of_moverCfg (cfg : GameConfig) (b : Board) (ms : List Move) (c : Coord)
    (h : c ∈ moversCfg cfg b ms) : carAt b c = true := by
  unfold moversCfg moverList at h
  rw [List.mem_filter] at h
  have h2 := h.2
  simp only [Bool.and_eq_true] at h2
  exact h2.1

/-! ## §3  Confluence Modes (`conflict`, `bunchBefore`, `annihilate`) -/

/-- Pre-move clash coordinates under `cfg`:
    - In `.conflict` mode, both source forks (`forkAt`) and direct destination collisions
      (`collideAt`) are pre-move clashes.
    - In `.bunchBefore` and `.annihilate` modes, only source forks (`forkAt`) are pre-move
      clashes; destination collisions and pathway merges are resolved during move application
      without re-entry. -/
def clashCoordsCfg (cfg : GameConfig) (b : Board) (ms : List Move) : List Coord :=
  match cfg.confluence with
  | .conflict    => clashCoords b ms
  | .bunchBefore => ((candidates ms).filter (fun c => forkAt ms c)).dedup
  | .annihilate  => ((candidates ms).filter (fun c => forkAt ms c)).dedup

theorem clashCoordsCfg_default (b : Board) (ms : List Move) :
    clashCoordsCfg {} b ms = clashCoords b ms := rfl

/-- Axial direction from `u` to `d`, if they share an axis and are distinct. -/
def axialDirOf (u d : Coord) : Option Dir :=
  if u.y == d.y then
    if u.x < d.x then some .xp
    else if d.x < u.x then some .xn
    else none
  else if u.x == d.x then
    if u.y < d.y then some .yp
    else if d.y < u.y then some .yn
    else none
  else none

/-- Step one cell short of `d` when arriving from `u` along an axial move `u → d`.
    If `u` and `d` are adjacent (distance ≤ 1) or not axial, returns `u`. -/
def stepOneShort (u d : Coord) : Coord :=
  match axialDirOf u d with
  | none      => u
  | some .xp  => if d.x > u.x + 1 then ⟨d.x - 1, d.y⟩ else u
  | some .xn  => if u.x > d.x + 1 then ⟨d.x + 1, d.y⟩ else u
  | some .yp  => if d.y > u.y + 1 then ⟨d.x, d.y - 1⟩ else u
  | some .yn  => if u.y > d.y + 1 then ⟨d.x, d.y + 1⟩ else u

/-- Interior coordinates along an axial edge `u → v`, ordered from 1 cell before `v`
    back toward `u` (exclusive of both `u` and `v`). -/
def reverseEdgeInterior (u v : Coord) : List Coord :=
  match axialDirOf u v with
  | none      => []
  | some .xp  => (List.range (v.x - u.x - 1)).map (fun k => ⟨v.x - 1 - k, u.y⟩)
  | some .xn  => (List.range (u.x - v.x - 1)).map (fun k => ⟨v.x + 1 + k, u.y⟩)
  | some .yp  => (List.range (v.y - u.y - 1)).map (fun k => ⟨u.x, v.y - 1 - k⟩)
  | some .yn  => (List.range (u.y - v.y - 1)).map (fun k => ⟨u.x, v.y + 1 + k⟩)

/-- Expand a chain vertex list `[v₀, v₁, ..., vₖ]` into the reverse sequence of all
    squares strictly before the final vertex `vₖ` along the path back to `v₀`. -/
def reverseChainBeforeEnd : List Coord → List Coord
  | []           => []
  | [_]          => []
  | u :: v :: vs => reverseChainBeforeEnd (v :: vs) ++ reverseEdgeInterior u v ++ [u]

/-- Trace the sequence of chain vertices `[c, v₁, ..., d]` along `E` from `c` up to `fuel` steps,
    stopping at the first carrying square after `c` or when `E` has no edge. -/
def traceChainVertices (E : Coord → Option Coord) (car : Coord → Bool) :
    Nat → Bool → Coord → List Coord
  | 0, _, c => [c]
  | fuel + 1, isStart, c =>
    if !isStart && car c then [c]
    else match E c with
      | none => [c]
      | some nxt => c :: traceChainVertices E car fuel false nxt

/-- Candidate stop squares for mover `s` targeting `d` in `bunchBefore` mode, ordered from
    furthest along the path (1 cell short of `d`) back to `s`:
    1. Includes all interior squares and intermediate chain vertices in reverse path order.
    2. Always ends with `[s]` as fallback so the mover is guaranteed a valid placement. -/
def bunchCandidates (b : Board) (ms : List Move) (s d : Coord) : List Coord :=
  let verts := traceChainVertices (edgeMap b ms) (carAt b) (ms.length + 1) true s
  let cands := (reverseChainBeforeEnd verts).filter (fun q => q != d && decide (b.inBounds q))
  if cands.any (· == s) then cands else cands ++ [s]

/-- Initial candidate trajectory for mover `s` in `bunchBefore` mode:
    starts with `d = rawLand s` if `d` is uncontested, in-bounds, and not a stayer;
    followed by `bunchCandidates b ms s d` back to `s`. -/
def initBunchCands (b : Board) (ms : List Move) (mvrs : List Coord)
    (rawLand : Coord → Coord) (s : Coord) : List Coord :=
  let d := rawLand s
  let isContested := (mvrs.filter (fun c => rawLand c == d)).length > 1
  let isStayer (q : Coord) : Bool := carAt b q && !(mvrs.any (· == q))
  let fallbacks := bunchCandidates b ms s d
  if !isContested && decide (b.inBounds d) && !isStayer d then
    d :: fallbacks
  else
    fallbacks

/-- Current proposed landing square for mover `s` given remaining candidate map `candFn`. -/
def currentBunchProp (candFn : Coord → List Coord) (s : Coord) : Coord :=
  (candFn s).head?.getD s

/-- Whether mover `s`'s current proposed square `d = currentBunchProp candFn s` is blocked
    (out of bounds, occupied by a stayer, in `forbidden`, or contested by another mover). -/
def bunchPropBlocked (b : Board) (mvrs : List Coord) (candFn : Coord → List Coord)
    (forbidden : List Coord) (s : Coord) : Bool :=
  let d := currentBunchProp candFn s
  let isStayer (q : Coord) : Bool := carAt b q && !(mvrs.any (· == q))
  d != s && (
    !decide (b.inBounds d)
    || isStayer d
    || forbidden.any (· == d)
    || (mvrs.filter (fun c => currentBunchProp candFn c == d)).length > 1
  )

/-- Synchronous reverse-path relaxation loop for `bunchBefore` mode:
    At each step, every mover whose current proposed square is blocked simultaneously backs up
    one cell along its candidate list toward its starting square `s`. Squares contested by ≥ 2
    movers arriving from outside are added to `forbidden` so merge points remain empty.
    Because every step depends only on membership and filter counts over `mvrs`, this resolution
    is permutation-invariant (fair) and handles multi-mover caterpillar chains behind collisions. -/
def relaxBunchLoop (b : Board) (mvrs : List Coord) :
    Nat → (Coord → List Coord) → List Coord → (Coord → List Coord)
  | 0, candFn, _ => candFn
  | fuel + 1, candFn, forbidden =>
    if !(mvrs.any (bunchPropBlocked b mvrs candFn forbidden)) then
      candFn
    else
      let newContested :=
        (mvrs.map (currentBunchProp candFn)).filter (fun d =>
          (mvrs.filter (fun c => currentBunchProp candFn c == d && c != d)).length > 1)
      let forbidden' := forbidden ++ newContested
      let candFn' (s : Coord) : List Coord :=
        if bunchPropBlocked b mvrs candFn forbidden s then (candFn s).tail else candFn s
      relaxBunchLoop b mvrs fuel candFn' forbidden'

/-- Assign each mover a unique landing coordinate in `bunchBefore` mode via synchronous
    reverse-path relaxation. -/
def assignBunchLandings (b : Board) (ms : List Move) (mvrs : List Coord)
    (rawLand : Coord → Coord) : List (Coord × Coord) :=
  let initForbidden :=
    (mvrs.map rawLand).filter (fun d => (mvrs.filter (fun c => rawLand c == d)).length > 1)
  let fuel := (mvrs.length + 1) * (b.size + ms.length + 2)
  let candFnFinal := relaxBunchLoop b mvrs fuel (initBunchCands b ms mvrs rawLand) initForbidden
  mvrs.map (fun s => (s, currentBunchProp candFnFinal s))

/-- Raw effective landing map produced by synchronous `bunchBefore` relaxation. -/
def rawBunchLandMap (cfg : GameConfig) (b : Board) (ms : List Move) : Coord → Coord :=
  let mvrs := moversCfg cfg b ms
  let rawLand := landMapCfg cfg b ms
  let initForbidden :=
    (mvrs.map rawLand).filter (fun d => (mvrs.filter (fun c => rawLand c == d)).length > 1)
  let fuel := (mvrs.length + 1) * (b.size + ms.length + 2)
  let candFnFinal := relaxBunchLoop b mvrs fuel (initBunchCands b ms mvrs rawLand) initForbidden
  fun c => if memB mvrs c then currentBunchProp candFnFinal c else c

/-- Whether mover `c ∈ mvrs` has a bad effective landing under `L`:
    not a unique arrival among `mvrs`, lands on a stayer, or lands out of bounds. -/
def bunchLandBadB (b : Board) (mvrs : List Coord) (L : Coord → Coord) (c : Coord) : Bool :=
  !((mvrs.filter (fun c' => L c' == L c)).length == 1)
    || (carAt b (L c) && !memB mvrs (L c))
    || !(decide (b.inBounds (L c)))

/-- Whether all movers in `mvrs` have clean effective landings under `L`. -/
def bunchResolvableB (b : Board) (mvrs : List Coord) (L : Coord → Coord) : Bool :=
  (mvrs.filter (bunchLandBadB b mvrs L)).isEmpty

/-- The piece-conserving relabelling induced by `bunchBefore` resolution. -/
def bunchConservePhi (cfg : GameConfig) (b : Board) (ms : List Move) : Coord → Coord :=
  let mvrs := moversCfg cfg b ms
  let rawEff := rawBunchLandMap cfg b ms
  if bunchResolvableB b mvrs rawEff = true then rawEff else id

/-- Bad landing check parameterized by `mvrs` and `rawLand`. -/
def landBadCfg (cfg : GameConfig) (b : Board) (ms : List Move) (c : Coord) : Bool :=
  let mvrs := moversCfg cfg b ms
  let rawLand := landMapCfg cfg b ms
  !((mvrs.filter (fun c' => rawLand c' == rawLand c)).length == 1)
    || (carAt b (rawLand c) && (rawLand (rawLand c) == rawLand c))
    || !(decide (b.inBounds (rawLand c)))

theorem landBadCfg_default (b : Board) (ms : List Move) :
    landBadCfg {} b ms = landBad b ms := by
  funext c
  unfold landBadCfg landBad
  rw [moversCfg_default, landMapCfg_default]

/-- Unresolved landing coordinates under `cfg`:
    - In `.conflict` mode, any `landBadCfg` landing triggers conflict re-entry.
    - In `.bunchBefore` and `.annihilate` modes, merges are handled in `resolveMovesCfg`,
      so `unresolvedCfg` is `[]` (no re-entry for merges). -/
def unresolvedCfg (cfg : GameConfig) (b : Board) (ms : List Move) : List Coord :=
  match cfg.confluence with
  | .conflict =>
    (((moversCfg cfg b ms).filter (landBadCfg cfg b ms)).map (landMapCfg cfg b ms)).dedup
  | .bunchBefore => []
  | .annihilate  => []

theorem unresolvedCfg_default (b : Board) (ms : List Move) :
    unresolvedCfg {} b ms = unresolved b ms := by
  unfold unresolvedCfg unresolved
  rw [moversCfg_default, landBadCfg_default, landMapCfg_default]

/-- Resolve moves under `GameConfig`:
    - `.conflict`: when `unresolvedCfg` is empty, applies `writeBoard b mvrs rawLand`.
    - `.annihilate`: movers targeting a destination `d` with `≥ 2` movers targeting `d` are
      destroyed (vacate their source square and do not appear at `d`); unique movers land at `d`.
    - `.bunchBefore`: contested merge destinations `d` remain empty, and colliding movers stop
      short of `d` along their chain/ray per `rawBunchLandMap`. -/
def resolveMovesCfg (cfg : GameConfig) (b : Board) (ms : List Move) : Board :=
  let mvrs := moversCfg cfg b ms
  let rawLand := landMapCfg cfg b ms
  match cfg.confluence with
  | .conflict =>
    if (unresolvedCfg cfg b ms).isEmpty then
      writeBoard b mvrs rawLand
    else
      b
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
    { b with cells := fun q =>
        match arrivalAt survivors rawLand q with
        | some s => b.cellAt s
        | none   => if memB vacated q then Particle.vacuum else b.cellAt q }
  | .bunchBefore =>
    let rawEff := rawBunchLandMap cfg b ms
    if bunchResolvableB b mvrs rawEff then
      writeBoard b mvrs rawEff
    else
      b

theorem resolveMovesCfg_default (b : Board) (ms : List Move) :
    resolveMovesCfg {} b ms = resolveMoves b ms := by
  unfold resolveMovesCfg resolveMoves resolvableB
  rw [unresolvedCfg_default, moversCfg_default, landMapCfg_default]

/-! ### Formal Conservation Proof for `bunchBefore` (`resolveBunchBefore_conserves`) -/

theorem rawBunchLandMap_of_not_mover (cfg : GameConfig) (b : Board) (ms : List Move) (c : Coord)
    (hm : c ∉ moversCfg cfg b ms) :
    rawBunchLandMap cfg b ms c = c := by
  unfold rawBunchLandMap
  have hmem : memB (moversCfg cfg b ms) c = false := by
    cases h : memB (moversCfg cfg b ms) c with
    | false => rfl
    | true => exact absurd ((memB_iff _ _).mp h) hm
  rw [if_neg (by simp [hmem])]

theorem bunchLandBad_false_of_resolvable (b : Board) (M : List Coord) (L : Coord → Coord)
    (h : bunchResolvableB b M L = true) (c : Coord) (hc : c ∈ M) :
    bunchLandBadB b M L c = false := by
  unfold bunchResolvableB at h
  rw [List.isEmpty_iff, List.filter_eq_nil_iff] at h
  simpa using h c hc

theorem clean_bunch_land (b : Board) (M : List Coord) (L : Coord → Coord)
    (h : bunchResolvableB b M L = true) (c : Coord) (hc : c ∈ M) :
    (M.filter (fun c' => L c' == L c)).length = 1
    ∧ (carAt b (L c) = false ∨ memB M (L c) = true)
    ∧ b.inBounds (L c) := by
  have hb := bunchLandBad_false_of_resolvable b M L h c hc
  unfold bunchLandBadB at hb
  simp only [Bool.or_eq_false_iff, Bool.not_eq_false', beq_iff_eq, Bool.and_eq_false_iff,
    decide_eq_true_eq] at hb
  exact ⟨hb.1.1, hb.1.2, hb.2⟩

theorem filter_bunch_land_eq_singleton (b : Board) (M : List Coord) (L : Coord → Coord)
    (h : bunchResolvableB b M L = true) (c : Coord) (hc : c ∈ M) :
    M.filter (fun c' => L c' == L c) = [c] := by
  have hlen := (clean_bunch_land b M L h c hc).1
  have hmem : c ∈ M.filter (fun c' => L c' == L c) := by
    rw [List.mem_filter]; exact ⟨hc, by simp⟩
  cases hf : M.filter (fun c' => L c' == L c) with
  | nil => rw [hf] at hlen; simp at hlen
  | cons a t =>
    cases t with
    | nil =>
      rw [hf] at hmem
      simp only [List.mem_singleton] at hmem
      rw [hmem]
    | cons a' t' => rw [hf] at hlen; simp at hlen

/-- **⚑ `bunchBefore` PIECE CONSERVATION THEOREM**:
    In `.bunchBefore` confluence mode, `resolveMovesCfg` unconditionally conserves all pieces on
    the board via the bijection `bunchConservePhi`! -/
theorem resolveBunchBefore_conserves (cfg : GameConfig) (b : Board) (ms : List Move)
    (hcfg : cfg.confluence = .bunchBefore) :
    Conserves b (resolveMovesCfg cfg b ms) (bunchConservePhi cfg b ms) := by
  let mvrs := moversCfg cfg b ms
  let rawEff := rawBunchLandMap cfg b ms
  have hnot : ∀ c, c ∉ mvrs → rawEff c = c :=
    fun c hm => rawBunchLandMap_of_not_mover cfg b ms c hm
  by_cases hres : bunchResolvableB b mvrs rawEff = true
  · have hphi : bunchConservePhi cfg b ms = rawEff := by
      unfold bunchConservePhi
      exact if_pos hres
    have hbd : resolveMovesCfg cfg b ms = writeBoard b mvrs rawEff := by
      unfold resolveMovesCfg
      rw [hcfg]
      exact if_pos hres
    have harr : ∀ c ∈ mvrs, arrivalAt mvrs rawEff (rawEff c) = some c := by
      intro c hc
      unfold arrivalAt
      rw [filter_bunch_land_eq_singleton b mvrs rawEff hres c hc]
      rfl
    have hnoStayer : ∀ c ∈ mvrs, ∀ q, rawEff c = q → carAt b q = true → q ∈ mvrs := by
      intro c hc q hq hcar
      rcases (clean_bunch_land b mvrs rawEff hres c hc).2.1 with h' | h'
      · rw [hq] at h'; rw [h'] at hcar; exact absurd hcar (by simp)
      · rw [hq] at h'; exact (memB_iff mvrs q).mp h'
    have hnoArr : ∀ q, carAt b q = true → q ∉ mvrs → arrivalAt mvrs rawEff q = none := by
      intro q hcar hqm
      unfold arrivalAt
      have hnil : mvrs.filter (fun c => rawEff c == q) = [] := by
        rw [List.filter_eq_nil_iff]
        intro c hc hcq
        exact hqm (hnoStayer c hc q (by simpa using hcq) hcar)
      rw [hnil]; rfl
    have harrMem : ∀ q c, arrivalAt mvrs rawEff q = some c → c ∈ mvrs ∧ rawEff c = q := by
      intro q c hq
      unfold arrivalAt at hq
      have hsing : mvrs.filter (fun c' => rawEff c' == q) = [c] := by
        cases hfl : mvrs.filter (fun c' => rawEff c' == q) with
        | nil => rw [hfl] at hq; simp [uniqueOf] at hq
        | cons a t =>
          cases t with
          | nil =>
            rw [hfl] at hq
            simp only [uniqueOf] at hq
            injection hq with hq'
            rw [hq']
          | cons a' t' => rw [hfl] at hq; simp [uniqueOf] at hq
      have hmem : c ∈ mvrs.filter (fun c' => rawEff c' == q) := by
        rw [hsing]; exact List.mem_singleton_self c
      rw [List.mem_filter] at hmem
      exact ⟨hmem.1, by simpa using hmem.2⟩
    refine ⟨?_, ?_, ?_⟩
    · -- carried
      intro c hc
      have hcar : carAt b c = true := by unfold carAt; simp [hc]
      rw [hphi, hbd]
      by_cases hm : c ∈ mvrs
      · rw [writeBoard_cellAt b _ _ _ (clean_bunch_land b mvrs rawEff hres c hm).2.2, harr c hm]
      · rw [hnot c hm,
            writeBoard_cellAt b _ _ _ (inBounds_of_carrying b c hc),
            hnoArr c hcar hm]
        simp only
        rw [if_neg (by simp [memB_iff]; exact hm)]
    · -- injOn
      intro c₁ c₂ h1 h2 heq
      rw [hphi] at heq
      have hcar1 : carAt b c₁ = true := by unfold carAt; simp [h1]
      have hcar2 : carAt b c₂ = true := by unfold carAt; simp [h2]
      by_cases hm1 : c₁ ∈ mvrs <;> by_cases hm2 : c₂ ∈ mvrs
      · have hsing := filter_bunch_land_eq_singleton b mvrs rawEff hres c₁ hm1
        have hmem : c₂ ∈ mvrs.filter (fun c' => rawEff c' == rawEff c₁) := by
          rw [List.mem_filter]; exact ⟨hm2, by simp [heq]⟩
        rw [hsing] at hmem
        simp only [List.mem_singleton] at hmem
        exact hmem.symm
      · exfalso
        rw [hnot c₂ hm2] at heq
        exact hm2 (hnoStayer c₁ hm1 c₂ heq hcar2)
      · exfalso
        rw [hnot c₁ hm1] at heq
        exact hm1 (hnoStayer c₂ hm2 c₁ heq.symm hcar1)
      · rw [hnot c₁ hm1, hnot c₂ hm2] at heq
        exact heq
    · -- onto
      intro q hq
      rw [hbd] at hq
      have hib : b.inBounds q := inBounds_of_carrying (writeBoard b mvrs rawEff) q hq
      rw [writeBoard_cellAt b _ _ _ hib] at hq
      cases harrq : arrivalAt mvrs rawEff q with
      | some c =>
        rw [harrq] at hq
        obtain ⟨_, hcq⟩ := harrMem q c harrq
        exact ⟨c, hq, by rw [hphi]; exact hcq⟩
      | none =>
        rw [harrq] at hq
        simp only at hq
        by_cases hqm : memB mvrs q = true
        · rw [if_pos hqm] at hq; simp [Particle.isVacuum] at hq
        · rw [if_neg hqm] at hq
          refine ⟨q, hq, ?_⟩
          rw [hphi]
          exact hnot q (fun hmem => hqm ((memB_iff _ _).mpr hmem))
  · have hphi : bunchConservePhi cfg b ms = id := by
      unfold bunchConservePhi
      exact if_neg hres
    have hbd : resolveMovesCfg cfg b ms = b := by
      unfold resolveMovesCfg
      rw [hcfg]
      exact if_neg hres
    rw [hphi, hbd]
    exact ⟨fun _ _ => rfl, fun _ _ _ _ h => h, fun q hq => ⟨q, hq, rfl⟩⟩

/-! ## §4  Momentum Tie-Break (with Last Move Direction) & Railgun Fling -/

/-- Compute the direction of an automaton step from `oldPos` to `newPos`. -/
def stepDir? (oldPos newPos : Coord) : Option Dir :=
  axialDirOf oldPos newPos

/-- Equal-priority tie-break aware of the automaton's previous movement direction `lastDir`:
    When `tb = .momentum preferSameAxis`:
    - If `lastDir` was along X (`.xp` or `.xn`), prefers X when `preferSameAxis = true` and Y when `false`.
    - If `lastDir` was along Y (`.yp` or `.yn`), prefers Y when `preferSameAxis = true` and X when `false`.
    - If `lastDir = none`, falls back to `chooseOffsetCfg xDec yDec tb`. -/
def chooseOffsetMomentum (xDec yDec : Decision) (tb : TieBreak) (lastDir : Option Dir) : Int × Int :=
  match decisionCmp xDec yDec, tb, lastDir with
  | .gt, _, _ => xDec.delta (1, 0)
  | .lt, _, _ => yDec.delta (0, 1)
  | .eq, .momentum preferSameAxis, some d =>
    let lastWasX := d == .xp || d == .xn
    let pickX := if preferSameAxis then lastWasX else !lastWasX
    if pickX then xDec.delta (1, 0) else yDec.delta (0, 1)
  | _, _, _ => chooseOffsetCfg xDec yDec tb

theorem chooseOffsetMomentum_none (xDec yDec : Decision) (tb : TieBreak) :
    chooseOffsetMomentum xDec yDec tb none = chooseOffsetCfg xDec yDec tb := by
  unfold chooseOffsetMomentum chooseOffsetCfg
  cases decisionCmp xDec yDec <;> cases tb <;> rfl

/-- Slide the automaton repeatedly along offset `off` up to `fuel` steps while the next cell
    is in bounds and vacuum (Railgun Fling mode). -/
def flingLoop (off : Int × Int) : Nat → Board → Board
  | 0, b => b
  | fuel + 1, b =>
    if 0 ≤ (b.automaton.x : Int) + off.1 ∧ (b.automaton.x : Int) + off.1 < b.size
        ∧ 0 ≤ (b.automaton.y : Int) + off.2 ∧ (b.automaton.y : Int) + off.2 < b.size
        ∧ (off.1 ≠ 0 ∨ off.2 ≠ 0)
        ∧ b.cellAt ⟨((b.automaton.x : Int) + off.1).toNat,
                    ((b.automaton.y : Int) + off.2).toNat⟩ = .vacuum then
      flingLoop off fuel
        (stepTo b ⟨((b.automaton.x : Int) + off.1).toNat,
                   ((b.automaton.y : Int) + off.2).toNat⟩)
    else b

theorem flingLoop_size (off : Int × Int) (fuel : Nat) (b : Board) :
    (flingLoop off fuel b).size = b.size := by
  induction fuel generalizing b with
  | zero => rfl
  | succ fuel ih =>
    unfold flingLoop
    split
    · rw [ih]; rfl
    · rfl

/-- Full variant automaton step supporting `momentum` tie-break and `railgunFling`. -/
def automatonStepVariant (cfg : GameConfig) (b : Board) (lastDir : Option Dir := none) : Board :=
  let off := chooseOffsetMomentum
    (evaluateAxis (b.raycast b.automaton .xp) (b.raycast b.automaton .xn))
    (evaluateAxis (b.raycast b.automaton .yp) (b.raycast b.automaton .yn))
    cfg.tieBreak lastDir
  if cfg.railgunFling then
    flingLoop off b.size b
  else
    if 0 ≤ (b.automaton.x : Int) + off.1 ∧ (b.automaton.x : Int) + off.1 < b.size
        ∧ 0 ≤ (b.automaton.y : Int) + off.2 ∧ (b.automaton.y : Int) + off.2 < b.size
        ∧ (off.1 ≠ 0 ∨ off.2 ≠ 0)
        ∧ b.cellAt ⟨((b.automaton.x : Int) + off.1).toNat,
                    ((b.automaton.y : Int) + off.2).toNat⟩ = .vacuum then
      stepTo b ⟨((b.automaton.x : Int) + off.1).toNat, ((b.automaton.y : Int) + off.2).toNat⟩
    else b

theorem automatonStepVariant_default (b : Board) :
    automatonStepVariant {} b none = automatonStepCfg {} b := by
  unfold automatonStepVariant automatonStepCfg automatonOffsetCfg
  simp only [chooseOffsetMomentum_none]
  rfl

theorem automatonStepVariant_default_eq (b : Board) :
    automatonStepVariant {} b none = automatonStepCfg {} b :=
  automatonStepVariant_default b

/-! ## §5  Cooldown Turns (`cooldownTurns`) -/

/-- Active cooldowns: pairs of `(coord, remainingTurns)`. -/
abbrev CooldownMap := List (Coord × Nat)

/-- Decrement all active cooldowns at the end of a turn, dropping expired entries. -/
def tickCooldowns (cds : CooldownMap) : CooldownMap :=
  cds.filterMap (fun (c, t) => if t > 1 then some (c, t - 1) else none)

/-- Record new cooldowns of duration `k` on all squares where movers landed this turn. -/
def recordCooldowns (k : Nat) (cds : CooldownMap) (landed : List Coord) : CooldownMap :=
  let ticked := tickCooldowns cds
  if k == 0 then ticked
  else
    let fresh := landed.dedup.map (fun c => (c, k))
    (ticked.filter (fun (c, _) => !(landed.any (· == c)))) ++ fresh

/-- Check whether coordinate `c` is currently on cooldown. -/
def onCooldownB (cds : CooldownMap) (c : Coord) : Bool :=
  cds.any (fun (q, t) => q == c && t > 0)

/-- Variant move legality checking both `moveLegalB` and cooldowns. -/
def moveLegalVariantB (b : Board) (marks : List Coord) (cds : CooldownMap) (m : Move) : Bool :=
  moveLegalB b marks m && !onCooldownB cds m.frm

/-- Variant intermediate board after resolving legal moves under `cfg`. -/
def resolveMidVariant (cfg : GameConfig) (b : Board) (marks : List Coord)
    (cds : CooldownMap) (ms : List Move) : Board :=
  resolveMovesCfg cfg b (ms.filter (moveLegalVariantB b marks cds))

theorem resolveMidVariant_conflict_default_eq (b : Board) (marks : List Coord) (ms : List Move) :
    resolveMidVariant {} b marks [] ms = resolveMoves b (ms.filter (moveLegalB b marks)) := by
  unfold resolveMidVariant
  rw [resolveMovesCfg_default]
  have hleg : moveLegalVariantB b marks [] = moveLegalB b marks := by
    funext m
    simp [moveLegalVariantB, onCooldownB]
  rw [hleg]

/-- Squares where moving pieces actually landed under `cfg`. -/
def landedCoordsCfg (cfg : GameConfig) (b : Board) (ms : List Move) : List Coord :=
  let mvrs := moversCfg cfg b ms
  let rawLand := landMapCfg cfg b ms
  match cfg.confluence with
  | .conflict =>
    if (unresolvedCfg cfg b ms).isEmpty then mvrs.map rawLand else []
  | .annihilate =>
    let isStayer (q : Coord) : Bool := carAt b q && !(mvrs.any (· == q))
    let survivors := mvrs.filter (fun c =>
      let d := rawLand c
      (mvrs.filter (fun c' => rawLand c' == d)).length == 1
      && !isStayer d
      && decide (b.inBounds d))
    survivors.map rawLand
  | .bunchBefore =>
    let effLand := bunchConservePhi cfg b ms
    (mvrs.filter (fun c => effLand c != c)).map effLand

/-! ## §6  Configurable Round Step (`roundStepVariant`) -/

/-- Extended round state carrying variant fields (`cooldowns`, `lastDir`) alongside `RoundState`. -/
structure VariantRoundState extends RoundState where
  cooldowns : CooldownMap := []
  lastDir   : Option Dir  := none

/-- Outcome of a variant round step. -/
inductive VariantRoundOutcome where
  | again    (rs : VariantRoundState)
  | resolved (b : Board) (winner : Option Pid) (cooldowns : CooldownMap) (lastDir : Option Dir)

/-- Step the automaton `k` times (`automataCount`), stopping early if a goal is entered. -/
def stepAutomataLoop (cfg : GameConfig) (g : GoalAssignment) :
    Nat → Board → Option Dir → Board × Option Pid × Option Dir
  | 0, b, d => (b, none, d)
  | k + 1, b, d =>
    let nxtB := automatonStepVariant cfg b d
    let w := winOnEntry b nxtB g
    let nxtD := match stepDir? b.automaton nxtB.automaton with
      | some dir => some dir
      | none     => d
    match w with
    | some _ => (nxtB, w, nxtD)
    | none   => stepAutomataLoop cfg g k nxtB nxtD

/-- Step the automaton once (or `automataCount` times when `> 1`). -/
def stepAutomataCfg (cfg : GameConfig) (g : GoalAssignment) (mid : Board)
    (lastDir : Option Dir) : Board × Option Pid × Option Dir :=
  if cfg.automataCount ≤ 1 then
    let nxtB := automatonStepVariant cfg mid lastDir
    let w := winOnEntry mid nxtB g
    let nxtD := match stepDir? mid.automaton nxtB.automaton with
      | some dir => some dir
      | none     => lastDir
    (nxtB, w, nxtD)
  else
    stepAutomataLoop cfg g cfg.automataCount mid lastDir

theorem stepAutomataCfg_default_fst (g : GoalAssignment) (mid : Board) :
    (stepAutomataCfg {} g mid none).1 = automatonStepCfg {} mid := by
  unfold stepAutomataCfg
  rw [if_pos (Nat.le_refl 1), automatonStepVariant_default]

theorem stepAutomataCfg_default_snd (g : GoalAssignment) (mid : Board) :
    (stepAutomataCfg {} g mid none).2.1 = winOnEntry mid (automatonStepCfg {} mid) g := by
  unfold stepAutomataCfg
  rw [if_pos (Nat.le_refl 1), automatonStepVariant_default]

/-- One round of Move Entry under full `GameConfig`:
    - Filters `subs` by `moveLegalVariantB` (including cooldown check).
    - Checks pre-move clashes via `clashCoordsCfg` (fork+collide for `.conflict`, fork-only for
      `.bunchBefore` and `.annihilate`).
    - Checks post-walk clashes via `unresolvedCfg` (only in `.conflict` mode).
    - Applies `resolveMovesCfg`, steps the automaton via `stepAutomataCfg`, checks `winOnEntry`,
      and updates cooldowns. -/
def roundStepVariant (cfg : GameConfig) (g : GoalAssignment)
    (vrs : VariantRoundState) (subs : List Move) : VariantRoundOutcome :=
  let rs := vrs.toRoundState
  let fresh := subs.filter (fun m =>
    rs.waiting.contains m.who && moveLegalVariantB rs.board rs.marks vrs.cooldowns m)
  let all := rs.locked ++ fresh
  let clash := clashCoordsCfg cfg rs.board all
  let cs := if clash.isEmpty then unresolvedCfg cfg rs.board all else clash
  if cs.isEmpty then
    let mid := resolveMovesCfg cfg rs.board all
    let res := stepAutomataCfg cfg g mid vrs.lastDir
    let landed := landedCoordsCfg cfg rs.board all
    let nextCds := recordCooldowns cfg.cooldownTurns vrs.cooldowns landed
    .resolved res.1 res.2.1 nextCds res.2.2
  else
    .again
      { board     := rs.board
        marks     := (rs.marks ++ cs).dedup
        locked    := all.filter (fun m => !(cs.contains m.frm || cs.contains m.to))
        waiting   := ((all.filter (fun m => cs.contains m.frm || cs.contains m.to)).map (·.who)).dedup
        cooldowns := vrs.cooldowns
        lastDir   := vrs.lastDir }

/-- Projection of `VariantRoundOutcome` to canonical `RoundOutcome`. -/
def VariantRoundOutcome.toRoundOutcome : VariantRoundOutcome → RoundOutcome
  | .again vrs => .again vrs.toRoundState
  | .resolved b w _ _ => .resolved b w

/-- **Default Config Bridge Theorem**: At default `GameConfig {}` with empty cooldowns and
    `lastDir = none`, `roundStepVariant` agrees with canonical `roundStep`! -/
theorem roundStepVariant_default (g : GoalAssignment) (rs : RoundState) (subs : List Move) :
    (roundStepVariant {} g { rs with cooldowns := [], lastDir := none } subs).toRoundOutcome =
      roundStep {} g rs subs := by
  unfold roundStepVariant roundStep
  have hleg : (fun m => rs.waiting.contains m.who && moveLegalVariantB rs.board rs.marks [] m) =
      (fun m => rs.waiting.contains m.who && moveLegalB rs.board rs.marks m) := by
    funext m
    simp [moveLegalVariantB, onCooldownB]
  simp only [hleg, clashCoordsCfg_default, unresolvedCfg_default, resolveMovesCfg_default]
  generalize
    (if (clashCoords rs.board
          (rs.locked ++ List.filter (fun m => rs.waiting.contains m.who && moveLegalB rs.board rs.marks m) subs)).isEmpty
     then unresolved rs.board
          (rs.locked ++ List.filter (fun m => rs.waiting.contains m.who && moveLegalB rs.board rs.marks m) subs)
     else clashCoords rs.board
          (rs.locked ++ List.filter (fun m => rs.waiting.contains m.who && moveLegalB rs.board rs.marks m) subs)) = cs
  by_cases h : cs.isEmpty = true
  · rw [if_pos h, if_pos h]
    dsimp only [VariantRoundOutcome.toRoundOutcome]
    rw [stepAutomataCfg_default_fst, stepAutomataCfg_default_snd]
  · rw [if_neg h, if_neg h]
    rfl

theorem roundStepVariant_default_eq (g : GoalAssignment) (rs : RoundState) (subs : List Move) :
    (roundStepVariant {} g { rs with cooldowns := [], lastDir := none } subs).toRoundOutcome =
      roundStep {} g rs subs :=
  roundStepVariant_default g rs subs

#assert_all_clean [
  presetBoard_classic11_eq,
  presetBoard_automaton_inBounds,
  leavesCfg_default,
  landOfCfg_default,
  landMapCfg_default,
  moversCfg_default,
  clashCoordsCfg_default,
  landBadCfg_default,
  unresolvedCfg_default,
  resolveMovesCfg_default,
  resolveBunchBefore_conserves,
  chooseOffsetMomentum_none,
  flingLoop_size,
  automatonStepVariant_default,
  automatonStepVariant_default_eq,
  resolveMidVariant_conflict_default_eq,
  roundStepVariant_default,
  roundStepVariant_default_eq
]

end Dregg2.Games.AutomataflRules
