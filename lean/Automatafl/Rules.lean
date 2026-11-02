import Automatafl.Base
import Automatafl.Tactics
import Automatafl.ListAux

open Automatafl.Tactics


/-!
# AutomataflRules — the automatafl spec REWRITTEN AGAINST THE RULESET

⚑ SUBSTRATE. This is a **Lean-authored game spec**. Nothing here is a Rust AIR, and the
AIR/gadget layer over it stays Lean-authored (`Dregg2.Circuit.Emit.Automatafl*`). The
existing hand-written Rust in `dregg-automatafl/` is DEBT and is not extended by this file.

## Why this file exists

`Dregg2.Games.Automatafl` was written by mirroring `logic/src/game.rs::apply_moves` and was
guarded by a differential test pointed at a second copy of the same code. The audit
(`docs/reference/AUTOMATAFL-RULES-CONFORMANCE-AUDIT.md`) read it against the Creator-Approved
ruleset and found nine divergences — six outcome-changing, three that DESTROY PIECES, the
smallest firing with one move on a 3x3 board. This file is the rules-faithful replacement.

**Ground truth**, in precedence order:

1. `~/dev/automatafl/logic/README.md` §"Game Rules" — the Creator-Approved ruleset.
2. `~/dev/automatafl/old_python_prototype/model.py` — the ACTUAL GAME per the author
   (`Game.ev_Move` / `Resolve` / `CompleteMoves` / `ClearState`, `Board.CanMove` / `Move` /
   `AgentStep`).
3. `logic/MOVE_EXPLAIN.md`, `logic/PHILOSOPHY.md` for intent.
4. `logic/src/*.rs` as corroboration only.

## The author's four rulings, implemented here

* **(A) Conflict re-entry is modelled in full.** `Board → List Move → Board` is the WRONG
  TYPE for a turn. A turn is N ROUNDS (§7): submit → detect → on conflict the involved
  players' moves are dropped and THEY re-enter, every non-conflicted player is LOCKED, the
  conflicted COORDINATE is MARKED and becomes illegal as source AND destination FOR ALL, and
  the round recurses. Only a clean round resolves; then the automaton steps; then the win is
  checked. Markers, locks and pending moves die at turn end (`model.py::ClearState`).
* **(B) The column rule is SELECTABLE** (`GameConfig.tieBreak`, §5): `Column` (default,
  = `automaton.rs`, prefers the Y axis) | `Row` (= `model.py::AgentStep`, prefers X) |
  `Freeze` (no move on an equal-priority tie). See §5 for why the two implementations'
  disagreement is only apparent.
* **(C) 2-cycles STAY PUT** (§4), the README's empty-square-back-to-a-source case stays put,
  and empty cycles cannot capture a piece.
* **(D) The automaton square is banned as a move SOURCE ONLY** (§2). Naming it as a
  DESTINATION is legal to propose and simply FAILS to execute — the square is occupied, so
  the inclusive path check (§4) blocks the move and the piece is replaced at its origin.

## What else changed vs. `Automatafl.lean`

* **3.2, the worst one.** The path check is INCLUSIVE OF THE DESTINATION
  (`model.py::CanMove` scans `range(min, max+1)` on both coordinates, exempting sources via
  `PC_F_PASSABLE`). A non-moving piece standing on the destination BLOCKS, and the mover is
  replaced at its origin. The old `interior`-only check let the mover overwrite it.
* **3.7, determinism.** `journeys.find?` awarded a shared landing square by MOVE-LIST ORDER
  and deleted the loser. Here a contested landing is a **CONFLICT** (§3, `unresolved`) —
  the `DetectAndConflict` reading of "the conflict rules are the weakest precondition that
  structurally ensures that following move resolution has a deterministic result". Resolution
  only runs on a round whose landings are uniquely claimed, so `resolve_perm` is a THEOREM.
* **Setup and win** (§6) existed only in hand-written Rust. Two-player = two corners in the
  same row each; four-player = one corner each; and the win fires on the automaton **moving
  into** a corner, not sitting on one.

## What was KEPT because it conforms

The identical-move exception, the caterpillar erratum (a piece landing on another move's
source participates in that move too; cycles permissible), >2-cycle rotation, and THE ENTIRE
AUTOMATON — all four priorities, both equidistant-removals, all empty-space guards. §5 reuses
`evaluateAxis` / `decisionCmp` / `Decision.delta` / `stepTo` from `Automatafl.lean`
UNCHANGED; only the equal-priority tie-break is rewired to `GameConfig`.

## The two keystones

* `resolve_conserves` (§8) — **UNCONDITIONAL** piece conservation: a bijection between the
  occupied squares before and after resolution. The old `applyMoves_conserves_pieces` had to
  assume `hlandA`/`hlandB` ("neither target square holds a piece that is not one of the two
  movers") — that assumption WAS rules clause 3.2, and implementing 3.2 discharges it.
* `resolve_perm` (§9) — **PROVEN** permutation-invariance. The old `FairnessObligation` was
  not merely unproven, it was FALSE (audit D5); this is the theorem it should have been.
-/

namespace Dregg2.Games.AutomataflRules

open Dregg2.Games.Automatafl

/-! ## §1  Game configuration -/

/-- The equal-priority tie-break, README: *"some people have suggested having the Automaton
'freeze' for that step altogether, and thus this is a selectable preference in every game"*.
`column` prefers the Y axis (`automaton.rs`, the deployed default); `row` prefers the X axis
(`model.py::AgentStep`, which ties to `colpri` — that file calls the X axis "columns" because
`self.columns` is indexed by x). -/
inductive ConfluenceMode
  | conflict      -- Canonical: mark conflicted square & re-enter

  | bunchBefore   -- Stop 1 cell short of collision without re-entry (MERGE_RESOLUTION_DESIGN.md)
  | annihilate    -- Colliding movers destroy each other
deriving DecidableEq, Repr, Inhabited

/-- Board size presets: 7×7 Blitz, 9×9 Tactical, 11×11 Classic. -/
inductive BoardPreset
  | blitz7
  | tactical9
  | classic11
deriving DecidableEq, Repr, Inhabited

inductive TieBreak
  | column
  | row
  | freeze
  | momentum (preferSameAxis : Bool)
deriving DecidableEq, Repr, Inhabited

/-- Per-game selectable preferences and variant configuration. -/
structure GameConfig where
  tieBreak      : TieBreak       := .column
  confluence    : ConfluenceMode := .conflict
  twoCycleSwaps : Bool           := false
  cooldownTurns : Nat            := 0
  railgunFling  : Bool           := false
  automataCount : Nat            := 1
  preset        : BoardPreset    := .classic11
deriving DecidableEq, Repr, Inhabited


/-! ## §2  Move legality

README, Move Entry Phase: a move is two coordinates, source ≠ destination, sharing an axis
(a rook move on an empty board), both in bounds. Plus:

* **(D)** `model.py::ev_Move` rejects `POS_CANT_MOVE_THAT` when the SOURCE is the agent, and
  says nothing about the destination. A move TARGETING the automaton is proposable and simply
  fails at resolution (`CanMove` finds a non-empty, non-passable square). `game.rs` bans both
  endpoints; the prototype and the README are the authority, so only the source is banned.
* The conflicted-coordinate clause is LIVE here: `marks` comes from the enclosing
  `RoundState` (§7), so it is genuinely set during a turn. (In `Automatafl.lean` the twin
  clause read `Board.conflictAt`, which was never set true anywhere in the tree.) -/

/-- Move legality against a board and the round's conflict markers. -/
def MoveLegal (b : Board) (marks : List Coord) (m : Move) : Prop :=
  m.frm ≠ m.to
  ∧ (m.frm.x = m.to.x ∨ m.frm.y = m.to.y)
  ∧ b.inBounds m.frm ∧ b.inBounds m.to
  ∧ ¬ b.isAutomaton m.frm
  ∧ m.frm ∉ marks ∧ m.to ∉ marks

instance moveLegal_decidable (b : Board) (marks : List Coord) (m : Move) :
    Decidable (MoveLegal b marks m) := by
  unfold MoveLegal; exact inferInstance

/-- The Bool twin. -/
def moveLegalB (b : Board) (marks : List Coord) (m : Move) : Bool :=
  decide (MoveLegal b marks m)

theorem moveLegalB_iff (b : Board) (marks : List Coord) (m : Move) :
    moveLegalB b marks m = true ↔ MoveLegal b marks m := by
  unfold moveLegalB; exact decide_eq_true_iff

/-! ## §3  Conflicts

README: *"a conflict occurs if multiple players specify the same source; or multiple players
specify the same destination with a non-vacuum source. Except two players specifying an
identical (same sources, same targets) move is not a conflict."*

Both detectors compare a PAIR of moves and require the pair to differ in the field that is
not shared — so an identical `(src,dst)` submitted twice can never fire either one. That is
the exception, by construction (and it is `model.py`'s `if pending_move in seen_moves:
continue`, expressed without an order-dependent `seen` set).

The third clause, `unresolved` (§4), is the merge/confluence conflict. -/

/-- Fork: ≥2 moves out of `s` to different destinations. -/
def forkAt (ms : List Move) (s : Coord) : Bool :=
  ms.any (fun m₁ => ms.any (fun m₂ => m₁.frm == s && m₂.frm == s && m₁.to != m₂.to))

/-- Collision: ≥2 moves into `d` from different NON-VACUUM sources. -/
def collideAt (b : Board) (ms : List Move) (d : Coord) : Bool :=
  ms.any (fun m₁ => ms.any (fun m₂ =>
    m₁.to == d && m₂.to == d && m₁.frm != m₂.frm
      && !(b.cellAt m₁.frm).isVacuum && !(b.cellAt m₂.frm).isVacuum))

/-- Every coordinate any move names — the only places a fork/collide can live. -/
def candidates (ms : List Move) : List Coord := (ms.map (·.frm)) ++ (ms.map (·.to))

/-- The fork/collide conflicted coordinates. -/
def clashCoords (b : Board) (ms : List Move) : List Coord :=
  ((candidates ms).filter (fun c => forkAt ms c || collideAt b ms c)).dedup

/-! ## §4  Resolution

Rules step 3, and `model.py::CompleteMoves` — which fires a move exactly when its source is
occupied, its destination is empty, and `CanMove` finds the INCLUSIVE rectangle clear of
non-passable pieces (all sources are `PC_F_PASSABLE`). That loop is order-dependent; the
conflict clauses exist precisely to make the order not matter, and §9 proves it here.

The order-free rendering: the non-blocked moves form a graph with out-degree ≤ 1 on sources
(a fork is a conflict), so each piece has a forward orbit. A piece advances along its orbit
to the first square strictly ahead that either carries a piece at turn start (the caterpillar
— that piece consumed the edge out of its own square, so we stop there) or dead-ends. It
advances only if that square actually empties. -/

/-- The straight-line path INCLUSIVE OF THE DESTINATION (`model.py::CanMove` scans
`range(min, max+1)` on both coordinates). This is the fix for audit divergence 3.2. -/
def pathCells (frm dst : Coord) : List Coord := interior frm dst ++ [dst]

/-- A move is blocked iff some cell on its path — DESTINATION INCLUDED — holds a piece that
is not itself a moving source. Sources are passable "in the process of being moved"
(`mark_passable`, `PC_F_PASSABLE`) whether or not their own move ends up executing.

A blocked move contributes NO EDGE, so its piece is replaced at its origin: the author's
*"designating a move to an occupied square is fine, it just fails to execute, it doesn't
generate a conflict and shouldn't."* -/
def blockedB (b : Board) (ms : List Move) (m : Move) : Bool :=
  (pathCells m.frm m.to).any
    (fun c => !(b.cellAt c).isVacuum && !(ms.any (fun m' => m'.frm == c)))

/-- "All these are the same coordinate" — `none` on an empty list AND on a fork. Written so
it depends on the list only through its MEMBERS (`allEqOpt_spec`), which is what makes the
whole resolution permutation-invariant (§9). -/
def allEqOpt (l : List Coord) : Option Coord :=
  match l.head? with
  | none   => none
  | some d => if l.all (fun e => e == d) then some d else none

/-- The move graph: the unique unblocked destination out of `c`, if there is one. -/
def edgeOf (b : Board) (ms : List Move) (c : Coord) : Option Coord :=
  allEqOpt ((ms.filter (fun m => m.frm == c && !blockedB b ms m)).map (·.to))

/-- Does `c` carry a piece at turn start? (The automaton counts — it occludes, it is never a
source, and no move may land on it.) -/
def carAt (b : Board) (c : Coord) : Bool := !(b.cellAt c).isVacuum

/-- `c`'s move goes to a square whose move comes straight back — a 2-cycle.
PHILOSOPHY.md: *"2-cycles (A→B, B→A): **Always** stay in place — unambiguous composition"*;
MERGE_RESOLUTION_DESIGN "Fixed behavior (not configurable)"; `game.rs` `is_two_cycle ⇒
dest_coord = start_coord`. `Automatafl.lean` SWAPPED them, which is audit divergence 3.5a,
and it also covers the README's own named case — *"a move from an empty square directly back
to some source square — the piece simply doesn't move"* (3.5b): there the far square is
vacuum, and the pair is still a 2-cycle. -/
def twoCyc (E : Coord → Option Coord) (c : Coord) : Bool :=
  match E c with
  | none   => false
  | some d => (E d == some c) && (d != c)

/-- Walk forward from `c`: the square a piece leaving `c` would come to rest on — the first
square STRICTLY AHEAD that carries a piece at turn start, or the first dead end.
`none` = the walk never terminates, i.e. it entered a cycle of squares that were ALL empty at
turn start; MOVE_EXPLAIN §4 *"An empty cycle cannot 'pull' a new piece into it. The move is
nullified"* (audit divergence 3.5c). -/
def stopWalk (E : Coord → Option Coord) (car : Coord → Bool) : Nat → Coord → Option Coord
  | 0,     _ => none
  | f + 1, c =>
    match E c with
    | none   => some c
    | some d => if car d then some d else stopWalk E car f d

/-- Does the piece standing on `c` leave its square? It does not if `c` is in a 2-cycle, if
its walk enters an empty cycle, if its walk returns to `c` itself, or if the square it would
come to rest on is held by a piece that does not itself leave (recursively — that is the
author's "fails to execute", propagated back down the chain).

Running out of `vf` means the chain of "waiting on the piece ahead" is longer than the number
of moves, i.e. it is a rotation cycle, and on a cycle every edge fires. -/
def leaves (E : Coord → Option Coord) (car : Coord → Bool) (sf : Nat) :
    Nat → Coord → Bool
  | 0,     _ => true
  | v + 1, c =>
    if twoCyc E c then false
    else
      match stopWalk E car sf c with
      | none   => false
      | some d => if d == c then false else if car d then leaves E car sf v d else true

/-- Where the piece on `c` ends the resolution. -/
def landOf (E : Coord → Option Coord) (car : Coord → Bool) (sf vf : Nat) (c : Coord) : Coord :=
  if leaves E car sf vf c then (stopWalk E car sf c).getD c else c

/-- The move graph of a round. -/
def edgeMap (b : Board) (ms : List Move) : Coord → Option Coord := edgeOf b ms

/-- The landing map of a round. Fuel `ms.length + 1` for both walks: a walk longer than the
number of moves has repeated an edge. -/
def landMap (b : Board) (ms : List Move) : Coord → Coord :=
  landOf (edgeMap b ms) (carAt b) (ms.length + 1) (ms.length + 1)

/-- The squares whose piece actually moves. `dedup` renders the identical-move exception at
the resolution layer: two players naming the same `(src,dst)` contribute ONE mover. -/
def moverList (b : Board) (ms : List Move) (L : Coord → Coord) : List Coord :=
  ((ms.map (·.frm)).dedup).filter (fun c => carAt b c && L c != c)

/-- The movers of a round. -/
def movers (b : Board) (ms : List Move) : List Coord := moverList b ms (landMap b ms)

/-- The single element of a list, or `none`. Depends on the list only up to permutation
(`uniqueOf_spec`). -/
def uniqueOf : List Coord → Option Coord
  | [c] => some c
  | _   => none

/-- Who arrives on `q`, if exactly one mover does. -/
def arrivalAt (M : List Coord) (L : Coord → Coord) (q : Coord) : Option Coord :=
  uniqueOf (M.filter (fun c => L c == q))

/-- Bool membership written through `any`, so permutation-invariance is one lemma. -/
def memB (M : List Coord) (q : Coord) : Bool := M.any (fun x => x == q)

theorem memB_iff (M : List Coord) (q : Coord) : memB M q = true ↔ q ∈ M := by
  unfold memB
  simp [List.any_eq_true]

/-- A mover whose landing is not cleanly its own. Three ways, all decidable:

⚑ **THIS IS A WELL-DEFINEDNESS GUARD, NOT A GAME RULE** (ember ruled 2026-07-24). The README's
conflict list has exactly two entries — same source, and same destination with a non-vacuum
source (plus the identical-move exception). The clause below is a THIRD condition that the
README does not state, so read it as what it is: the predicate that makes a round's landings a
PARTIAL INJECTION onto empty-or-emptying squares, which is precisely what makes `resolve_perm`
provable. It is derived from README §"the weakest precondition that structurally ensures that
following move resolution has a deterministic result", not invented beside the rules.

**It is PROVABLY VACUOUS at the deployed game.** `resolvableB_pair` (below) proves a legal pair
with distinct sources and distinct destinations is ALWAYS resolvable, so at m = 2 — and the
deployed automatafl IS two-player (`stockGoals2`, `reference.rs::stock_two_player`; >2 players
was never firmed up as a ruleset) — `unresolved` is empty by theorem and this guard never fires.
Triggering it takes ≥ 3 movers in one round. So it costs nothing in play; it exists so that a
hypothetical wider round CONFLICTS instead of corrupting the board.

**Its failure mode is the point.** Every arm degrades to "this round is a conflict", never to a
lost or overwritten piece — contrast `~/dev/automatafl/logic/`, whose `DetectAndConflict` arm
falls through placing nothing and ANNIHILATES both pieces on exactly this configuration
(`detect_merging_pathways` was never implemented), and `fastlogic`, which resolves it
order-dependently. Both of those are experiments, not canonical.

1. **the merge/confluence clause** — another mover claims the same landing square. This is
   audit divergence 3.7. `Automatafl.lean` resolved it by `journeys.find?`, i.e. by MOVE-LIST
   ORDER, and DELETED the loser. Here it is a conflict.
2. the landing square holds a piece that does not itself leave. `leaves` is written to
   prevent this; the check is kept so that resolution can never be reached in a state where a
   piece would be overwritten. It is a CHECKED side condition, not a proven property of
   `leaves` — a labelled residual, and its failure mode is a conflict, never a lost piece.
3. the landing is off the board. Unreachable through `roundStep` (which filters for
   legality); kept for the same reason. -/
def landBad (b : Board) (ms : List Move) (c : Coord) : Bool :=
  !(((movers b ms).filter (fun c' => landMap b ms c' == landMap b ms c)).length == 1)
    || (carAt b (landMap b ms c) && (landMap b ms (landMap b ms c) == landMap b ms c))
    || !(decide (b.inBounds (landMap b ms c)))

/-- The contested coordinates: the landing squares that are not cleanly claimed. Empty ⟺ the
round's landings are a partial injection onto empty-or-emptying squares. -/
def unresolved (b : Board) (ms : List Move) : List Coord :=
  (((movers b ms).filter (landBad b ms)).map (landMap b ms)).dedup

/-- Is this round's resolution well-defined? -/
def resolvableB (b : Board) (ms : List Move) : Bool := (unresolved b ms).isEmpty

/-- Write the landings onto the board. Every occupied square is either a mover's origin
(vacated unless something arrives) or untouched. -/
def writeBoard (b : Board) (M : List Coord) (L : Coord → Coord) : Board :=
  { b with
    cells := fun q =>
      match arrivalAt M L q with
      | some c => b.cellAt c
      | none   => if memB M q then .vacuum else b.cellAt q }

/-- **Resolution.** Rules step 3, in full. Guarded by `resolvableB`, which `roundStep` also
uses to decide whether the round resolves at all — so the guard is not a fiction, it is the
same predicate that turns an ambiguous round into a conflict. -/
def resolveMoves (b : Board) (ms : List Move) : Board :=
  if resolvableB b ms then writeBoard b (movers b ms) (landMap b ms) else b

/-! ## §5  The automaton step — UNCHANGED, with the tie-break wired to `GameConfig`

`evaluateAxis`'s nine-case table, its four empty-space guards, its two equidistant-removals
and its distance tie-break order were checked clause-by-clause against README Priorities 1–4
and conform on every one. They are reused here verbatim from `Dregg2.Games.Automatafl`; the
ONLY change is the equal-priority branch.

Note the `(repulsor, repulsor)` arm's empty-space guard is IMPLIED, not missing: distances are
≥ 1, so `pos.dist ≠ neg.dist` forces `max ≥ 2` and the flight direction has room. Do not
"fix" it into a bug.

**On the two implementations disagreeing about which axis is "the column".** `automaton.rs`
breaks equal-priority ties along Y; `model.py::AgentStep` breaks them along X. They are the
same game: `model.py`'s `DEFAULT_SETUP` is indexed `[x][y]` and `board.rs`'s `arr2` is indexed
`[y][x]` (`Coord::ix` = `(y, x)`), so the two stock boards are TRANSPOSES of each other, and
transposing swaps the axes. The author has ruled the preference selectable with `Column`
(= Y, `automaton.rs`) as the default, and §6's `stockTwoPlayer` uses the `board.rs`
orientation to match. -/

/-- The equal-priority branch, per `GameConfig`. Everything else is `Automatafl.chooseOffset`. -/
def chooseOffsetCfg (xDec yDec : Decision) (tb : TieBreak) : Int × Int :=
  match decisionCmp xDec yDec, tb with
  | .gt, _       => xDec.delta (1, 0)
  | .lt, _       => yDec.delta (0, 1)
  | .eq, .column => yDec.delta (0, 1)
  | .eq, .row    => xDec.delta (1, 0)
  | .eq, .freeze => (0, 0)
  | .eq, .momentum true  => yDec.delta (0, 1)
  | .eq, .momentum false => xDec.delta (1, 0)


/-- The automaton's offset. -/
def automatonOffsetCfg (b : Board) (tb : TieBreak) : Int × Int :=
  chooseOffsetCfg
    (evaluateAxis (b.raycast b.automaton .xp) (b.raycast b.automaton .xn))
    (evaluateAxis (b.raycast b.automaton .yp) (b.raycast b.automaton .yn))
    tb

/-- The offset is one of the five cardinal offsets (including zero). -/
theorem chooseOffsetCfg_mem (x y : Decision) (tb : TieBreak) :
    chooseOffsetCfg x y tb = (1, 0) ∨ chooseOffsetCfg x y tb = (-1, 0)
      ∨ chooseOffsetCfg x y tb = (0, 1) ∨ chooseOffsetCfg x y tb = (0, -1)
      ∨ chooseOffsetCfg x y tb = (0, 0) := by
  unfold chooseOffsetCfg
  split <;>
    first
      | exact Decision.delta_mem _ _ (Or.inl rfl)
      | exact Decision.delta_mem _ _ (Or.inr rfl)
      | exact Or.inr (Or.inr (Or.inr (Or.inr rfl)))

/-- **The automaton moves AT MOST one step in a cardinal direction**, for every tie-break. -/
theorem automatonOffsetCfg_bounded (b : Board) (tb : TieBreak) :
    (automatonOffsetCfg b tb).1.natAbs + (automatonOffsetCfg b tb).2.natAbs ≤ 1 := by
  rcases chooseOffsetCfg_mem
      (evaluateAxis (b.raycast b.automaton .xp) (b.raycast b.automaton .xn))
      (evaluateAxis (b.raycast b.automaton .yp) (b.raycast b.automaton .yn)) tb with
    h | h | h | h | h <;> (unfold automatonOffsetCfg; rw [h]) <;> decide

/-- The automaton step: move onto the one-step target iff it is in bounds, a genuine move,
and vacuum ("the Automaton can never move into an occupied square"). -/
def automatonStepCfg (cfg : GameConfig) (b : Board) : Board :=
  let off := automatonOffsetCfg b cfg.tieBreak
  if 0 ≤ (b.automaton.x : Int) + off.1 ∧ (b.automaton.x : Int) + off.1 < b.size
      ∧ 0 ≤ (b.automaton.y : Int) + off.2 ∧ (b.automaton.y : Int) + off.2 < b.size
      ∧ (off.1 ≠ 0 ∨ off.2 ≠ 0)
      ∧ b.cellAt ⟨((b.automaton.x : Int) + off.1).toNat,
                  ((b.automaton.y : Int) + off.2).toNat⟩ = .vacuum then
    stepTo b ⟨((b.automaton.x : Int) + off.1).toNat, ((b.automaton.y : Int) + off.2).toNat⟩
  else b

/-! ### The Leg-A migration bridge

At the DEFAULT tie-break the new automaton is the OLD automaton, arm for arm. So the ~13.8k
lines of Leg A (`AutomataflStepRefine`, `StepBackend`, `StepCapstone`, `StepChoose`,
`StepEmit`, `StepCoord`) re-point by rewriting with these three lemmas — nothing in the
nine-case `evaluateAxis` table, the raycast congruences or the arithmetization moves. A game
that ships `.row` or `.freeze` needs a regenerated `automatafl-step.json`; `.column` does not. -/

theorem chooseOffsetCfg_column (x y : Decision) :
    chooseOffsetCfg x y .column = chooseOffset x y true := by
  unfold chooseOffsetCfg chooseOffset
  cases decisionCmp x y <;> rfl

theorem automatonOffsetCfg_column (b : Board) (hb : b.useColumnRule = true) :
    automatonOffsetCfg b .column = automatonOffset b := by
  unfold automatonOffsetCfg automatonOffset
  rw [hb, chooseOffsetCfg_column]

theorem automatonStepCfg_size (cfg : GameConfig) (b : Board) :
    (automatonStepCfg cfg b).size = b.size := by
  simp only [automatonStepCfg]; split <;> rfl

/-- **The Leg-A STEP bridge at the default tie-break.** The third bridge lemma (alongside
`chooseOffsetCfg_column` / `automatonOffsetCfg_column`): at `.column`, on a board whose
`useColumnRule = true`, the NEW automaton step IS the OLD one, arm for arm — the only difference
between `automatonStepCfg .column` and `automatonStep` is the offset function, and that is equated
by `automatonOffsetCfg_column`. So the ~13.8k-line Leg-A capstone re-points onto `automatonStepCfg`
by a single rewrite, with NO descriptor re-emit for the deployed default game. -/
theorem automatonStepCfg_column_eq (cfg : GameConfig) (b : Board)
    (hcfg : cfg.tieBreak = .column) (hb : b.useColumnRule = true) :
    automatonStepCfg cfg b = automatonStep b := by
  have hoff : automatonOffsetCfg b cfg.tieBreak = automatonOffset b := by
    rw [hcfg]; exact automatonOffsetCfg_column b hb
  simp only [automatonStepCfg, automatonStep, hoff]

/-- The automaton stays in bounds across its step. -/
theorem automatonStepCfg_preserves_inBounds (cfg : GameConfig) (b : Board)
    (hb : b.inBounds b.automaton) :
    (automatonStepCfg cfg b).inBounds (automatonStepCfg cfg b).automaton := by
  unfold Board.inBounds
  rw [automatonStepCfg_size]
  simp only [automatonStepCfg]
  split
  · rename_i h
    obtain ⟨_, _, _, _, _, _⟩ := h
    exact ⟨by simp only [stepTo]; omega, by simp only [stepTo]; omega⟩
  · exact hb

/-! ## §6  Setup and the win condition

README, Initial Setup: *"In a two-player game, each player picks two corners that are in the
same row. In a four-player game, each player picks exactly one corner."*
Win: *"When the Automaton **moves into** a corner, the game is won by whomever owns the
corner."*

`Automatafl.lean` had none of this — no layout, no corners, no goal well-formedness — and its
`winner` tested SITS ON, so its own witness reported a win on a board where the automaton had
never moved. -/

/-- The four corners of an `n × n` board. -/
def cornersOf (size : Nat) : List Coord :=
  [⟨0, 0⟩, ⟨size - 1, 0⟩, ⟨0, size - 1⟩, ⟨size - 1, size - 1⟩]

/-- A goal assignment: which seat owns which corner. -/
structure GoalAssignment where
  entries : List (Coord × Pid)
deriving DecidableEq, Repr

/-- Every goal is a corner. -/
def GoalAssignment.onCorners (g : GoalAssignment) (size : Nat) : Bool :=
  g.entries.all (fun e => (cornersOf size).contains e.1)

/-- Every corner is assigned. -/
def GoalAssignment.coversCorners (g : GoalAssignment) (size : Nat) : Bool :=
  (cornersOf size).all (fun c => g.entries.any (fun e => e.1 == c))

/-- No square is assigned twice. -/
def GoalAssignment.squaresDistinct (g : GoalAssignment) : Bool :=
  (g.entries.map (·.1)).Nodup

/-- The seats holding at least one corner. -/
def GoalAssignment.seats (g : GoalAssignment) : List Pid := (g.entries.map (·.2)).dedup

/-- Each seat's corners share a row (equal `y`). -/
def GoalAssignment.seatsShareRow (g : GoalAssignment) : Bool :=
  g.entries.all (fun e₁ => g.entries.all (fun e₂ => e₁.2 != e₂.2 || e₁.1.y == e₂.1.y))

/-- Each seat holds exactly `k` corners. -/
def GoalAssignment.eachSeatHolds (g : GoalAssignment) (k : Nat) : Bool :=
  g.seats.all (fun p => (g.entries.filter (fun e => e.2 == p)).length == k)

/-- **Two-player setup**: exactly two seats, two corners each, each pair sharing a row. -/
def GoalAssignment.WellFormed2 (g : GoalAssignment) (size : Nat) : Bool :=
  g.onCorners size && g.coversCorners size && g.squaresDistinct
    && g.seats.length == 2 && g.eachSeatHolds 2 && g.seatsShareRow

/-- **Four-player setup**: exactly four seats, one corner each. -/
def GoalAssignment.WellFormed4 (g : GoalAssignment) (size : Nat) : Bool :=
  g.onCorners size && g.coversCorners size && g.squaresDistinct
    && g.seats.length == 4 && g.eachSeatHolds 1

/-- The stock two-player assignment: seat 0 takes the `y = 0` row, seat 1 the `y = size-1`
row. (`reference.rs::GOAL_CORNERS_2P` agrees. `model.py::DEFAULT_GOALS[2]` does NOT — it reads
`[[(0,0),(10,0)], [(10,0),(10,10)]]`, repeating `(10,0)` and giving seat 1 a COLUMN. That is
a prototype bug; the README is the authority.) -/
def stockGoals2 (size : Nat) : GoalAssignment :=
  ⟨[(⟨0, 0⟩, 0), (⟨size - 1, 0⟩, 0), (⟨0, size - 1⟩, 1), (⟨size - 1, size - 1⟩, 1)]⟩

/-- The stock four-player assignment: one corner each, counter-clockwise. -/
def stockGoals4 (size : Nat) : GoalAssignment :=
  ⟨[(⟨0, 0⟩, 0), (⟨size - 1, 0⟩, 1), (⟨size - 1, size - 1⟩, 2), (⟨0, size - 1⟩, 3)]⟩

/-- **The deployed game's seat list is a CLOSED FORM: `[0, 1]`, at EVERY board size.**

`GoalAssignment.seats` is `(entries.map (·.2)).dedup`, and `stockGoals2`'s seat components are the
literals `0, 0, 1, 1` at every `size` — `size` enters only the corner COORDINATES, never a seat. So
"who is seated" in the stock two-player game is DECIDABLE, not a hypothesis: no `size ≥ 2`, no `n =
11` instance needed. Holds definitionally (`rfl`). -/
theorem stockGoals2_seats (size : Nat) : (stockGoals2 size).seats = [0, 1] := rfl

/-- **Seat 0 is seated in the deployed game** — for every `size`. This is the fact the circuit
capstones carried as the hypothesis `hseat0 : seats.contains 0 = true`; at `stockGoals2` it is a
THEOREM, so the deployed-instance capstones assume nothing about who is seated. -/
theorem stockGoals2_seats_contains_zero (size : Nat) :
    (stockGoals2 size).seats.contains 0 = true := rfl

/-- Seat 1 likewise — the two-player game seats EXACTLY `{0, 1}`. -/
theorem stockGoals2_seats_contains_one (size : Nat) :
    (stockGoals2 size).seats.contains 1 = true := rfl

/-- The win check: the automaton **moved**, and its new square is a declared goal. -/
def winOnEntry (before after : Board) (g : GoalAssignment) : Option Pid :=
  if after.automaton = before.automaton then none
  else winnerAux after.automaton g.entries

/-- **Win soundness.** A win means the automaton genuinely MOVED and genuinely landed on a
declared goal of the winner. -/
theorem winOnEntry_sound (before after : Board) (g : GoalAssignment) (p : Pid)
    (h : winOnEntry before after g = some p) :
    after.automaton ≠ before.automaton ∧ (after.automaton, p) ∈ g.entries := by
  unfold winOnEntry at h
  by_cases hm : after.automaton = before.automaton
  · rw [if_pos hm] at h; exact absurd h (by simp)
  · rw [if_neg hm] at h
    refine ⟨hm, ?_⟩
    have : ∀ (l : List (Coord × Pid)), winnerAux after.automaton l = some p →
        (after.automaton, p) ∈ l := by
      intro l
      induction l with
      | nil => intro hn; simp [winnerAux] at hn
      | cons e es ih =>
        obtain ⟨c, q⟩ := e
        simp only [winnerAux]
        by_cases hc : c = after.automaton
        · rw [if_pos hc]; intro hq; injection hq with hq; subst hq; subst hc
          exact List.mem_cons_self
        · rw [if_neg hc]; intro hq; exact List.mem_cons_of_mem _ (ih hq)
    exact this _ h

/-- **A win is a CORNER**, once the assignment is on corners. This is the statement
`Automatafl.lean`'s `winner_sound` could not make, because cornerhood was not in the model. -/
theorem winOnEntry_corner (before after : Board) (g : GoalAssignment) (size : Nat) (p : Pid)
    (hg : g.onCorners size = true) (h : winOnEntry before after g = some p) :
    after.automaton ∈ cornersOf size := by
  obtain ⟨_, hmem⟩ := winOnEntry_sound before after g p h
  unfold GoalAssignment.onCorners at hg
  have := List.all_eq_true.mp hg _ hmem
  simpa using this

/-! ### The stock 11×11 opening

Transcribed from `board.rs::stock_two_player` (identical to `model.py::DEFAULT_SETUP` up to
the transpose discussed in §5; the `board.rs` orientation is used, so the default `Column`
tie-break is the prototype's play). Corners hold repulsors: the automaton can never move into
an occupied square, so a corner must be cleared before it can be won. -/

private def rowR (y : Nat) (xs : List Nat) : List (Coord × Particle) :=
  xs.map (fun x => (⟨x, y⟩, Particle.repulsor))

private def rowA (y : Nat) (xs : List Nat) : List (Coord × Particle) :=
  xs.map (fun x => (⟨x, y⟩, Particle.attractor))

/-- The stock two-player 11×11 board, automaton centred at (5,5). -/
def stockTwoPlayer : Board :=
  mkBoard 11
    (rowR 0 [0, 1, 4, 5, 6, 9, 10] ++
     rowA 1 [3, 7] ++ rowR 1 [4, 5, 6] ++
     rowA 4 [0, 1, 9, 10] ++
     rowR 5 [0, 1, 9, 10] ++
     rowA 6 [0, 1, 9, 10] ++
     rowA 9 [3, 7] ++ rowR 9 [4, 5, 6] ++
     rowR 10 [0, 1, 4, 5, 6, 9, 10])
    ⟨5, 5⟩

/-! ## §6B  ⚑ ADJUDICATION AT THE TURN CAP — the terminal rule the ruleset never had

**The design wound.** `winOnEntry` is the game's ONLY terminal condition, so a match that never
walks the Automaton into a corner never ends. It does not end. That much is a fact about the
rules and is why this section exists.

⚠ **RETRACTED — the numbers that used to stand here.** This section previously reported that
"two competent seats draw 100% of the time", froze in 8–11 turns, and had "30 of 198" useful
moves, from a one-ply simultaneous-MINIMAX agent. That claim was withdrawn the same day in
`docs/CLAIM-CORRECTIONS-2026-07-25.md` §4 and the retraction is the correct reading: two
DETERMINISTIC agents on a MIRROR-SYMMETRIC board play mirror-symmetric games BY CONSTRUCTION, so
the freeze was a property of the agents, not of automatafl. In a simultaneous-move game optimal
play is generally MIXED, and a deterministic agent is not a weak player but the wrong kind of
object. The harness was never committed and the numbers are not reproducible from this tree.

**Re-measured 2026-07-26, with an instrument that survives that critique.** Both seats sample a
MIXED-strategy equilibrium of the one-round matrix game (solved by LP), so symmetry no longer
forces a lockstep line. Over 38 matches across two independent policies (sampled Nash; and pure
maximin with a uniform random tie-break, which has no solver-vertex bias at all):

  * draws **17–21%**, not 100%;
  * outright corner wins **12–14%** — `winOnEntry` is LIVE, and a cooperative beam search reaches
    a corner in **10 turns**, so the win is not a dead affordance;
  * and the standing finding: **64–71% of matches are decided by this section's own cap rule**
    rather than by the game's win condition.

  ⚑ What DOES hold up, and is the real structural point: the value of a round is **0.000** at
  every position sampled, every position has a PURE saddle (no seat is ever forced to randomise),
  and a mean of **39 of 190** candidate moves are tied EXACTLY for best. The game is not frozen —
  it is FLAT. That is a different diagnosis from the retracted one and it wants a different fix.

**What the deployed game already did about it, unproven.** `dregg-automatafl/src/surface.rs`
carries `const MAX_TURNS: u64 = 64` and calls the capped match *a draw* — a terminal rule that
exists in the Rust surface and NOWHERE in this ruleset. So the shipped game's most common
outcome is decided by a constant no theorem here has ever seen.

**What this section adds.** The cap's adjudication, in the ruleset, as a POSITIONAL decision
rather than a blanket draw: at the cap the seat whose nearest own goal is strictly nearer the
Automaton takes the match; exact parity is an honest dead heat. It does not touch `winOnEntry`,
`roundStep` or the automaton, so every existing theorem and the emitted step descriptor are
untouched. Re-measured, it leaves only 17–21% of matches drawn.

⚑ **THE ARITHMETIC OF THIS RULE AT `stockGoals2`, which is worth stating plainly.** The two
distances are `min(x, 10-x) + y` and `min(x, 10-x) + (10-y)`, so their difference is `2y - 10`:
**the x term cancels, and `adjudicateCapped` is exactly `sign(5 - y)` on all 121 squares**
(`adjudicate_midline_draws` is the `y = 5` slice of that identity). Two consequences:

  * the Automaton's COLUMN is worth nothing to the capped verdict, and an exhaustive sweep of all
    `33^4 = 1,185,921` ray configurations puts **46.9%** of its steps on that verdict-irrelevant
    axis (it is frozen in only 4.4%, so it is not a metronome — it is loud and half-useless);
  * the two `goalDistance` numbers a surface paints are only meaningful as a DIFFERENCE. 220 of
    the 440 single steps that move them at all move BOTH by the same amount.

⚠ The residual, restated honestly: the mid-line still draws, by the board's own `y ↦ 10 - y`
symmetry. The withdrawn recommendation was "an asymmetric opening"; ember's ruling stands that
automatafl needs no such thing, and it is NOT proposed here. The measured diagnosis is instead
FLATNESS — a round is worth 0.000 and dozens of moves tie exactly — which points at giving the
game a second currency rather than at breaking its symmetry. -/

/-- Manhattan distance between two board coordinates. -/
def manhattan (a b : Coord) : Nat :=
  (max a.x b.x - min a.x b.x) + (max a.y b.y - min a.y b.y)

@[simp] theorem manhattan_self (a : Coord) : manhattan a a = 0 := by
  simp only [manhattan, max_self, min_self, Nat.sub_self, Nat.add_zero]

/-- The distances from `a` to each goal a seat owns. -/
def goalDists (a : Coord) (g : GoalAssignment) (p : Pid) : List Nat :=
  (g.entries.filter (fun e => e.2 == p)).map (fun e => manhattan a e.1)

/-- The distance to a seat's NEAREST goal; `none` when the seat owns no goal. -/
def goalDistance (a : Coord) (g : GoalAssignment) (p : Pid) : Option Nat :=
  (goalDists a g p).min?

/-- **`adjudicateCapped` — the capped match's terminal rule** for the two-seat game: the seat strictly
nearer its own goal wins; a seat that owns goals beats one that owns none; exact parity draws. -/
def adjudicateCapped (a : Coord) (g : GoalAssignment) : Option Pid :=
  match goalDistance a g 0, goalDistance a g 1 with
  | some d0, some d1 => if d0 < d1 then some 0 else if d1 < d0 then some 1 else none
  | some _, none     => some 0
  | none,   some _   => some 1
  | none,   none     => none

/-- **`adjudicate_seated` (soundness — a winner OWNS a goal).** The adjudicated winner is always a
seat with at least one goal corner; the rule cannot award a match to an unseated player. -/
theorem adjudicate_seated (a : Coord) (g : GoalAssignment) (p : Pid)
    (h : adjudicateCapped a g = some p) : ∃ d, goalDistance a g p = some d := by
  unfold adjudicateCapped at h
  cases h0 : goalDistance a g 0 <;> cases h1 : goalDistance a g 1 <;>
    rw [h0, h1] at h <;> simp only at h
  · exact absurd h (by simp)
  · rw [Option.some.injEq] at h; subst h; exact ⟨_, h1⟩
  · rw [Option.some.injEq] at h; subst h; exact ⟨_, h0⟩
  · rename_i d0 d1
    split at h
    · rw [Option.some.injEq] at h; subst h; exact ⟨d0, h0⟩
    · split at h
      · rw [Option.some.injEq] at h; subst h; exact ⟨d1, h1⟩
      · exact absurd h (by simp)

/-- **`adjudicate_sound` (the winner is genuinely NEARER).** When both seats own goals, an
adjudicated winner is strictly closer to its own goal than the loser is to theirs. This is the
teeth: the rule never awards a capped match to the seat that was behind on the board. -/
theorem adjudicate_sound (a : Coord) (g : GoalAssignment) (p : Pid) (d0 d1 : Nat)
    (h : adjudicateCapped a g = some p)
    (h0 : goalDistance a g 0 = some d0) (h1 : goalDistance a g 1 = some d1) :
    (p = 0 ∧ d0 < d1) ∨ (p = 1 ∧ d1 < d0) := by
  unfold adjudicateCapped at h
  rw [h0, h1] at h
  simp only at h
  split at h
  · rename_i hlt; rw [Option.some.injEq] at h; subst h; exact Or.inl ⟨rfl, hlt⟩
  · split at h
    · rename_i hlt; rw [Option.some.injEq] at h; subst h; exact Or.inr ⟨rfl, hlt⟩
    · exact absurd h (by simp)

/-- **`adjudicate_draw_level` (a draw is a DEAD HEAT).** The rule returns `none` only when the two
seats are exactly level — same nearest-goal distance, or neither owning a goal at all. There is no
arm that draws a match one seat was winning. -/
theorem adjudicate_draw_level (a : Coord) (g : GoalAssignment) (h : adjudicateCapped a g = none) :
    goalDistance a g 0 = goalDistance a g 1 := by
  unfold adjudicateCapped at h
  cases h0 : goalDistance a g 0 <;> cases h1 : goalDistance a g 1 <;>
      rw [h0, h1] at h <;> simp only at h
  case none.some => exact absurd h (by simp)
  case some.none => exact absurd h (by simp)
  case some.some d0 d1 =>
    split at h
    · exact absurd h (by simp)
    · split at h
      · exact absurd h (by simp)
      · rename_i hge hle
        exact congrArg some (by omega)

/-- **`stock_opening_adjudicates_draw` (FAIRNESS of the setup, decided).** The stock 11×11 opening
puts the Automaton on `(5,5)`, which is exactly equidistant from seat 0's `y = 0` corners and seat
1's `y = 10` corners — so the adjudication does not hand either seat a free positional edge at
turn zero. The rule is a contest, not a thumb on the scale. -/
theorem stock_opening_adjudicates_draw :
    adjudicateCapped stockTwoPlayer.automaton (stockGoals2 11) = none := by decide

/-- **`adjudicate_decisive_witness` (NON-VACUITY, at a position the play harness actually froze
on).** The 5×-search asymmetric run parked the Automaton on `(10,2)` — on seat 0's goal COLUMN,
two squares from `(10,0)` — and seat 1 still held the draw under the shipped rules. Under
adjudication that same board is a seat-0 win. This is the measured line, not a toy. -/
theorem adjudicate_decisive_witness : adjudicateCapped ⟨10, 2⟩ (stockGoals2 11) = some 0 := by decide

/-- The mirrored witness: `(3,8)` is nearer seat 1's `y = 10` row, and adjudicates to seat 1. So
the rule is symmetric under the board's own `y ↦ 10 - y` symmetry. -/
theorem adjudicate_decisive_witness_mirror : adjudicateCapped ⟨3, 8⟩ (stockGoals2 11) = some 1 := by
  decide

/-- **`adjudicate_midline_draws` (the RESIDUAL, witnessed rather than hidden).** Every square on
the mid-line `y = 5` adjudicates to a draw, because the stock goal rows are symmetric about it.
This is exactly the equilibrium the play harness found competent seats parking on, and it is why
adjudication halves the draws rather than eliminating them. Stated as a theorem so the remaining
weakness is a CHECKED fact about the shipped rule, not a footnote. -/
theorem adjudicate_midline_draws :
    ∀ x : Fin 11, adjudicateCapped ⟨x.val, 5⟩ (stockGoals2 11) = none := by decide

/-! ## §7  THE ROUND — the honest type of a turn

`applyTurn : Board → List Move → Board` cannot express the rules. Per (A):

> In the event of a conflict, all players involved in the conflict must invalidate their
> previous move and prepare another move. It is illegal to specify as a source or destination
> the *exact* coordinate which was conflicted upon … this is often indicated with a temporary
> marker … After all involved players have prepared their respective moves, they are revealed
> simultaneously, and, if needed, the conflict resolution will recurse.

So a turn is a sequence of ROUNDS over a `RoundState`: the board (frozen for the whole turn —
nothing resolves until a clean round), the accumulated markers, the LOCKED moves that stand,
and the seats that must re-enter. `roundStep` is one round; `runTurn` folds a trace of
submissions. `ClearState` is structural: a fresh `RoundState` per turn carries no markers, no
locks and no pending moves. -/

/-- The mid-turn state. `board` is the TURN-START board; rounds never mutate it. -/
structure RoundState where
  board   : Board
  marks   : List Coord
  locked  : List Move
  waiting : List Pid

/-- The turn's opening round state. -/
def openRound (b : Board) (seats : List Pid) : RoundState :=
  { board := b, marks := [], locked := [], waiting := seats }

/-- A round either demands re-entry, or resolves the turn. -/
inductive RoundOutcome
  | again (rs : RoundState)
  | resolved (b : Board) (win : Option Pid)

/-- **ONE ROUND.**

1. Take the submissions from the seats that owe a move, keeping only the legal ones (a
   marked coordinate is illegal as source AND destination, for everyone — §2).
2. Add the locked moves. Detect fork/collide conflicts; if none, detect merge conflicts (§4).
3. Any conflicted coordinate is MARKED, every move naming it at EITHER endpoint is dropped
   and its seat re-enters, and every other move is LOCKED. The round recurses.
   (`Automatafl.lean`'s `conflictResolve` dropped a move only if *its own* source was
   fork-conflicted or *its own* destination collide-conflicted — a move merely MENTIONING a
   conflicted coordinate at the other endpoint survived and executed. Audit divergences
   2.4c / D4a / D4b.)
4. A clean round RESOLVES: markers are cleared (they live in `RoundState`, so this is
   structural), moves resolve, the automaton steps, the win is checked on ENTRY.

One deviation from `model.py`, in the README's favour: a previously-locked seat whose move is
newly conflicted re-enters. `model.py` deletes its pending move but never removes it from
`self.locked`, so it can no longer submit — a deadlock the README's "all players involved …
must prepare another move" does not permit. -/
def roundStep (cfg : GameConfig) (g : GoalAssignment) (rs : RoundState) (subs : List Move) :
    RoundOutcome :=
  let fresh := subs.filter (fun m => rs.waiting.contains m.who && moveLegalB rs.board rs.marks m)
  let all := rs.locked ++ fresh
  let clash := clashCoords rs.board all
  let cs := if clash.isEmpty then unresolved rs.board all else clash
  if cs.isEmpty then
    let mid := resolveMoves rs.board all
    let after := automatonStepCfg cfg mid
    .resolved after (winOnEntry mid after g)
  else
    .again
      { board := rs.board
        marks := (rs.marks ++ cs).dedup
        locked := all.filter (fun m => !(cs.contains m.frm || cs.contains m.to))
        waiting := ((all.filter (fun m => cs.contains m.frm || cs.contains m.to)).map (·.who)).dedup }

/-- A whole turn: fold a trace of per-round submissions. `none` = the trace ran out before a
clean round (the turn is still awaiting re-entry). -/
def runTurn (cfg : GameConfig) (g : GoalAssignment) (rs : RoundState) :
    List (List Move) → Option (Board × Option Pid)
  | []           => none
  | subs :: rest =>
    match roundStep cfg g rs subs with
    | .resolved b w => some (b, w)
    | .again rs'    => runTurn cfg g rs' rest

/-- Read-only projections, so `#guard` can inspect an outcome without needing `DecidableEq`
on `Board` (which carries function fields). -/
def RoundOutcome.isAgain : RoundOutcome → Bool
  | .again _ => true | .resolved _ _ => false

def RoundOutcome.marks : RoundOutcome → List Coord
  | .again rs => rs.marks | .resolved _ _ => []

def RoundOutcome.waiting : RoundOutcome → List Pid
  | .again rs => rs.waiting | .resolved _ _ => []

def RoundOutcome.locked : RoundOutcome → List Move
  | .again rs => rs.locked | .resolved _ _ => []

def RoundOutcome.cellAt : RoundOutcome → Coord → Option Particle
  | .again _ => fun _ => none | .resolved b _ => fun c => some (b.cellAt c)

def RoundOutcome.win : RoundOutcome → Option Pid
  | .again _ => none | .resolved _ w => w

/-! ## §8  ⚑ PIECE CONSERVATION — UNCONDITIONAL

Every piece on the board at turn start is on the board after resolution, exactly once, and
nothing appears from nowhere: `φ` is a bijection between the occupied squares.

No hypotheses. `Automatafl.lean`'s `applyMoves_conserves_pieces` carried `hlandA`/`hlandB`
("neither target square holds a piece that is not one of the two movers") and was stated only
at arity 2. Those hypotheses WERE rules clause 3.2; `blockedB`'s inclusive path check
discharges them, and the guard `resolvableB` — which is `roundStep`'s own conflict test —
supplies uniqueness of arrivals. -/

/-- A piece-preserving relabelling of the occupied squares. -/
structure Conserves (b b' : Board) (φ : Coord → Coord) : Prop where
  /-- every piece survives, at `φ` of where it was -/
  carried : ∀ c, (b.cellAt c).isVacuum = false → b'.cellAt (φ c) = b.cellAt c
  /-- no two pieces end up on the same square -/
  injOn : ∀ c₁ c₂, (b.cellAt c₁).isVacuum = false → (b.cellAt c₂).isVacuum = false →
            φ c₁ = φ c₂ → c₁ = c₂
  /-- nothing appears from nowhere -/
  onto : ∀ q, (b'.cellAt q).isVacuum = false → ∃ c, (b.cellAt c).isVacuum = false ∧ φ c = q

/-- The relabelling resolution induces. -/
def conservePhi (b : Board) (ms : List Move) : Coord → Coord :=
  if resolvableB b ms = true then landMap b ms else id

theorem cellAt_in (b : Board) (q : Coord) (h : b.inBounds q) : b.cellAt q = b.cells q := by
  unfold Board.cellAt; exact if_pos h

theorem inBounds_of_carrying (b : Board) (c : Coord) (h : (b.cellAt c).isVacuum = false) :
    b.inBounds c := by
  unfold Board.cellAt at h
  by_cases hb : c.x < b.size ∧ c.y < b.size
  · exact hb
  · rw [if_neg hb] at h; simp [Particle.isVacuum] at h

/-- `writeBoard` reads through at any in-bounds square. -/
theorem writeBoard_cellAt (b : Board) (M : List Coord) (L : Coord → Coord) (q : Coord)
    (hq : b.inBounds q) :
    (writeBoard b M L).cellAt q =
      (match arrivalAt M L q with
       | some c => b.cellAt c
       | none   => if memB M q then Particle.vacuum else b.cellAt q) := by
  rw [cellAt_in (writeBoard b M L) q hq]
  rfl

/-- A square nothing moves out of is a square whose landing is itself. -/
theorem landMap_of_not_src (b : Board) (ms : List Move) (c : Coord)
    (h : (ms.any (fun m => m.frm == c)) = false) : landMap b ms c = c := by
  have hE : edgeMap b ms c = none := by
    unfold edgeMap edgeOf
    have : (ms.filter (fun m => m.frm == c && !blockedB b ms m)) = [] := by
      rw [List.filter_eq_nil_iff]
      intro m hm
      have := (List.any_eq_false.mp h) m hm
      simp only [Bool.not_eq_true] at this ⊢
      simp [this]
    rw [this]; rfl
  unfold landMap landOf
  have hs : stopWalk (edgeMap b ms) (carAt b) (ms.length + 1) c = some c := by
    rw [stopWalk, hE]
  have hl : leaves (edgeMap b ms) (carAt b) (ms.length + 1) (ms.length + 1) c = false := by
    rw [leaves]
    have h2 : twoCyc (edgeMap b ms) c = false := by unfold twoCyc; rw [hE]
    rw [if_neg (by simp [h2]), hs]
    simp
  rw [if_neg (by simp [hl])]

/-- A carrying square that is not a mover keeps its landing. -/
theorem landMap_of_not_mover (b : Board) (ms : List Move) (c : Coord)
    (hcar : carAt b c = true) (hM : c ∉ movers b ms) : landMap b ms c = c := by
  by_cases hsrc : (ms.any (fun m => m.frm == c)) = true
  · by_contra hne
    apply hM
    unfold movers moverList
    rw [List.mem_filter]
    refine ⟨?_, by simp [hcar, hne]⟩
    rw [List.mem_dedup, List.mem_map]
    obtain ⟨m, hm, hmc⟩ := List.any_eq_true.mp hsrc
    exact ⟨m, hm, by simpa using hmc⟩
  · exact landMap_of_not_src b ms c (by simpa using hsrc)

/-- Every mover carries a piece. -/
theorem carAt_of_mover (b : Board) (ms : List Move) (c : Coord) (h : c ∈ movers b ms) :
    carAt b c = true := by
  unfold movers moverList at h
  rw [List.mem_filter] at h
  have h2 := h.2
  simp only [Bool.and_eq_true] at h2
  exact h2.1

section Guarded

variable (b : Board) (ms : List Move)

/-- Unpack `resolvableB`. -/
theorem landBad_false_of_resolvable (h : resolvableB b ms = true) (c : Coord)
    (hc : c ∈ movers b ms) : landBad b ms c = false := by
  unfold resolvableB unresolved at h
  rw [List.isEmpty_iff] at h
  have h1 : ((movers b ms).filter (landBad b ms)).map (landMap b ms) = [] := by
    simpa using h
  have h2 : (movers b ms).filter (landBad b ms) = [] := by simpa using h1
  rw [List.filter_eq_nil_iff] at h2
  simpa using h2 c hc

/-- The three clauses of a clean landing, unpacked. -/
theorem clean_land (h : resolvableB b ms = true) (c : Coord) (hc : c ∈ movers b ms) :
    ((movers b ms).filter (fun c' => landMap b ms c' == landMap b ms c)).length = 1
    ∧ (carAt b (landMap b ms c) = false
        ∨ landMap b ms (landMap b ms c) ≠ landMap b ms c)
    ∧ b.inBounds (landMap b ms c) := by
  have hb := landBad_false_of_resolvable b ms h c hc
  unfold landBad at hb
  simp only [Bool.or_eq_false_iff, Bool.not_eq_false', beq_iff_eq, Bool.and_eq_false_iff,
    beq_eq_false_iff_ne, decide_eq_true_eq] at hb
  exact ⟨hb.1.1, hb.1.2, hb.2⟩

/-- The unique-claim clause: a mover is the ONLY mover landing where it lands. -/
theorem filter_land_eq_singleton (h : resolvableB b ms = true) (c : Coord)
    (hc : c ∈ movers b ms) :
    (movers b ms).filter (fun c' => landMap b ms c' == landMap b ms c) = [c] := by
  have hlen := (clean_land b ms h c hc).1
  have hmem : c ∈ (movers b ms).filter (fun c' => landMap b ms c' == landMap b ms c) := by
    rw [List.mem_filter]; exact ⟨hc, by simp⟩
  cases hf : (movers b ms).filter (fun c' => landMap b ms c' == landMap b ms c) with
  | nil => rw [hf] at hlen; simp at hlen
  | cons a t =>
    cases t with
    | nil =>
      rw [hf] at hmem
      simp only [List.mem_singleton] at hmem
      rw [hmem]
    | cons a' t' => rw [hf] at hlen; simp at hlen

/-- The landing stays on the board. -/
theorem land_inBounds (h : resolvableB b ms = true) (c : Coord) (hc : c ∈ movers b ms) :
    b.inBounds (landMap b ms c) := (clean_land b ms h c hc).2.2

end Guarded

/-- **⚑ THE CONSERVATION THEOREM — no hypotheses.** -/
theorem resolve_conserves (b : Board) (ms : List Move) :
    Conserves b (resolveMoves b ms) (conservePhi b ms) := by
  by_cases hres : resolvableB b ms = true
  · -- the resolving branch
    have hphi : conservePhi b ms = landMap b ms := by unfold conservePhi; rw [if_pos hres]
    have hbd : resolveMoves b ms = writeBoard b (movers b ms) (landMap b ms) := by
      unfold resolveMoves; rw [if_pos hres]
    -- arrivals of movers
    have harr : ∀ c ∈ movers b ms,
        arrivalAt (movers b ms) (landMap b ms) (landMap b ms c) = some c := by
      intro c hc
      unfold arrivalAt
      rw [filter_land_eq_singleton b ms hres c hc]
      rfl
    -- no mover lands on a carrying square that is not itself a mover
    have hnoStayer : ∀ c ∈ movers b ms, ∀ q, landMap b ms c = q → carAt b q = true →
        q ∈ movers b ms := by
      intro c hc q hq hcar
      by_contra hqm
      have hfix := landMap_of_not_mover b ms q hcar hqm
      rcases (clean_land b ms hres c hc).2.1 with h' | h'
      · rw [hq] at h'; rw [h'] at hcar; exact absurd hcar (by simp)
      · rw [hq, hfix] at h'; exact h' rfl
    -- so nothing arrives on a carrying non-mover square
    have hnoArr : ∀ q, carAt b q = true → q ∉ movers b ms →
        arrivalAt (movers b ms) (landMap b ms) q = none := by
      intro q hcar hqm
      unfold arrivalAt
      have hnil : (movers b ms).filter (fun c => landMap b ms c == q) = [] := by
        rw [List.filter_eq_nil_iff]
        intro c hc hcq
        exact hqm (hnoStayer c hc q (by simpa using hcq) hcar)
      rw [hnil]; rfl
    -- an arrival on `q` is a mover that lands on `q`
    have harrMem : ∀ q c, arrivalAt (movers b ms) (landMap b ms) q = some c →
        c ∈ movers b ms ∧ landMap b ms c = q := by
      intro q c hq
      unfold arrivalAt at hq
      have hsing : (movers b ms).filter (fun c' => landMap b ms c' == q) = [c] := by
        cases hfl : (movers b ms).filter (fun c' => landMap b ms c' == q) with
        | nil => rw [hfl] at hq; simp [uniqueOf] at hq
        | cons a t =>
          cases t with
          | nil =>
            rw [hfl] at hq
            simp only [uniqueOf] at hq
            injection hq with hq'
            rw [hq']
          | cons a' t' => rw [hfl] at hq; simp [uniqueOf] at hq
      have hmem : c ∈ (movers b ms).filter (fun c' => landMap b ms c' == q) := by
        rw [hsing]; exact List.mem_singleton_self c
      rw [List.mem_filter] at hmem
      exact ⟨hmem.1, by simpa using hmem.2⟩
    refine ⟨?_, ?_, ?_⟩
    · -- carried
      intro c hc
      have hcar : carAt b c = true := by unfold carAt; simp [hc]
      rw [hphi, hbd]
      by_cases hm : c ∈ movers b ms
      · rw [writeBoard_cellAt b _ _ _ (land_inBounds b ms hres c hm), harr c hm]
      · rw [landMap_of_not_mover b ms c hcar hm,
            writeBoard_cellAt b _ _ _ (inBounds_of_carrying b c hc),
            hnoArr c hcar hm]
        simp only
        rw [if_neg (by simp [memB_iff]; exact hm)]
    · -- injOn
      intro c₁ c₂ h1 h2 heq
      rw [hphi] at heq
      have hcar1 : carAt b c₁ = true := by unfold carAt; simp [h1]
      have hcar2 : carAt b c₂ = true := by unfold carAt; simp [h2]
      by_cases hm1 : c₁ ∈ movers b ms <;> by_cases hm2 : c₂ ∈ movers b ms
      · have hsing := filter_land_eq_singleton b ms hres c₁ hm1
        have hmem : c₂ ∈ (movers b ms).filter
            (fun c' => landMap b ms c' == landMap b ms c₁) := by
          rw [List.mem_filter]; exact ⟨hm2, by simp [heq]⟩
        rw [hsing] at hmem
        simp only [List.mem_singleton] at hmem
        exact hmem.symm
      · exfalso
        rw [landMap_of_not_mover b ms c₂ hcar2 hm2] at heq
        exact hm2 (hnoStayer c₁ hm1 c₂ heq hcar2)
      · exfalso
        rw [landMap_of_not_mover b ms c₁ hcar1 hm1] at heq
        exact hm1 (hnoStayer c₂ hm2 c₁ heq.symm hcar1)
      · rw [landMap_of_not_mover b ms c₁ hcar1 hm1,
            landMap_of_not_mover b ms c₂ hcar2 hm2] at heq
        exact heq
    · -- onto
      intro q hq
      rw [hbd] at hq
      have hib : b.inBounds q :=
        inBounds_of_carrying (writeBoard b (movers b ms) (landMap b ms)) q hq
      rw [writeBoard_cellAt b _ _ _ hib] at hq
      cases harrq : arrivalAt (movers b ms) (landMap b ms) q with
      | some c =>
        rw [harrq] at hq
        obtain ⟨hcm, hcq⟩ := harrMem q c harrq
        exact ⟨c, hq, by rw [hphi]; exact hcq⟩
      | none =>
        rw [harrq] at hq
        simp only at hq
        by_cases hqm : memB (movers b ms) q = true
        · rw [if_pos hqm] at hq; simp [Particle.isVacuum] at hq
        · rw [if_neg hqm] at hq
          refine ⟨q, hq, ?_⟩
          rw [hphi]
          exact landMap_of_not_mover b ms q (by unfold carAt; simp [hq])
            (fun hmem => hqm ((memB_iff _ _).mpr hmem))
  · -- the conflicting branch: the board is unchanged
    have hphi : conservePhi b ms = id := by unfold conservePhi; rw [if_neg hres]
    have hbd : resolveMoves b ms = b := by unfold resolveMoves; rw [if_neg hres]
    rw [hphi, hbd]
    exact ⟨fun _ _ => rfl, fun _ _ _ _ h => h, fun q hq => ⟨q, hq, rfl⟩⟩

/-! ## §9  ⚑ DETERMINISM — permutation-invariance, PROVEN

PHILOSOPHY.md, Principle of Fairness: *"The success and order of moves should be independent
of any ordering on players or intrinsic ordering on the moves."*

`Automatafl.lean` stated this as `FairnessObligation` and it was **false** — audit D5 permutes
four moves and gets a different board. Here it is a theorem, and the reason it is provable is
structural: every primitive below reads the move list only through membership
(`List.any`, `List.filter`, `List.map`, `List.dedup`, `List.length`), and the one place that
could have been order-sensitive — who wins a shared landing square — is a CONFLICT, so
resolution never runs there. -/

theorem perm_any {α} (p : α → Bool) {l₁ l₂ : List α} (h : l₁.Perm l₂) :
    l₁.any p = l₂.any p := by
  apply Bool.eq_iff_iff.mpr
  simp only [List.any_eq_true]
  exact ⟨fun ⟨x, hx, hp⟩ => ⟨x, h.mem_iff.mp hx, hp⟩,
         fun ⟨x, hx, hp⟩ => ⟨x, h.mem_iff.mpr hx, hp⟩⟩

theorem perm_any' {α} {p q : α → Bool} (hpq : ∀ a, p a = q a) {l₁ l₂ : List α}
    (h : l₁.Perm l₂) : l₁.any p = l₂.any q := by
  rw [show p = q from funext hpq]; exact perm_any _ h

theorem allEqOpt_spec (l : List Coord) (d : Coord) :
    allEqOpt l = some d ↔ (d ∈ l ∧ ∀ x ∈ l, x = d) := by
  cases l with
  | nil => simp [allEqOpt]
  | cons a t =>
    have hhd : (a :: t).head? = some a := rfl
    simp only [allEqOpt, hhd]
    by_cases hall : (a :: t).all (fun e => e == a) = true
    · rw [if_pos hall]
      constructor
      · intro h
        injection h with h
        subst h
        exact ⟨List.mem_cons_self, fun x hx => by simpa using List.all_eq_true.mp hall x hx⟩
      · rintro ⟨_, hev⟩
        rw [hev a List.mem_cons_self]
    · rw [if_neg hall]
      constructor
      · intro h; exact absurd h (by simp)
      · rintro ⟨_, hev⟩
        exfalso
        apply hall
        rw [List.all_eq_true]
        intro x hx
        rw [hev x hx, hev a List.mem_cons_self]
        simp

theorem allEqOpt_perm {l₁ l₂ : List Coord} (h : l₁.Perm l₂) :
    allEqOpt l₁ = allEqOpt l₂ := by
  apply Option.ext
  intro d
  rw [allEqOpt_spec, allEqOpt_spec]
  constructor
  · rintro ⟨hm, hev⟩; exact ⟨h.mem_iff.mp hm, fun x hx => hev x (h.mem_iff.mpr hx)⟩
  · rintro ⟨hm, hev⟩; exact ⟨h.mem_iff.mpr hm, fun x hx => hev x (h.mem_iff.mp hx)⟩

theorem uniqueOf_spec (l : List Coord) (c : Coord) : uniqueOf l = some c ↔ l = [c] := by
  cases l with
  | nil => simp [uniqueOf]
  | cons a t =>
    cases t with
    | nil =>
      constructor
      · intro h; simp only [uniqueOf] at h; injection h with h; rw [h]
      · intro h; injection h with h _; rw [h]; rfl
    | cons a' t' => simp [uniqueOf]

theorem uniqueOf_perm {l₁ l₂ : List Coord} (h : l₁.Perm l₂) :
    uniqueOf l₁ = uniqueOf l₂ := by
  apply Option.ext
  intro c
  rw [uniqueOf_spec, uniqueOf_spec]
  constructor
  · intro h1; subst h1; exact (List.perm_singleton.mp h.symm)
  · intro h2; subst h2; exact (List.perm_singleton.mp h)

/-- `blockedB` reads the move list only through membership. -/
theorem blockedB_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (b : Board) (m : Move) :
    blockedB b ms₁ m = blockedB b ms₂ m := by
  unfold blockedB
  congr 1
  funext c
  rw [show (ms₁.any fun m' => m'.frm == c) = (ms₂.any fun m' => m'.frm == c) from perm_any _ h]

/-- The move graph is permutation-invariant. -/
theorem edgeMap_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (b : Board) :
    edgeMap b ms₁ = edgeMap b ms₂ := by
  funext c
  unfold edgeMap edgeOf
  apply allEqOpt_perm
  apply List.Perm.map
  have hfe : ms₁.filter (fun m => m.frm == c && !blockedB b ms₁ m)
      = ms₁.filter (fun m => m.frm == c && !blockedB b ms₂ m) := by
    apply List.filter_congr
    intro m _
    rw [blockedB_perm h b m]
  rw [hfe]
  exact h.filter _

/-- The landing map is permutation-invariant. -/
theorem landMap_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (b : Board) :
    landMap b ms₁ = landMap b ms₂ := by
  unfold landMap
  rw [edgeMap_perm h b, h.length_eq]

/-- The mover set is permutation-invariant (as a set). -/
theorem movers_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (b : Board) :
    (movers b ms₁).Perm (movers b ms₂) := by
  unfold movers moverList
  rw [landMap_perm h b]
  exact ((h.map _).dedup).filter _

/-- `landBad` is permutation-invariant. -/
theorem landBad_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (b : Board) (c : Coord) :
    landBad b ms₁ c = landBad b ms₂ c := by
  have hL := landMap_perm h b
  have hlen : ((movers b ms₁).filter (fun c' => landMap b ms₁ c' == landMap b ms₁ c)).length
      = ((movers b ms₂).filter (fun c' => landMap b ms₂ c' == landMap b ms₂ c)).length := by
    rw [hL]; exact ((movers_perm h b).filter _).length_eq
  unfold landBad
  rw [hlen, hL]

/-- The contested-coordinate set is permutation-invariant (as a set). -/
theorem unresolved_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (b : Board) :
    (unresolved b ms₁).Perm (unresolved b ms₂) := by
  unfold unresolved
  rw [landMap_perm h b]
  have hf : (movers b ms₁).filter (landBad b ms₁)
      = (movers b ms₁).filter (landBad b ms₂) := by
    apply List.filter_congr; intro c _; exact landBad_perm h b c
  rw [hf]
  exact (((movers_perm h b).filter _).map _).dedup

/-- The resolvability gate is permutation-invariant. -/
theorem resolvableB_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (b : Board) :
    resolvableB b ms₁ = resolvableB b ms₂ := by
  have hp := unresolved_perm h b
  unfold resolvableB
  apply Bool.eq_iff_iff.mpr
  simp only [List.isEmpty_iff]
  constructor
  · intro h1
    apply List.Perm.eq_nil
    rw [← h1]; exact hp.symm
  · intro h2
    apply List.Perm.eq_nil
    rw [← h2]; exact hp

/-- **⚑ THE DETERMINISM THEOREM.** Resolution is permutation-invariant: no player order, no
submission order, no move-list order can change the resulting board. -/
theorem resolve_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (b : Board) (q : Coord) :
    (resolveMoves b ms₁).cellAt q = (resolveMoves b ms₂).cellAt q := by
  have hL := landMap_perm h b
  have hM := movers_perm h b
  have harr : ∀ x, arrivalAt (movers b ms₁) (landMap b ms₁) x
      = arrivalAt (movers b ms₂) (landMap b ms₂) x := by
    intro x
    unfold arrivalAt
    rw [hL]
    exact uniqueOf_perm (hM.filter _)
  have hmem : ∀ x, memB (movers b ms₁) x = memB (movers b ms₂) x := by
    intro x; unfold memB; exact perm_any _ hM
  have hwrite : writeBoard b (movers b ms₁) (landMap b ms₁)
      = writeBoard b (movers b ms₂) (landMap b ms₂) := by
    unfold writeBoard
    congr 1
    funext x
    rw [harr x, hmem x]
  unfold resolveMoves
  rw [resolvableB_perm h b]
  by_cases hr : resolvableB b ms₂ = true
  · rw [if_pos hr, if_pos hr, hwrite]
  · rw [if_neg hr, if_neg hr]

theorem forkAt_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (s : Coord) :
    forkAt ms₁ s = forkAt ms₂ s := by
  unfold forkAt
  exact perm_any' (fun m₁ => perm_any _ h) h

theorem collideAt_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (b : Board) (d : Coord) :
    collideAt b ms₁ d = collideAt b ms₂ d := by
  unfold collideAt
  exact perm_any' (fun m₁ => perm_any _ h) h

theorem candidates_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) :
    (candidates ms₁).Perm (candidates ms₂) := (h.map _).append (h.map _)

/-- Conflict detection is permutation-invariant too, so a round's BRANCH does not depend on
submission order either. -/
theorem clashCoords_perm {ms₁ ms₂ : List Move} (h : ms₁.Perm ms₂) (b : Board) (c : Coord) :
    (c ∈ clashCoords b ms₁) ↔ (c ∈ clashCoords b ms₂) := by
  unfold clashCoords
  have hf : (candidates ms₁).filter (fun x => forkAt ms₁ x || collideAt b ms₁ x)
      = (candidates ms₁).filter (fun x => forkAt ms₂ x || collideAt b ms₂ x) := by
    apply List.filter_congr
    intro x _
    rw [forkAt_perm h x, collideAt_perm h b x]
  simp only [List.mem_dedup]
  rw [hf]
  exact ((candidates_perm h).filter _).mem_iff

/-! ## §9.6  ⚑ THE m = 2 RESOLVEMOVES COLLAPSE — closed forms for a two-move round

The landing machinery of §4 (`edgeMap`/`landMap` via the bounded-fuel `stopWalk`/`leaves` mutual
recursion) has no obvious closed form: it is written to be permutation-invariant and to handle
`m`-move rounds uniformly. This section proves the CLOSED FORMS at `m = 2` — the analog, for the new
machinery, of the retired `followChain`/`chainDest_a`/`chainDest_b` at arity two.

Fix a move list `[ma, mb]` that survived conflict adjudication: **distinct sources** (`ma.frm ≠ mb.frm`,
else a fork), **distinct raw destinations** (`ma.to ≠ mb.to`, else a collide/merge), both `MoveLegal`.
Write `sa/da` for `ma.frm/ma.to`, `sb/db` for `mb.frm/mb.to`, and `BA/BB` for `blockedB` of each.

* `edgeMap_pair` — the move graph is supported on `{sa, sb}`: `E sa = if BA then none else some da`,
  `E sb = if BB then none else some db`, `E c = none` elsewhere. Hence `landMap c = c` off `{sa, sb}`.
* `landMap_pair_a` / `landMap_pair_b` — **the two landing closed forms**, casing on the finite
  configuration set. Each mover ends at (in precedence): its origin if its own move is blocked;
  otherwise, if it chains into the other's source (`da = sb`), the sub-cases —
  leader blocked ⇒ stay (occupied source) or slide into the empty square; **2-cycle** (`db = sa`) ⇒
  **stay** (ruling C); **caterpillar** (occupied vacated source) ⇒ ride onto it (`= da`);
  **flow-through** (vacuum waypoint) ⇒ continue to `db`; otherwise its own destination `da`.
* `movers_pair` — the movers are `[sa, sb]` filtered by "carries a piece and actually moves".
* `resolvableB_pair` — **NO CONFLUENCE at m = 2**: a legal, distinct-source, distinct-dest pair is
  ALWAYS `resolvableB` (`= true`). This is the `landBad`/merge-clause obligation the emitted
  `cResolvable = ¬merge · ¬badA · ¬badB` must match; the two movers cannot converge because a piece
  that reaches the OTHER move's destination did so through that move's VACUUM source, whence the other
  square is empty and its move contributes no rival landing (`landMap_movers_distinct`).
* `writeBoard_resolveMoves_cell` / `resolveMoves_cell_pair` — `(resolveMoves b [ma,mb]).cellAt q` in
  closed, cell-by-cell form (`resolvableB` ? the arrival/vacated board : `b`), the reference surface
  the circuit's `cMidV2` cell gate matches against.

Nothing here changes an existing definition; §10's audit witnesses re-appear at the foot of the
conformance block, computed against these closed forms (`landMap`/`movers`/`resolvableB` and the
firing theorems). -/

/-- **`blockedB_swap`.** `blockedB` depends on the mover set only through MEMBERSHIP of the sources
(`ms.any (·.frm == c)`), so swapping the two-element mover list leaves it unchanged — the mirror of
`AutomataflResolveCapstone.occluded_swap` for the inclusive `pathCells` predicate. This is what lets
the emitted descriptor (which pins the OTHER piece via `cOccIncl (1 - which)`, i.e. builds the mover
list in each move's own order `[which, 1 - which]`) meet the reference's fixed order `[m0, m1]`. -/
theorem blockedB_swap (b : Board) (m0 m1 m : Move) :
    blockedB b [m0, m1] m = blockedB b [m1, m0] m := by
  simp only [blockedB, List.any_cons, List.any_nil, Bool.or_false]
  congr 1
  funext c
  rw [Bool.or_comm (m0.frm == c) (m1.frm == c)]

/-- A list is nonempty iff it has a member. -/
theorem ne_nil_iff_exists {α : Type _} (l : List α) : l ≠ [] ↔ ∃ x, x ∈ l := by
  constructor
  · exact List.exists_mem_of_ne_nil l
  · rintro ⟨x, hx⟩; exact List.ne_nil_of_mem hx

/-- `dedup` preserves nonemptiness. -/
theorem dedup_ne_nil_iff {α : Type _} [DecidableEq α] (l : List α) :
    l.dedup ≠ [] ↔ l ≠ [] := by
  rw [ne_nil_iff_exists, ne_nil_iff_exists]
  exact exists_congr (fun _ => List.mem_dedup)

section PairCollapse

set_option autoImplicit false

variable (b : Board) (ma mb : Move)

-- ===== edgeMap closed form for a distinct-source pair =====
theorem edgeMap_pair (hne : ma.frm ≠ mb.frm) (c : Coord) :
    edgeMap b [ma, mb] c =
      (if c = ma.frm then (if blockedB b [ma, mb] ma then none else some ma.to)
       else if c = mb.frm then (if blockedB b [ma, mb] mb then none else some mb.to)
       else none) := by
  unfold edgeMap edgeOf
  simp only [List.filter_cons, List.filter_nil]
  by_cases ha : c = ma.frm
  · subst ha
    have hfmb : (mb.frm == ma.frm) = false := by
      simp only [beq_eq_false_iff_ne]; exact fun h => hne h.symm
    rw [if_pos rfl]
    by_cases hba : blockedB b [ma, mb] ma
    · simp [hfmb, hba, allEqOpt]
    · simp only [Bool.not_eq_true] at hba
      simp [hfmb, hba, allEqOpt]
  · rw [if_neg ha]
    have hfma : (ma.frm == c) = false := by simp only [beq_eq_false_iff_ne]; exact fun h => ha h.symm
    by_cases hb : c = mb.frm
    · subst hb
      rw [if_pos rfl]
      by_cases hbb : blockedB b [ma, mb] mb
      · simp [hfma, hbb, allEqOpt]
      · simp only [Bool.not_eq_true] at hbb
        simp [hfma, hbb, allEqOpt]
    · rw [if_neg hb]
      have hfmb : (mb.frm == c) = false := by
        simp only [beq_eq_false_iff_ne]; exact fun h => hb h.symm
      simp [hfma, hfmb, allEqOpt]

-- ===== general walk helpers =====

theorem stopWalk_dead (E : Coord → Option Coord) (car : Coord → Bool) (f : Nat) (c : Coord)
    (h : E c = none) : stopWalk E car (f + 1) c = some c := by
  rw [stopWalk, h]

theorem stopWalk_stop (E : Coord → Option Coord) (car : Coord → Bool) (f : Nat) (c d : Coord)
    (h : E c = some d) (hcar : car d = true) : stopWalk E car (f + 1) c = some d := by
  rw [stopWalk, h]; simp [hcar]

theorem stopWalk_go (E : Coord → Option Coord) (car : Coord → Bool) (f : Nat) (c d : Coord)
    (h : E c = some d) (hcar : car d = false) :
    stopWalk E car (f + 1) c = stopWalk E car f d := by
  rw [stopWalk, h]; simp [hcar]

/-- A dead-end (or non-source) square lands on itself, any fuel. -/
theorem landOf_edge_none (E : Coord → Option Coord) (car : Coord → Bool) (sf vf : Nat) (c : Coord)
    (h : E c = none) : landOf E car (sf + 1) vf c = c := by
  have hs : stopWalk E car (sf + 1) c = some c := stopWalk_dead E car sf c h
  unfold landOf
  rw [hs]
  simp only [Option.getD_some, ite_self]

-- ===== destination clear when a move is not blocked =====
theorem dst_clear_of_not_blocked (bb : Board) (ms : List Move) (m : Move)
    (h : blockedB bb ms m = false) :
    carAt bb m.to = false ∨ (ms.any (fun m' => m'.frm == m.to)) = true := by
  unfold blockedB at h
  have hmem : m.to ∈ pathCells m.frm m.to := by
    unfold pathCells; exact List.mem_append_right _ (List.mem_singleton.mpr rfl)
  have hp := (List.any_eq_false.mp h) m.to hmem
  simp only [Bool.not_eq_true, Bool.and_eq_false_iff, Bool.not_eq_false'] at hp
  unfold carAt
  rcases hp with hp | hp
  · exact Or.inl (by simp [hp])
  · exact Or.inr hp

/-- The destination of an unblocked move that is nobody's source is EMPTY at turn start. -/
theorem carAt_to_false_of_not_blocked (bb : Board) (ms : List Move) (m : Move)
    (h : blockedB bb ms m = false)
    (hns : (ms.any (fun m' => m'.frm == m.to)) = false) : carAt bb m.to = false := by
  rcases dst_clear_of_not_blocked bb ms m h with hc | hc
  · exact hc
  · rw [hns] at hc; exact absurd hc (by simp)

-- ===== landOf outcome helpers (bounded-fuel walk resolved) =====

/-- Walk stops at a DISTINCT EMPTY square `d` (a dead end) ⇒ the piece lands there. -/
theorem landOf_stop_empty (E : Coord → Option Coord) (car : Coord → Bool) (sf vf : Nat)
    (c d : Coord) (htc : twoCyc E c = false) (hs : stopWalk E car sf c = some d)
    (hdc : d ≠ c) (hcar : car d = false) : landOf E car sf (vf + 1) c = d := by
  unfold landOf
  have hl : leaves E car sf (vf + 1) c = true := by
    rw [leaves, hs, htc]; simp [hdc, hcar]
  rw [if_pos hl, hs, Option.getD_some]

/-- `c` is in a 2-cycle ⇒ it stays. -/
theorem landOf_twoCyc (E : Coord → Option Coord) (car : Coord → Bool) (sf vf : Nat) (c : Coord)
    (h : twoCyc E c = true) : landOf E car sf (vf + 1) c = c := by
  unfold landOf
  have hl : leaves E car sf (vf + 1) c = false := by rw [leaves, if_pos h]
  rw [if_neg (by simp [hl])]

/-- Walk stops at an OCCUPIED square `d` that itself does NOT leave ⇒ the piece stays. -/
theorem landOf_stop_stayer (E : Coord → Option Coord) (car : Coord → Bool) (sf vf : Nat)
    (c d : Coord) (htc : twoCyc E c = false) (hs : stopWalk E car sf c = some d)
    (hdc : d ≠ c) (hcar : car d = true) (hleave : leaves E car sf vf d = false) :
    landOf E car sf (vf + 1) c = c := by
  unfold landOf
  have hl : leaves E car sf (vf + 1) c = false := by
    rw [leaves, hs, htc]; simp [hdc, hcar, hleave]
  rw [if_neg (by simp [hl])]

/-- Walk stops at an OCCUPIED square `d` that itself DOES leave ⇒ the piece rides onto `d`. -/
theorem landOf_stop_mover (E : Coord → Option Coord) (car : Coord → Bool) (sf vf : Nat)
    (c d : Coord) (htc : twoCyc E c = false) (hs : stopWalk E car sf c = some d)
    (hdc : d ≠ c) (hcar : car d = true) (hleave : leaves E car sf vf d = true) :
    landOf E car sf (vf + 1) c = d := by
  unfold landOf
  have hl : leaves E car sf (vf + 1) c = true := by
    rw [leaves, hs, htc]; simp [hdc, hcar, hleave]
  rw [if_pos hl, hs, Option.getD_some]

-- ===== edge values at the pair's coordinates =====
theorem edge_a (hne : ma.frm ≠ mb.frm) :
    edgeMap b [ma, mb] ma.frm = if blockedB b [ma, mb] ma then none else some ma.to := by
  rw [edgeMap_pair b ma mb hne, if_pos rfl]

theorem edge_b (hne : ma.frm ≠ mb.frm) :
    edgeMap b [ma, mb] mb.frm = if blockedB b [ma, mb] mb then none else some mb.to := by
  rw [edgeMap_pair b ma mb hne, if_neg (Ne.symm hne), if_pos rfl]

theorem edge_off (hne : ma.frm ≠ mb.frm) (c : Coord) (ha : c ≠ ma.frm) (hb : c ≠ mb.frm) :
    edgeMap b [ma, mb] c = none := by
  rw [edgeMap_pair b ma mb hne, if_neg ha, if_neg hb]

theorem landMap_pair_eq (c : Coord) :
    landMap b [ma, mb] c = landOf (edgeMap b [ma, mb]) (carAt b) 3 3 c := rfl

-- ===== (i) INDEPENDENT: each piece lands at its own destination (unblocked) or stays (blocked) =====
theorem landMap_pair_indep_a (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to)
    (hab : ma.to ≠ mb.frm) (hBA : blockedB b [ma, mb] ma = false) :
    landMap b [ma, mb] ma.frm = ma.to := by
  have hda_ne_sa : ma.to ≠ ma.frm := fun h => hlegA h.symm
  have hEa : edgeMap b [ma, mb] ma.frm = some ma.to := by rw [edge_a b ma mb hne, if_neg (by simp [hBA])]
  have hns : ([ma, mb].any (fun m' => m'.frm == ma.to)) = false := by
    simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff, beq_eq_false_iff_ne]
    exact ⟨hlegA, fun h => hab h.symm⟩
  have hcarDa : carAt b ma.to = false := carAt_to_false_of_not_blocked b [ma, mb] ma hBA hns
  have hEda : edgeMap b [ma, mb] ma.to = none := edge_off b ma mb hne ma.to hda_ne_sa hab
  have hstop : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 ma.frm = some ma.to := by
    rw [stopWalk_go (edgeMap b [ma, mb]) (carAt b) 2 ma.frm ma.to hEa hcarDa]
    exact stopWalk_dead (edgeMap b [ma, mb]) (carAt b) 1 ma.to hEda
  have htc : twoCyc (edgeMap b [ma, mb]) ma.frm = false := by simp [twoCyc, hEa, hEda]
  rw [landMap_pair_eq b ma mb]
  exact landOf_stop_empty (edgeMap b [ma, mb]) (carAt b) 3 2 ma.frm ma.to htc hstop hda_ne_sa hcarDa

theorem landMap_pair_indep_b (hne : ma.frm ≠ mb.frm) (hlegB : mb.frm ≠ mb.to)
    (hba : mb.to ≠ ma.frm) (hBB : blockedB b [ma, mb] mb = false) :
    landMap b [ma, mb] mb.frm = mb.to := by
  have hdb_ne_sb : mb.to ≠ mb.frm := fun h => hlegB h.symm
  have hEb : edgeMap b [ma, mb] mb.frm = some mb.to := by rw [edge_b b ma mb hne, if_neg (by simp [hBB])]
  have hns : ([ma, mb].any (fun m' => m'.frm == mb.to)) = false := by
    simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff, beq_eq_false_iff_ne]
    exact ⟨fun h => hba h.symm, hlegB⟩
  have hcarDb : carAt b mb.to = false := carAt_to_false_of_not_blocked b [ma, mb] mb hBB hns
  have hEdb : edgeMap b [ma, mb] mb.to = none := edge_off b ma mb hne mb.to hba hdb_ne_sb
  have hstop : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 mb.frm = some mb.to := by
    rw [stopWalk_go (edgeMap b [ma, mb]) (carAt b) 2 mb.frm mb.to hEb hcarDb]
    exact stopWalk_dead (edgeMap b [ma, mb]) (carAt b) 1 mb.to hEdb
  have htc : twoCyc (edgeMap b [ma, mb]) mb.frm = false := by simp [twoCyc, hEb, hEdb]
  rw [landMap_pair_eq b ma mb]
  exact landOf_stop_empty (edgeMap b [ma, mb]) (carAt b) 3 2 mb.frm mb.to htc hstop hdb_ne_sb hcarDb

-- ===== (iv-partial) BLOCKED: an occluded move does not execute, its piece stays =====
theorem landMap_pair_blocked_a (hne : ma.frm ≠ mb.frm) (hBA : blockedB b [ma, mb] ma = true) :
    landMap b [ma, mb] ma.frm = ma.frm := by
  have hEa : edgeMap b [ma, mb] ma.frm = none := by rw [edge_a b ma mb hne, if_pos hBA]
  rw [landMap_pair_eq b ma mb]
  exact landOf_edge_none (edgeMap b [ma, mb]) (carAt b) 2 3 ma.frm hEa

theorem landMap_pair_blocked_b (hne : ma.frm ≠ mb.frm) (hBB : blockedB b [ma, mb] mb = true) :
    landMap b [ma, mb] mb.frm = mb.frm := by
  have hEb : edgeMap b [ma, mb] mb.frm = none := by rw [edge_b b ma mb hne, if_pos hBB]
  rw [landMap_pair_eq b ma mb]
  exact landOf_edge_none (edgeMap b [ma, mb]) (carAt b) 2 3 mb.frm hEb

-- ===== (iii) 2-CYCLE: both pieces stay (ruling C) =====
theorem landMap_pair_twoCycle_a (hne : ma.frm ≠ mb.frm)
    (hab : ma.to = mb.frm) (hba : mb.to = ma.frm)
    (hBA : blockedB b [ma, mb] ma = false) (hBB : blockedB b [ma, mb] mb = false) :
    landMap b [ma, mb] ma.frm = ma.frm := by
  have hEa : edgeMap b [ma, mb] ma.frm = some ma.to := by rw [edge_a b ma mb hne, if_neg (by simp [hBA])]
  have hEb : edgeMap b [ma, mb] mb.frm = some mb.to := by rw [edge_b b ma mb hne, if_neg (by simp [hBB])]
  have htc : twoCyc (edgeMap b [ma, mb]) ma.frm = true := by
    simp only [twoCyc, hEa]; rw [hab, hEb, hba]; simp [Ne.symm hne]
  rw [landMap_pair_eq b ma mb]
  exact landOf_twoCyc (edgeMap b [ma, mb]) (carAt b) 3 2 ma.frm htc

theorem landMap_pair_twoCycle_b (hne : ma.frm ≠ mb.frm)
    (hab : ma.to = mb.frm) (hba : mb.to = ma.frm)
    (hBA : blockedB b [ma, mb] ma = false) (hBB : blockedB b [ma, mb] mb = false) :
    landMap b [ma, mb] mb.frm = mb.frm := by
  have hEa : edgeMap b [ma, mb] ma.frm = some ma.to := by rw [edge_a b ma mb hne, if_neg (by simp [hBA])]
  have hEb : edgeMap b [ma, mb] mb.frm = some mb.to := by rw [edge_b b ma mb hne, if_neg (by simp [hBB])]
  have htc : twoCyc (edgeMap b [ma, mb]) mb.frm = true := by
    simp only [twoCyc, hEb]; rw [hba, hEa, hab]; simp [hne]
  rw [landMap_pair_eq b ma mb]
  exact landOf_twoCyc (edgeMap b [ma, mb]) (carAt b) 3 2 mb.frm htc

-- ===== leaves helpers for the chain recursion =====
theorem leaves_stop_empty (E : Coord → Option Coord) (car : Coord → Bool) (sf vf : Nat)
    (c d : Coord) (htc : twoCyc E c = false) (hs : stopWalk E car sf c = some d)
    (hdc : d ≠ c) (hcar : car d = false) : leaves E car sf (vf + 1) c = true := by
  rw [leaves, hs, htc]; simp [hdc, hcar]

theorem leaves_dead (E : Coord → Option Coord) (car : Coord → Bool) (sf vf : Nat) (c : Coord)
    (h : E c = none) : leaves E car (sf + 1) (vf + 1) c = false := by
  have htc : twoCyc E c = false := by unfold twoCyc; rw [h]
  rw [leaves, stopWalk_dead E car sf c h, htc]; simp

-- ===== (ii) CHAIN / caterpillar: the follower rides onto the vacated leader source =====
theorem landMap_pair_caterpillar_a (hne : ma.frm ≠ mb.frm) (hlegB : mb.frm ≠ mb.to)
    (hab : ma.to = mb.frm) (hba : mb.to ≠ ma.frm) (hcarSb : carAt b mb.frm = true)
    (hBA : blockedB b [ma, mb] ma = false) (hBB : blockedB b [ma, mb] mb = false) :
    landMap b [ma, mb] ma.frm = ma.to := by
  have hEa : edgeMap b [ma, mb] ma.frm = some ma.to := by rw [edge_a b ma mb hne, if_neg (by simp [hBA])]
  have hEb : edgeMap b [ma, mb] mb.frm = some mb.to := by rw [edge_b b ma mb hne, if_neg (by simp [hBB])]
  have hdb_ne_sb : mb.to ≠ mb.frm := fun h => hlegB h.symm
  have hns_db : ([ma, mb].any (fun m' => m'.frm == mb.to)) = false := by
    simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff, beq_eq_false_iff_ne]
    exact ⟨fun h => hba h.symm, hlegB⟩
  have hcarDb : carAt b mb.to = false := carAt_to_false_of_not_blocked b [ma, mb] mb hBB hns_db
  have hEdb : edgeMap b [ma, mb] mb.to = none := edge_off b ma mb hne mb.to hba hdb_ne_sb
  have hstopB : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 mb.frm = some mb.to := by
    rw [stopWalk_go (edgeMap b [ma, mb]) (carAt b) 2 mb.frm mb.to hEb hcarDb]
    exact stopWalk_dead (edgeMap b [ma, mb]) (carAt b) 1 mb.to hEdb
  have htcB : twoCyc (edgeMap b [ma, mb]) mb.frm = false := by simp [twoCyc, hEb, hEdb]
  have hleaveB : leaves (edgeMap b [ma, mb]) (carAt b) 3 2 mb.frm = true :=
    leaves_stop_empty (edgeMap b [ma, mb]) (carAt b) 3 1 mb.frm mb.to htcB hstopB hdb_ne_sb hcarDb
  have hcarDa : carAt b ma.to = true := by rw [hab]; exact hcarSb
  have hda_ne_sa : ma.to ≠ ma.frm := by rw [hab]; exact Ne.symm hne
  have hstopA : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 ma.frm = some ma.to :=
    stopWalk_stop (edgeMap b [ma, mb]) (carAt b) 2 ma.frm ma.to hEa hcarDa
  have htcA : twoCyc (edgeMap b [ma, mb]) ma.frm = false := by
    simp only [twoCyc, hEa]; rw [hab, hEb]; simp [hba]
  have hleaveDa : leaves (edgeMap b [ma, mb]) (carAt b) 3 2 ma.to = true := by rw [hab]; exact hleaveB
  rw [landMap_pair_eq b ma mb]
  exact landOf_stop_mover (edgeMap b [ma, mb]) (carAt b) 3 2 ma.frm ma.to htcA hstopA hda_ne_sa hcarDa hleaveDa

-- ===== (ii') CHAIN through a VACUUM waypoint: the piece flows all the way to db =====
theorem landMap_pair_flowthrough_a (hne : ma.frm ≠ mb.frm) (hlegB : mb.frm ≠ mb.to)
    (hab : ma.to = mb.frm) (hba : mb.to ≠ ma.frm) (hvacSb : carAt b mb.frm = false)
    (hBA : blockedB b [ma, mb] ma = false) (hBB : blockedB b [ma, mb] mb = false) :
    landMap b [ma, mb] ma.frm = mb.to := by
  have hEa : edgeMap b [ma, mb] ma.frm = some ma.to := by rw [edge_a b ma mb hne, if_neg (by simp [hBA])]
  have hEb : edgeMap b [ma, mb] mb.frm = some mb.to := by rw [edge_b b ma mb hne, if_neg (by simp [hBB])]
  have hdb_ne_sb : mb.to ≠ mb.frm := fun h => hlegB h.symm
  have hns_db : ([ma, mb].any (fun m' => m'.frm == mb.to)) = false := by
    simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff, beq_eq_false_iff_ne]
    exact ⟨fun h => hba h.symm, hlegB⟩
  have hcarDb : carAt b mb.to = false := carAt_to_false_of_not_blocked b [ma, mb] mb hBB hns_db
  have hEdb : edgeMap b [ma, mb] mb.to = none := edge_off b ma mb hne mb.to hba hdb_ne_sb
  have hcarDa : carAt b ma.to = false := by rw [hab]; exact hvacSb
  have hstopA : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 ma.frm = some mb.to := by
    rw [stopWalk_go (edgeMap b [ma, mb]) (carAt b) 2 ma.frm ma.to hEa hcarDa, hab,
        stopWalk_go (edgeMap b [ma, mb]) (carAt b) 1 mb.frm mb.to hEb hcarDb]
    exact stopWalk_dead (edgeMap b [ma, mb]) (carAt b) 0 mb.to hEdb
  have htcA : twoCyc (edgeMap b [ma, mb]) ma.frm = false := by
    simp only [twoCyc, hEa]; rw [hab, hEb]; simp [hba]
  rw [landMap_pair_eq b ma mb]
  exact landOf_stop_empty (edgeMap b [ma, mb]) (carAt b) 3 2 ma.frm mb.to htcA hstopA hba hcarDb

-- ===== (iv) OCCLUDED-STAYER: leader blocked ⇒ follower on the occupied leader-source stays =====
theorem landMap_pair_stuck_a (hne : ma.frm ≠ mb.frm)
    (hab : ma.to = mb.frm) (hcarSb : carAt b mb.frm = true)
    (hBA : blockedB b [ma, mb] ma = false) (hBB : blockedB b [ma, mb] mb = true) :
    landMap b [ma, mb] ma.frm = ma.frm := by
  have hEa : edgeMap b [ma, mb] ma.frm = some ma.to := by rw [edge_a b ma mb hne, if_neg (by simp [hBA])]
  have hEb0 : edgeMap b [ma, mb] mb.frm = none := by rw [edge_b b ma mb hne, if_pos hBB]
  have hcarDa : carAt b ma.to = true := by rw [hab]; exact hcarSb
  have hda_ne_sa : ma.to ≠ ma.frm := by rw [hab]; exact Ne.symm hne
  have hstopA : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 ma.frm = some ma.to :=
    stopWalk_stop (edgeMap b [ma, mb]) (carAt b) 2 ma.frm ma.to hEa hcarDa
  have htcA : twoCyc (edgeMap b [ma, mb]) ma.frm = false := by
    simp only [twoCyc, hEa]; rw [hab, hEb0]; simp
  have hleaveDa : leaves (edgeMap b [ma, mb]) (carAt b) 3 2 ma.to = false := by
    rw [hab]; exact leaves_dead (edgeMap b [ma, mb]) (carAt b) 2 1 mb.frm hEb0
  rw [landMap_pair_eq b ma mb]
  exact landOf_stop_stayer (edgeMap b [ma, mb]) (carAt b) 3 2 ma.frm ma.to htcA hstopA hda_ne_sa hcarDa hleaveDa

-- ===== leader blocked but its source VACUUM: ma just moves into the empty square =====
theorem landMap_pair_intoEmpty_a (hne : ma.frm ≠ mb.frm)
    (hab : ma.to = mb.frm) (hvacSb : carAt b mb.frm = false)
    (hBA : blockedB b [ma, mb] ma = false) (hBB : blockedB b [ma, mb] mb = true) :
    landMap b [ma, mb] ma.frm = ma.to := by
  have hEa : edgeMap b [ma, mb] ma.frm = some ma.to := by rw [edge_a b ma mb hne, if_neg (by simp [hBA])]
  have hEb0 : edgeMap b [ma, mb] mb.frm = none := by rw [edge_b b ma mb hne, if_pos hBB]
  have hcarDa : carAt b ma.to = false := by rw [hab]; exact hvacSb
  have hda_ne_sa : ma.to ≠ ma.frm := by rw [hab]; exact Ne.symm hne
  have hstopA : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 ma.frm = some ma.to := by
    rw [stopWalk_go (edgeMap b [ma, mb]) (carAt b) 2 ma.frm ma.to hEa hcarDa, hab]
    exact stopWalk_dead (edgeMap b [ma, mb]) (carAt b) 1 mb.frm hEb0
  have htcA : twoCyc (edgeMap b [ma, mb]) ma.frm = false := by
    simp only [twoCyc, hEa]; rw [hab, hEb0]; simp
  rw [landMap_pair_eq b ma mb]
  exact landOf_stop_empty (edgeMap b [ma, mb]) (carAt b) 3 2 ma.frm ma.to htcA hstopA hda_ne_sa hcarDa

-- ===== THE A-SIDE LANDING, ALL CASES (analog of the old `chainDest_a`) =====
theorem landMap_pair_a (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to) :
    landMap b [ma, mb] ma.frm =
      (if blockedB b [ma, mb] ma then ma.frm
       else if ma.to = mb.frm then
         (if blockedB b [ma, mb] mb then (if carAt b mb.frm then ma.frm else ma.to)
          else if mb.to = ma.frm then ma.frm
          else (if carAt b mb.frm then ma.to else mb.to))
       else ma.to) := by
  by_cases hBA : blockedB b [ma, mb] ma = true
  · rw [if_pos hBA]; exact landMap_pair_blocked_a b ma mb hne hBA
  · rw [if_neg hBA]; simp only [Bool.not_eq_true] at hBA
    by_cases hab : ma.to = mb.frm
    · rw [if_pos hab]
      by_cases hBB : blockedB b [ma, mb] mb = true
      · rw [if_pos hBB]
        by_cases hcs : carAt b mb.frm = true
        · rw [if_pos hcs]; exact landMap_pair_stuck_a b ma mb hne hab hcs hBA hBB
        · rw [if_neg hcs]; simp only [Bool.not_eq_true] at hcs
          exact landMap_pair_intoEmpty_a b ma mb hne hab hcs hBA hBB
      · rw [if_neg hBB]; simp only [Bool.not_eq_true] at hBB
        by_cases hba : mb.to = ma.frm
        · rw [if_pos hba]; exact landMap_pair_twoCycle_a b ma mb hne hab hba hBA hBB
        · rw [if_neg hba]
          by_cases hcs : carAt b mb.frm = true
          · rw [if_pos hcs]; exact landMap_pair_caterpillar_a b ma mb hne hlegB hab hba hcs hBA hBB
          · rw [if_neg hcs]; simp only [Bool.not_eq_true] at hcs
            exact landMap_pair_flowthrough_a b ma mb hne hlegB hab hba hcs hBA hBB
    · rw [if_neg hab]; exact landMap_pair_indep_a b ma mb hne hlegA hab hBA

-- ===== B-SIDE chain cases (mirror; the follower is mb, the leader is ma) =====
theorem landMap_pair_caterpillar_b (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to)
    (hba : mb.to = ma.frm) (hab : ma.to ≠ mb.frm) (hcarSa : carAt b ma.frm = true)
    (hBA : blockedB b [ma, mb] ma = false) (hBB : blockedB b [ma, mb] mb = false) :
    landMap b [ma, mb] mb.frm = mb.to := by
  have hEb : edgeMap b [ma, mb] mb.frm = some mb.to := by rw [edge_b b ma mb hne, if_neg (by simp [hBB])]
  have hEa : edgeMap b [ma, mb] ma.frm = some ma.to := by rw [edge_a b ma mb hne, if_neg (by simp [hBA])]
  have hda_ne_sa : ma.to ≠ ma.frm := fun h => hlegA h.symm
  have hns_da : ([ma, mb].any (fun m' => m'.frm == ma.to)) = false := by
    simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff, beq_eq_false_iff_ne]
    exact ⟨hlegA, fun h => hab h.symm⟩
  have hcarDa : carAt b ma.to = false := carAt_to_false_of_not_blocked b [ma, mb] ma hBA hns_da
  have hEda : edgeMap b [ma, mb] ma.to = none := edge_off b ma mb hne ma.to hda_ne_sa hab
  have hstopA : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 ma.frm = some ma.to := by
    rw [stopWalk_go (edgeMap b [ma, mb]) (carAt b) 2 ma.frm ma.to hEa hcarDa]
    exact stopWalk_dead (edgeMap b [ma, mb]) (carAt b) 1 ma.to hEda
  have htcA : twoCyc (edgeMap b [ma, mb]) ma.frm = false := by simp [twoCyc, hEa, hEda]
  have hleaveA : leaves (edgeMap b [ma, mb]) (carAt b) 3 2 ma.frm = true :=
    leaves_stop_empty (edgeMap b [ma, mb]) (carAt b) 3 1 ma.frm ma.to htcA hstopA hda_ne_sa hcarDa
  have hcarDb : carAt b mb.to = true := by rw [hba]; exact hcarSa
  have hdb_ne_sb : mb.to ≠ mb.frm := by rw [hba]; exact hne
  have hstopB : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 mb.frm = some mb.to :=
    stopWalk_stop (edgeMap b [ma, mb]) (carAt b) 2 mb.frm mb.to hEb hcarDb
  have htcB : twoCyc (edgeMap b [ma, mb]) mb.frm = false := by
    simp only [twoCyc, hEb]; rw [hba, hEa]; simp [hab]
  have hleaveDb : leaves (edgeMap b [ma, mb]) (carAt b) 3 2 mb.to = true := by rw [hba]; exact hleaveA
  rw [landMap_pair_eq b ma mb]
  exact landOf_stop_mover (edgeMap b [ma, mb]) (carAt b) 3 2 mb.frm mb.to htcB hstopB hdb_ne_sb hcarDb hleaveDb

theorem landMap_pair_flowthrough_b (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to)
    (hba : mb.to = ma.frm) (hab : ma.to ≠ mb.frm) (hvacSa : carAt b ma.frm = false)
    (hBA : blockedB b [ma, mb] ma = false) (hBB : blockedB b [ma, mb] mb = false) :
    landMap b [ma, mb] mb.frm = ma.to := by
  have hEb : edgeMap b [ma, mb] mb.frm = some mb.to := by rw [edge_b b ma mb hne, if_neg (by simp [hBB])]
  have hEa : edgeMap b [ma, mb] ma.frm = some ma.to := by rw [edge_a b ma mb hne, if_neg (by simp [hBA])]
  have hda_ne_sa : ma.to ≠ ma.frm := fun h => hlegA h.symm
  have hns_da : ([ma, mb].any (fun m' => m'.frm == ma.to)) = false := by
    simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff, beq_eq_false_iff_ne]
    exact ⟨hlegA, fun h => hab h.symm⟩
  have hcarDa : carAt b ma.to = false := carAt_to_false_of_not_blocked b [ma, mb] ma hBA hns_da
  have hEda : edgeMap b [ma, mb] ma.to = none := edge_off b ma mb hne ma.to hda_ne_sa hab
  have hcarDb : carAt b mb.to = false := by rw [hba]; exact hvacSa
  have hstopB : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 mb.frm = some ma.to := by
    rw [stopWalk_go (edgeMap b [ma, mb]) (carAt b) 2 mb.frm mb.to hEb hcarDb, hba,
        stopWalk_go (edgeMap b [ma, mb]) (carAt b) 1 ma.frm ma.to hEa hcarDa]
    exact stopWalk_dead (edgeMap b [ma, mb]) (carAt b) 0 ma.to hEda
  have htcB : twoCyc (edgeMap b [ma, mb]) mb.frm = false := by
    simp only [twoCyc, hEb]; rw [hba, hEa]; simp [hab]
  have hda_ne_sb : ma.to ≠ mb.frm := hab
  rw [landMap_pair_eq b ma mb]
  exact landOf_stop_empty (edgeMap b [ma, mb]) (carAt b) 3 2 mb.frm ma.to htcB hstopB hda_ne_sb hcarDa

theorem landMap_pair_stuck_b (hne : ma.frm ≠ mb.frm)
    (hba : mb.to = ma.frm) (hcarSa : carAt b ma.frm = true)
    (hBB : blockedB b [ma, mb] mb = false) (hBA : blockedB b [ma, mb] ma = true) :
    landMap b [ma, mb] mb.frm = mb.frm := by
  have hEb : edgeMap b [ma, mb] mb.frm = some mb.to := by rw [edge_b b ma mb hne, if_neg (by simp [hBB])]
  have hEa0 : edgeMap b [ma, mb] ma.frm = none := by rw [edge_a b ma mb hne, if_pos hBA]
  have hcarDb : carAt b mb.to = true := by rw [hba]; exact hcarSa
  have hdb_ne_sb : mb.to ≠ mb.frm := by rw [hba]; exact hne
  have hstopB : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 mb.frm = some mb.to :=
    stopWalk_stop (edgeMap b [ma, mb]) (carAt b) 2 mb.frm mb.to hEb hcarDb
  have htcB : twoCyc (edgeMap b [ma, mb]) mb.frm = false := by
    simp only [twoCyc, hEb]; rw [hba, hEa0]; simp
  have hleaveDb : leaves (edgeMap b [ma, mb]) (carAt b) 3 2 mb.to = false := by
    rw [hba]; exact leaves_dead (edgeMap b [ma, mb]) (carAt b) 2 1 ma.frm hEa0
  rw [landMap_pair_eq b ma mb]
  exact landOf_stop_stayer (edgeMap b [ma, mb]) (carAt b) 3 2 mb.frm mb.to htcB hstopB hdb_ne_sb hcarDb hleaveDb

theorem landMap_pair_intoEmpty_b (hne : ma.frm ≠ mb.frm)
    (hba : mb.to = ma.frm) (hvacSa : carAt b ma.frm = false)
    (hBB : blockedB b [ma, mb] mb = false) (hBA : blockedB b [ma, mb] ma = true) :
    landMap b [ma, mb] mb.frm = mb.to := by
  have hEb : edgeMap b [ma, mb] mb.frm = some mb.to := by rw [edge_b b ma mb hne, if_neg (by simp [hBB])]
  have hEa0 : edgeMap b [ma, mb] ma.frm = none := by rw [edge_a b ma mb hne, if_pos hBA]
  have hcarDb : carAt b mb.to = false := by rw [hba]; exact hvacSa
  have hdb_ne_sb : mb.to ≠ mb.frm := by rw [hba]; exact hne
  have hstopB : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 mb.frm = some mb.to := by
    rw [stopWalk_go (edgeMap b [ma, mb]) (carAt b) 2 mb.frm mb.to hEb hcarDb, hba]
    exact stopWalk_dead (edgeMap b [ma, mb]) (carAt b) 1 ma.frm hEa0
  have htcB : twoCyc (edgeMap b [ma, mb]) mb.frm = false := by
    simp only [twoCyc, hEb]; rw [hba, hEa0]; simp
  rw [landMap_pair_eq b ma mb]
  exact landOf_stop_empty (edgeMap b [ma, mb]) (carAt b) 3 2 mb.frm mb.to htcB hstopB hdb_ne_sb hcarDb

-- ===== THE B-SIDE LANDING, ALL CASES =====
theorem landMap_pair_b (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to) :
    landMap b [ma, mb] mb.frm =
      (if blockedB b [ma, mb] mb then mb.frm
       else if mb.to = ma.frm then
         (if blockedB b [ma, mb] ma then (if carAt b ma.frm then mb.frm else mb.to)
          else if ma.to = mb.frm then mb.frm
          else (if carAt b ma.frm then mb.to else ma.to))
       else mb.to) := by
  by_cases hBB : blockedB b [ma, mb] mb = true
  · rw [if_pos hBB]; exact landMap_pair_blocked_b b ma mb hne hBB
  · rw [if_neg hBB]; simp only [Bool.not_eq_true] at hBB
    by_cases hba : mb.to = ma.frm
    · rw [if_pos hba]
      by_cases hBA : blockedB b [ma, mb] ma = true
      · rw [if_pos hBA]
        by_cases hcs : carAt b ma.frm = true
        · rw [if_pos hcs]; exact landMap_pair_stuck_b b ma mb hne hba hcs hBB hBA
        · rw [if_neg hcs]; simp only [Bool.not_eq_true] at hcs
          exact landMap_pair_intoEmpty_b b ma mb hne hba hcs hBB hBA
      · rw [if_neg hBA]; simp only [Bool.not_eq_true] at hBA
        by_cases hab : ma.to = mb.frm
        · rw [if_pos hab]; exact landMap_pair_twoCycle_b b ma mb hne hab hba hBA hBB
        · rw [if_neg hab]
          by_cases hcs : carAt b ma.frm = true
          · rw [if_pos hcs]; exact landMap_pair_caterpillar_b b ma mb hne hlegA hba hab hcs hBA hBB
          · rw [if_neg hcs]; simp only [Bool.not_eq_true] at hcs
            exact landMap_pair_flowthrough_b b ma mb hne hlegA hba hab hcs hBA hBB
    · rw [if_neg hba]; exact landMap_pair_indep_b b ma mb hne hlegB hba hBB

-- ===== movers of a pair =====
theorem movers_pair (hne : ma.frm ≠ mb.frm) :
    movers b [ma, mb] =
      [ma.frm, mb.frm].filter (fun c => carAt b c && landMap b [ma, mb] c != c) := by
  unfold movers moverList
  have hnd : ([ma.frm, mb.frm] : List Coord).Nodup := by
    simp only [List.nodup_cons, List.not_mem_nil, not_false_eq_true,
      List.nodup_nil, and_true, List.mem_cons, or_false]
    exact hne
  have : (List.map (·.frm) [ma, mb]).dedup = [ma.frm, mb.frm] := by
    simp only [List.map_cons, List.map_nil]
    exact List.Nodup.dedup hnd
  rw [this]

theorem mem_movers_pair (hne : ma.frm ≠ mb.frm) (c : Coord) :
    c ∈ movers b [ma, mb] ↔
      (c = ma.frm ∨ c = mb.frm) ∧ carAt b c = true ∧ landMap b [ma, mb] c ≠ c := by
  rw [movers_pair b ma mb hne, List.mem_filter]
  simp only [List.mem_cons, List.not_mem_nil, or_false, Bool.and_eq_true, bne_iff_ne]

-- ===== a mover's landing is its own raw dest, unless it flows through the other's vacuum source =====
theorem landMap_mover_a (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hmov : landMap b [ma, mb] ma.frm ≠ ma.frm) :
    landMap b [ma, mb] ma.frm = ma.to
      ∨ (landMap b [ma, mb] ma.frm = mb.to ∧ carAt b mb.frm = false) := by
  by_cases hBA : blockedB b [ma, mb] ma = true
  · exact absurd (landMap_pair_blocked_a b ma mb hne hBA) hmov
  · simp only [Bool.not_eq_true] at hBA
    by_cases hab : ma.to = mb.frm
    · by_cases hBB : blockedB b [ma, mb] mb = true
      · by_cases hcs : carAt b mb.frm = true
        · exact absurd (landMap_pair_stuck_a b ma mb hne hab hcs hBA hBB) hmov
        · simp only [Bool.not_eq_true] at hcs
          exact Or.inl (landMap_pair_intoEmpty_a b ma mb hne hab hcs hBA hBB)
      · simp only [Bool.not_eq_true] at hBB
        by_cases hba : mb.to = ma.frm
        · exact absurd (landMap_pair_twoCycle_a b ma mb hne hab hba hBA hBB) hmov
        · by_cases hcs : carAt b mb.frm = true
          · exact Or.inl (landMap_pair_caterpillar_a b ma mb hne hlegB hab hba hcs hBA hBB)
          · simp only [Bool.not_eq_true] at hcs
            exact Or.inr ⟨landMap_pair_flowthrough_a b ma mb hne hlegB hab hba hcs hBA hBB, hcs⟩
    · exact Or.inl (landMap_pair_indep_a b ma mb hne hlegA hab hBA)

theorem landMap_mover_b (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hmov : landMap b [ma, mb] mb.frm ≠ mb.frm) :
    landMap b [ma, mb] mb.frm = mb.to
      ∨ (landMap b [ma, mb] mb.frm = ma.to ∧ carAt b ma.frm = false) := by
  by_cases hBB : blockedB b [ma, mb] mb = true
  · exact absurd (landMap_pair_blocked_b b ma mb hne hBB) hmov
  · simp only [Bool.not_eq_true] at hBB
    by_cases hba : mb.to = ma.frm
    · by_cases hBA : blockedB b [ma, mb] ma = true
      · by_cases hcs : carAt b ma.frm = true
        · exact absurd (landMap_pair_stuck_b b ma mb hne hba hcs hBB hBA) hmov
        · simp only [Bool.not_eq_true] at hcs
          exact Or.inl (landMap_pair_intoEmpty_b b ma mb hne hba hcs hBB hBA)
      · simp only [Bool.not_eq_true] at hBA
        by_cases hab : ma.to = mb.frm
        · exact absurd (landMap_pair_twoCycle_b b ma mb hne hab hba hBA hBB) hmov
        · by_cases hcs : carAt b ma.frm = true
          · exact Or.inl (landMap_pair_caterpillar_b b ma mb hne hlegA hba hab hcs hBA hBB)
          · simp only [Bool.not_eq_true] at hcs
            exact Or.inr ⟨landMap_pair_flowthrough_b b ma mb hne hlegA hba hab hcs hBA hBB, hcs⟩
    · exact Or.inl (landMap_pair_indep_b b ma mb hne hlegB hba hBB)

/-- With two moves, distinct RAW destinations, the two movers cannot converge — no confluence. -/
theorem landMap_movers_distinct (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to)
    (hlegB : mb.frm ≠ mb.to) (hdd : ma.to ≠ mb.to)
    (hcarA : carAt b ma.frm = true) (hcarB : carAt b mb.frm = true)
    (hmovA : landMap b [ma, mb] ma.frm ≠ ma.frm) (hmovB : landMap b [ma, mb] mb.frm ≠ mb.frm) :
    landMap b [ma, mb] ma.frm ≠ landMap b [ma, mb] mb.frm := by
  have hA : landMap b [ma, mb] ma.frm = ma.to := by
    rcases landMap_mover_a b ma mb hne hlegA hlegB hmovA with h | ⟨_, hc⟩
    · exact h
    · rw [hcarB] at hc; exact absurd hc (by simp)
  have hB : landMap b [ma, mb] mb.frm = mb.to := by
    rcases landMap_mover_b b ma mb hne hlegA hlegB hmovB with h | ⟨_, hc⟩
    · exact h
    · rw [hcarA] at hc; exact absurd hc (by simp)
  rw [hA, hB]; exact hdd

-- ===== clause 2 (non-leaver) is false for a mover: its landing is empty or itself moves =====
theorem clause2_false_a (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hdd : ma.to ≠ mb.to) (hmovA : landMap b [ma, mb] ma.frm ≠ ma.frm) :
    carAt b (landMap b [ma, mb] ma.frm) = false
      ∨ landMap b [ma, mb] (landMap b [ma, mb] ma.frm) ≠ landMap b [ma, mb] ma.frm := by
  by_cases hBA : blockedB b [ma, mb] ma = true
  · exact absurd (landMap_pair_blocked_a b ma mb hne hBA) hmovA
  · simp only [Bool.not_eq_true] at hBA
    by_cases hab : ma.to = mb.frm
    · by_cases hBB : blockedB b [ma, mb] mb = true
      · by_cases hcs : carAt b mb.frm = true
        · exact absurd (landMap_pair_stuck_a b ma mb hne hab hcs hBA hBB) hmovA
        · simp only [Bool.not_eq_true] at hcs
          left; rw [landMap_pair_intoEmpty_a b ma mb hne hab hcs hBA hBB, hab]; exact hcs
      · simp only [Bool.not_eq_true] at hBB
        by_cases hba : mb.to = ma.frm
        · exact absurd (landMap_pair_twoCycle_a b ma mb hne hab hba hBA hBB) hmovA
        · by_cases hcs : carAt b mb.frm = true
          · have hLd : landMap b [ma, mb] ma.to = mb.to := by
              rw [hab]; exact landMap_pair_indep_b b ma mb hne hlegB hba hBB
            right; rw [landMap_pair_caterpillar_a b ma mb hne hlegB hab hba hcs hBA hBB, hLd]
            exact fun h => hdd h.symm
          · simp only [Bool.not_eq_true] at hcs
            left; rw [landMap_pair_flowthrough_a b ma mb hne hlegB hab hba hcs hBA hBB]
            have hns_db : ([ma, mb].any (fun m' => m'.frm == mb.to)) = false := by
              simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff,
                beq_eq_false_iff_ne]
              exact ⟨fun h => hba h.symm, hlegB⟩
            exact carAt_to_false_of_not_blocked b [ma, mb] mb hBB hns_db
    · left; rw [landMap_pair_indep_a b ma mb hne hlegA hab hBA]
      have hns : ([ma, mb].any (fun m' => m'.frm == ma.to)) = false := by
        simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff,
          beq_eq_false_iff_ne]
        exact ⟨hlegA, fun h => hab h.symm⟩
      exact carAt_to_false_of_not_blocked b [ma, mb] ma hBA hns

theorem clause2_false_b (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hdd : ma.to ≠ mb.to) (hmovB : landMap b [ma, mb] mb.frm ≠ mb.frm) :
    carAt b (landMap b [ma, mb] mb.frm) = false
      ∨ landMap b [ma, mb] (landMap b [ma, mb] mb.frm) ≠ landMap b [ma, mb] mb.frm := by
  by_cases hBB : blockedB b [ma, mb] mb = true
  · exact absurd (landMap_pair_blocked_b b ma mb hne hBB) hmovB
  · simp only [Bool.not_eq_true] at hBB
    by_cases hba : mb.to = ma.frm
    · by_cases hBA : blockedB b [ma, mb] ma = true
      · by_cases hcs : carAt b ma.frm = true
        · exact absurd (landMap_pair_stuck_b b ma mb hne hba hcs hBB hBA) hmovB
        · simp only [Bool.not_eq_true] at hcs
          left; rw [landMap_pair_intoEmpty_b b ma mb hne hba hcs hBB hBA, hba]; exact hcs
      · simp only [Bool.not_eq_true] at hBA
        by_cases hab : ma.to = mb.frm
        · exact absurd (landMap_pair_twoCycle_b b ma mb hne hab hba hBA hBB) hmovB
        · by_cases hcs : carAt b ma.frm = true
          · have hLd : landMap b [ma, mb] mb.to = ma.to := by
              rw [hba]; exact landMap_pair_indep_a b ma mb hne hlegA hab hBA
            right; rw [landMap_pair_caterpillar_b b ma mb hne hlegA hba hab hcs hBA hBB, hLd]
            exact hdd
          · simp only [Bool.not_eq_true] at hcs
            left; rw [landMap_pair_flowthrough_b b ma mb hne hlegA hba hab hcs hBA hBB]
            have hns_da : ([ma, mb].any (fun m' => m'.frm == ma.to)) = false := by
              simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff,
                beq_eq_false_iff_ne]
              exact ⟨hlegA, fun h => hab h.symm⟩
            exact carAt_to_false_of_not_blocked b [ma, mb] ma hBA hns_da
    · left; rw [landMap_pair_indep_b b ma mb hne hlegB hba hBB]
      have hns_db : ([ma, mb].any (fun m' => m'.frm == mb.to)) = false := by
        simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff,
          beq_eq_false_iff_ne]
        exact ⟨fun h => hba h.symm, hlegB⟩
      exact carAt_to_false_of_not_blocked b [ma, mb] mb hBB hns_db

-- ===== clause 1 (confluence): each mover is the UNIQUE mover landing where it lands =====
theorem filter_land_singleton_a (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to)
    (hlegB : mb.frm ≠ mb.to) (hdd : ma.to ≠ mb.to) (hcarA : carAt b ma.frm = true)
    (hmovA : landMap b [ma, mb] ma.frm ≠ ma.frm) :
    (movers b [ma, mb]).filter (fun c' => landMap b [ma, mb] c' == landMap b [ma, mb] ma.frm)
      = [ma.frm] := by
  rw [movers_pair b ma mb hne, List.filter_filter]
  have hPa : ((landMap b [ma, mb] ma.frm == landMap b [ma, mb] ma.frm)
      && (carAt b ma.frm && (landMap b [ma, mb] ma.frm != ma.frm))) = true := by
    simp [hcarA, hmovA]
  have hPb : ((landMap b [ma, mb] mb.frm == landMap b [ma, mb] ma.frm)
      && (carAt b mb.frm && (landMap b [ma, mb] mb.frm != mb.frm))) = false := by
    by_cases hcb : carAt b mb.frm = true
    · by_cases hmb : landMap b [ma, mb] mb.frm = mb.frm
      · simp [hmb]
      · have hd := landMap_movers_distinct b ma mb hne hlegA hlegB hdd hcarA hcb hmovA hmb
        simp [beq_eq_false_iff_ne.mpr (fun h => hd h.symm)]
    · simp only [Bool.not_eq_true] at hcb; simp [hcb]
  rw [List.filter_cons, if_pos hPa, List.filter_cons, if_neg (by rw [hPb]; exact Bool.false_ne_true),
    List.filter_nil]

theorem filter_land_singleton_b (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to)
    (hlegB : mb.frm ≠ mb.to) (hdd : ma.to ≠ mb.to) (hcarB : carAt b mb.frm = true)
    (hmovB : landMap b [ma, mb] mb.frm ≠ mb.frm) :
    (movers b [ma, mb]).filter (fun c' => landMap b [ma, mb] c' == landMap b [ma, mb] mb.frm)
      = [mb.frm] := by
  rw [movers_pair b ma mb hne, List.filter_filter]
  have hPb : ((landMap b [ma, mb] mb.frm == landMap b [ma, mb] mb.frm)
      && (carAt b mb.frm && (landMap b [ma, mb] mb.frm != mb.frm))) = true := by
    simp [hcarB, hmovB]
  have hPa : ((landMap b [ma, mb] ma.frm == landMap b [ma, mb] mb.frm)
      && (carAt b ma.frm && (landMap b [ma, mb] ma.frm != ma.frm))) = false := by
    by_cases hca : carAt b ma.frm = true
    · by_cases hma : landMap b [ma, mb] ma.frm = ma.frm
      · simp [hma]
      · have hd := landMap_movers_distinct b ma mb hne hlegA hlegB hdd hca hcarB hma hmovB
        simp [beq_eq_false_iff_ne.mpr hd]
    · simp only [Bool.not_eq_true] at hca; simp [hca]
  rw [List.filter_cons, if_neg (by rw [hPa]; exact Bool.false_ne_true), List.filter_cons,
    if_pos hPb, List.filter_nil]

-- ===== landBad is false on every mover of a legal, distinct-dest pair =====
theorem landBad_false_a (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hdd : ma.to ≠ mb.to) (hInA : b.inBounds ma.to) (hInB : b.inBounds mb.to)
    (hmemA : ma.frm ∈ movers b [ma, mb]) : landBad b [ma, mb] ma.frm = false := by
  obtain ⟨_, hcarA, hmovA⟩ := (mem_movers_pair b ma mb hne ma.frm).mp hmemA
  have hin : b.inBounds (landMap b [ma, mb] ma.frm) := by
    rcases landMap_mover_a b ma mb hne hlegA hlegB hmovA with h | ⟨h, _⟩ <;> rw [h]
    · exact hInA
    · exact hInB
  have hd : decide (b.inBounds (landMap b [ma, mb] ma.frm)) = true := decide_eq_true hin
  unfold landBad
  rw [filter_land_singleton_a b ma mb hne hlegA hlegB hdd hcarA hmovA]
  rcases clause2_false_a b ma mb hne hlegA hlegB hdd hmovA with h2 | h2
  · simp [h2, hd]
  · simp [hd, beq_eq_false_iff_ne.mpr h2]

theorem landBad_false_b (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hdd : ma.to ≠ mb.to) (hInA : b.inBounds ma.to) (hInB : b.inBounds mb.to)
    (hmemB : mb.frm ∈ movers b [ma, mb]) : landBad b [ma, mb] mb.frm = false := by
  obtain ⟨_, hcarB, hmovB⟩ := (mem_movers_pair b ma mb hne mb.frm).mp hmemB
  have hin : b.inBounds (landMap b [ma, mb] mb.frm) := by
    rcases landMap_mover_b b ma mb hne hlegA hlegB hmovB with h | ⟨h, _⟩ <;> rw [h]
    · exact hInB
    · exact hInA
  have hd : decide (b.inBounds (landMap b [ma, mb] mb.frm)) = true := decide_eq_true hin
  unfold landBad
  rw [filter_land_singleton_b b ma mb hne hlegA hlegB hdd hcarB hmovB]
  rcases clause2_false_b b ma mb hne hlegA hlegB hdd hmovB with h2 | h2
  · simp [h2, hd]
  · simp [hd, beq_eq_false_iff_ne.mpr h2]

-- ===== (v) NO CONFLUENCE: a legal distinct-dest pair is ALWAYS resolvable =====
theorem resolvableB_pair (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hdd : ma.to ≠ mb.to) (hInA : b.inBounds ma.to) (hInB : b.inBounds mb.to) :
    resolvableB b [ma, mb] = true := by
  have hfilt : (movers b [ma, mb]).filter (landBad b [ma, mb]) = [] := by
    rw [List.filter_eq_nil_iff]
    intro c hc
    obtain ⟨hcor, _, _⟩ := (mem_movers_pair b ma mb hne c).mp hc
    rcases hcor with rfl | rfl
    · rw [landBad_false_a b ma mb hne hlegA hlegB hdd hInA hInB hc]; exact Bool.false_ne_true
    · rw [landBad_false_b b ma mb hne hlegA hlegB hdd hInA hInB hc]; exact Bool.false_ne_true
  unfold resolvableB unresolved
  rw [hfilt]; rfl

-- ===== cell-wise closed form of resolveMoves for a pair =====
theorem writeBoard_resolveMoves_cell (q : Coord) (hq : b.inBounds q) :
    (resolveMoves b [ma, mb]).cellAt q =
      (if resolvableB b [ma, mb] then
         (match arrivalAt (movers b [ma, mb]) (landMap b [ma, mb]) q with
          | some c => b.cellAt c
          | none   => if memB (movers b [ma, mb]) q then Particle.vacuum else b.cellAt q)
       else b.cellAt q) := by
  unfold resolveMoves
  by_cases hr : resolvableB b [ma, mb] = true
  · rw [if_pos hr, if_pos hr]; exact writeBoard_cellAt b _ _ q hq
  · simp only [Bool.not_eq_true] at hr; rw [hr]; simp

/-- The `resolvableB`-gate collapses: a legal, distinct-dest pair always resolves, so the cell is
the placed/vacated board unconditionally. -/
theorem resolveMoves_cell_pair (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to)
    (hlegB : mb.frm ≠ mb.to) (hdd : ma.to ≠ mb.to) (hInA : b.inBounds ma.to)
    (hInB : b.inBounds mb.to) (q : Coord) (hq : b.inBounds q) :
    (resolveMoves b [ma, mb]).cellAt q =
      (match arrivalAt (movers b [ma, mb]) (landMap b [ma, mb]) q with
       | some c => b.cellAt c
       | none   => if memB (movers b [ma, mb]) q then Particle.vacuum else b.cellAt q) := by
  rw [writeBoard_resolveMoves_cell b ma mb q hq,
    if_pos (resolvableB_pair b ma mb hne hlegA hlegB hdd hInA hInB)]

/-- **THE PAIR ARRIVAL.** On a legal, distinct-dest pair the arrival at `q` is decided by which of the
two sources is a mover landing exactly on `q`: `ma.frm` if it moves and lands on `q`, else `mb.frm` if
it moves and lands on `q`, else nobody (the two movers cannot converge, by `landMap_movers_distinct`,
so the filter is a singleton or empty). This is the reduction the board-cell correspondence rides:
paired with `resolveMoves_cell_pair` it turns the arrival `match` into the same nested `if` the emitted
`cWBoardV4` cellAlgebra collapse produces (mover-A landing → place A; mover-B landing → place B). -/
theorem arrivalAt_pair (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to)
    (hlegB : mb.frm ≠ mb.to) (hdd : ma.to ≠ mb.to) (q : Coord) :
    arrivalAt (movers b [ma, mb]) (landMap b [ma, mb]) q =
      (if ma.frm ∈ movers b [ma, mb] ∧ landMap b [ma, mb] ma.frm = q then some ma.frm
       else if mb.frm ∈ movers b [ma, mb] ∧ landMap b [ma, mb] mb.frm = q then some mb.frm
       else none) := by
  unfold arrivalAt
  by_cases hA : ma.frm ∈ movers b [ma, mb] ∧ landMap b [ma, mb] ma.frm = q
  · obtain ⟨hmemA, hLAq⟩ := hA
    obtain ⟨_, hcarA, hmovA⟩ := (mem_movers_pair b ma mb hne ma.frm).mp hmemA
    have hfil : (movers b [ma, mb]).filter (fun c => landMap b [ma, mb] c == q) = [ma.frm] := by
      rw [← hLAq]; exact filter_land_singleton_a b ma mb hne hlegA hlegB hdd hcarA hmovA
    rw [hfil, if_pos ⟨hmemA, hLAq⟩]; rfl
  · rw [if_neg hA]
    by_cases hB : mb.frm ∈ movers b [ma, mb] ∧ landMap b [ma, mb] mb.frm = q
    · obtain ⟨hmemB, hLBq⟩ := hB
      obtain ⟨_, hcarB, hmovB⟩ := (mem_movers_pair b ma mb hne mb.frm).mp hmemB
      have hfil : (movers b [ma, mb]).filter (fun c => landMap b [ma, mb] c == q) = [mb.frm] := by
        rw [← hLBq]; exact filter_land_singleton_b b ma mb hne hlegA hlegB hdd hcarB hmovB
      rw [hfil, if_pos ⟨hmemB, hLBq⟩]; rfl
    · rw [if_neg hB]
      have hfil : (movers b [ma, mb]).filter (fun c => landMap b [ma, mb] c == q) = [] := by
        rw [List.filter_eq_nil_iff]
        intro c hc heq
        rw [beq_iff_eq] at heq
        obtain ⟨hcor, _, _⟩ := (mem_movers_pair b ma mb hne c).mp hc
        rcases hcor with rfl | rfl
        · exact hA ⟨hc, heq⟩
        · exact hB ⟨hc, heq⟩
      rw [hfil]; rfl

/-! ### The clash ⟺ selection bridge

`clashCoords` (fork/collide adjudication) on a two-move round fires exactly when the pair forks
(shared source, different destinations) or collides (shared destination from two DISTINCT NON-VACUUM
sources). This matches, arm for arm, the circuit's own `cSurv` condition
(`AutomataflResolveCapstone.ResolveFactsN.survIff`): `cSurv = 1 ⟺ clashCoords b [ma,mb] = []`. -/

/-- `forkAt` on a two-move round fires at `s` iff both sources are `s` and the destinations differ. -/
theorem forkAt_pair (s : Coord) :
    forkAt [ma, mb] s = true ↔ (ma.frm = s ∧ mb.frm = s ∧ ma.to ≠ mb.to) := by
  simp only [forkAt, List.any_cons, List.any_nil, Bool.or_false, Bool.and_eq_true,
    beq_iff_eq, bne_iff_ne, Bool.or_eq_true, ne_eq]
  by_cases h1 : ma.frm = s <;> by_cases h2 : mb.frm = s <;> by_cases h3 : ma.to = mb.to <;>
    simp_all

/-- `collideAt` on a two-move round fires at `d` iff both destinations are `d` from two distinct,
non-vacuum sources. -/
theorem collideAt_pair (d : Coord) :
    collideAt b [ma, mb] d = true ↔
      (ma.to = d ∧ mb.to = d ∧ ma.frm ≠ mb.frm
        ∧ carAt b ma.frm = true ∧ carAt b mb.frm = true) := by
  simp only [collideAt, carAt, List.any_cons, List.any_nil, Bool.or_false, Bool.and_eq_true,
    beq_iff_eq, bne_iff_ne, Bool.or_eq_true, ne_eq, Bool.not_eq_true']
  by_cases h1 : ma.to = d <;> by_cases h2 : mb.to = d <;> by_cases h3 : ma.frm = mb.frm <;>
    by_cases h4 : (b.cellAt ma.frm).isVacuum = false <;>
    by_cases h5 : (b.cellAt mb.frm).isVacuum = false <;> simp_all

/-- **THE CLASH BRIDGE.** `clashCoords b [ma,mb]` is nonempty exactly on a fork (shared source,
different destinations) or a collide (shared destination, distinct non-vacuum sources) — arm for arm
the negation of the circuit's `cSurv` survival condition. -/
theorem clashCoords_pair_iff :
    clashCoords b [ma, mb] ≠ [] ↔
      ((ma.frm = mb.frm ∧ ma.to ≠ mb.to)
        ∨ (ma.to = mb.to ∧ ma.frm ≠ mb.frm
            ∧ carAt b ma.frm = true ∧ carAt b mb.frm = true)) := by
  unfold clashCoords
  rw [dedup_ne_nil_iff, ne_nil_iff_exists]
  simp only [List.mem_filter, Bool.or_eq_true, forkAt_pair, collideAt_pair]
  constructor
  · rintro ⟨c, -, hc⟩
    rcases hc with ⟨rfl, hb, hne⟩ | ⟨rfl, hb, hne, ca, cb⟩
    · exact Or.inl ⟨hb.symm, hne⟩
    · exact Or.inr ⟨hb.symm, hne, ca, cb⟩
  · rintro (⟨heq, hne⟩ | ⟨heq, hne, ca, cb⟩)
    · exact ⟨ma.frm, by simp [candidates], Or.inl ⟨rfl, heq.symm, hne⟩⟩
    · exact ⟨ma.to, by simp [candidates], Or.inr ⟨rfl, heq.symm, hne, ca, cb⟩⟩

/-! ### The single-effective-mover collapse (`hdd` replaced by a SEPARATION hypothesis)

Edge `da = db` with one source VACUUM: the two moves share a destination but at most ONE carries a
piece, so at most one is a mover and the two movers cannot converge. The `_sep` lemmas below re-run
the `arrivalAt_pair` / `resolvableB_pair` collapse under the weaker hypothesis
`hsep : ¬(ma.frm ∈ movers ∧ mb.frm ∈ movers)` (NOT both movers) in place of the distinct-destination
`hdd`. This covers BOTH the distinct-dest clean class (where `hdd` gives non-convergence via
`landMap_movers_distinct`) and the shared-dest single-vacuum edge (where a vacuum source is never a
mover). The caterpillar arm of `clause2_false` — the ONLY place the distinct-dest form used `hdd` —
is killed here because a caterpillar makes BOTH pieces movers, contradicting `hsep`. -/

theorem filter_land_singleton_a_sep (hne : ma.frm ≠ mb.frm) (hcarA : carAt b ma.frm = true)
    (hmovA : landMap b [ma, mb] ma.frm ≠ ma.frm)
    (hsep : ¬(ma.frm ∈ movers b [ma, mb] ∧ mb.frm ∈ movers b [ma, mb])) :
    (movers b [ma, mb]).filter (fun c' => landMap b [ma, mb] c' == landMap b [ma, mb] ma.frm)
      = [ma.frm] := by
  rw [movers_pair b ma mb hne, List.filter_filter]
  have hPa : ((landMap b [ma, mb] ma.frm == landMap b [ma, mb] ma.frm)
      && (carAt b ma.frm && (landMap b [ma, mb] ma.frm != ma.frm))) = true := by
    simp [hcarA, hmovA]
  have hPb : ((landMap b [ma, mb] mb.frm == landMap b [ma, mb] ma.frm)
      && (carAt b mb.frm && (landMap b [ma, mb] mb.frm != mb.frm))) = false := by
    by_cases hcb : carAt b mb.frm = true
    · by_cases hmb : landMap b [ma, mb] mb.frm = mb.frm
      · simp [hmb]
      · exfalso; apply hsep
        exact ⟨(mem_movers_pair b ma mb hne ma.frm).mpr ⟨Or.inl rfl, hcarA, hmovA⟩,
               (mem_movers_pair b ma mb hne mb.frm).mpr ⟨Or.inr rfl, hcb, hmb⟩⟩
    · simp only [Bool.not_eq_true] at hcb; simp [hcb]
  rw [List.filter_cons, if_pos hPa, List.filter_cons,
    if_neg (by rw [hPb]; exact Bool.false_ne_true), List.filter_nil]

theorem filter_land_singleton_b_sep (hne : ma.frm ≠ mb.frm) (hcarB : carAt b mb.frm = true)
    (hmovB : landMap b [ma, mb] mb.frm ≠ mb.frm)
    (hsep : ¬(ma.frm ∈ movers b [ma, mb] ∧ mb.frm ∈ movers b [ma, mb])) :
    (movers b [ma, mb]).filter (fun c' => landMap b [ma, mb] c' == landMap b [ma, mb] mb.frm)
      = [mb.frm] := by
  rw [movers_pair b ma mb hne, List.filter_filter]
  have hPb : ((landMap b [ma, mb] mb.frm == landMap b [ma, mb] mb.frm)
      && (carAt b mb.frm && (landMap b [ma, mb] mb.frm != mb.frm))) = true := by simp [hcarB, hmovB]
  have hPa : ((landMap b [ma, mb] ma.frm == landMap b [ma, mb] mb.frm)
      && (carAt b ma.frm && (landMap b [ma, mb] ma.frm != ma.frm))) = false := by
    by_cases hca : carAt b ma.frm = true
    · by_cases hma : landMap b [ma, mb] ma.frm = ma.frm
      · simp [hma]
      · exfalso; apply hsep
        exact ⟨(mem_movers_pair b ma mb hne ma.frm).mpr ⟨Or.inl rfl, hca, hma⟩,
               (mem_movers_pair b ma mb hne mb.frm).mpr ⟨Or.inr rfl, hcarB, hmovB⟩⟩
    · simp only [Bool.not_eq_true] at hca; simp [hca]
  rw [List.filter_cons, if_neg (by rw [hPa]; exact Bool.false_ne_true), List.filter_cons,
    if_pos hPb, List.filter_nil]

theorem clause2_false_a_sep (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hcarA : carAt b ma.frm = true)
    (hsep : ¬(ma.frm ∈ movers b [ma, mb] ∧ mb.frm ∈ movers b [ma, mb]))
    (hmovA : landMap b [ma, mb] ma.frm ≠ ma.frm) :
    carAt b (landMap b [ma, mb] ma.frm) = false
      ∨ landMap b [ma, mb] (landMap b [ma, mb] ma.frm) ≠ landMap b [ma, mb] ma.frm := by
  by_cases hBA : blockedB b [ma, mb] ma = true
  · exact absurd (landMap_pair_blocked_a b ma mb hne hBA) hmovA
  · simp only [Bool.not_eq_true] at hBA
    by_cases hab : ma.to = mb.frm
    · by_cases hBB : blockedB b [ma, mb] mb = true
      · by_cases hcs : carAt b mb.frm = true
        · exact absurd (landMap_pair_stuck_a b ma mb hne hab hcs hBA hBB) hmovA
        · simp only [Bool.not_eq_true] at hcs
          left; rw [landMap_pair_intoEmpty_a b ma mb hne hab hcs hBA hBB, hab]; exact hcs
      · simp only [Bool.not_eq_true] at hBB
        by_cases hba : mb.to = ma.frm
        · exact absurd (landMap_pair_twoCycle_a b ma mb hne hab hba hBA hBB) hmovA
        · by_cases hcs : carAt b mb.frm = true
          · exfalso; apply hsep
            have hmb : landMap b [ma, mb] mb.frm = mb.to :=
              landMap_pair_indep_b b ma mb hne hlegB hba hBB
            exact ⟨(mem_movers_pair b ma mb hne ma.frm).mpr ⟨Or.inl rfl, hcarA, hmovA⟩,
                   (mem_movers_pair b ma mb hne mb.frm).mpr
                     ⟨Or.inr rfl, hcs, by rw [hmb]; exact fun h => hlegB h.symm⟩⟩
          · simp only [Bool.not_eq_true] at hcs
            left; rw [landMap_pair_flowthrough_a b ma mb hne hlegB hab hba hcs hBA hBB]
            have hns_db : ([ma, mb].any (fun m' => m'.frm == mb.to)) = false := by
              simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff,
                beq_eq_false_iff_ne]
              exact ⟨fun h => hba h.symm, hlegB⟩
            exact carAt_to_false_of_not_blocked b [ma, mb] mb hBB hns_db
    · left; rw [landMap_pair_indep_a b ma mb hne hlegA hab hBA]
      have hns : ([ma, mb].any (fun m' => m'.frm == ma.to)) = false := by
        simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff,
          beq_eq_false_iff_ne]
        exact ⟨hlegA, fun h => hab h.symm⟩
      exact carAt_to_false_of_not_blocked b [ma, mb] ma hBA hns

theorem clause2_false_b_sep (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hcarB : carAt b mb.frm = true)
    (hsep : ¬(ma.frm ∈ movers b [ma, mb] ∧ mb.frm ∈ movers b [ma, mb]))
    (hmovB : landMap b [ma, mb] mb.frm ≠ mb.frm) :
    carAt b (landMap b [ma, mb] mb.frm) = false
      ∨ landMap b [ma, mb] (landMap b [ma, mb] mb.frm) ≠ landMap b [ma, mb] mb.frm := by
  by_cases hBB : blockedB b [ma, mb] mb = true
  · exact absurd (landMap_pair_blocked_b b ma mb hne hBB) hmovB
  · simp only [Bool.not_eq_true] at hBB
    by_cases hba : mb.to = ma.frm
    · by_cases hBA : blockedB b [ma, mb] ma = true
      · by_cases hcs : carAt b ma.frm = true
        · exact absurd (landMap_pair_stuck_b b ma mb hne hba hcs hBB hBA) hmovB
        · simp only [Bool.not_eq_true] at hcs
          left; rw [landMap_pair_intoEmpty_b b ma mb hne hba hcs hBB hBA, hba]; exact hcs
      · simp only [Bool.not_eq_true] at hBA
        by_cases hab : ma.to = mb.frm
        · exact absurd (landMap_pair_twoCycle_b b ma mb hne hab hba hBA hBB) hmovB
        · by_cases hcs : carAt b ma.frm = true
          · exfalso; apply hsep
            have hma : landMap b [ma, mb] ma.frm = ma.to :=
              landMap_pair_indep_a b ma mb hne hlegA hab hBA
            exact ⟨(mem_movers_pair b ma mb hne ma.frm).mpr
                     ⟨Or.inl rfl, hcs, by rw [hma]; exact fun h => hlegA h.symm⟩,
                   (mem_movers_pair b ma mb hne mb.frm).mpr ⟨Or.inr rfl, hcarB, hmovB⟩⟩
          · simp only [Bool.not_eq_true] at hcs
            left; rw [landMap_pair_flowthrough_b b ma mb hne hlegA hba hab hcs hBA hBB]
            have hns_da : ([ma, mb].any (fun m' => m'.frm == ma.to)) = false := by
              simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff,
                beq_eq_false_iff_ne]
              exact ⟨hlegA, fun h => hab h.symm⟩
            exact carAt_to_false_of_not_blocked b [ma, mb] ma hBA hns_da
    · left; rw [landMap_pair_indep_b b ma mb hne hlegB hba hBB]
      have hns_db : ([ma, mb].any (fun m' => m'.frm == mb.to)) = false := by
        simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff,
          beq_eq_false_iff_ne]
        exact ⟨fun h => hba h.symm, hlegB⟩
      exact carAt_to_false_of_not_blocked b [ma, mb] mb hBB hns_db

theorem landBad_false_a_sep (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hsep : ¬(ma.frm ∈ movers b [ma, mb] ∧ mb.frm ∈ movers b [ma, mb]))
    (hInA : b.inBounds ma.to) (hInB : b.inBounds mb.to)
    (hmemA : ma.frm ∈ movers b [ma, mb]) : landBad b [ma, mb] ma.frm = false := by
  obtain ⟨_, hcarA, hmovA⟩ := (mem_movers_pair b ma mb hne ma.frm).mp hmemA
  have hin : b.inBounds (landMap b [ma, mb] ma.frm) := by
    rcases landMap_mover_a b ma mb hne hlegA hlegB hmovA with h | ⟨h, _⟩ <;> rw [h]
    · exact hInA
    · exact hInB
  have hd : decide (b.inBounds (landMap b [ma, mb] ma.frm)) = true := decide_eq_true hin
  unfold landBad
  rw [filter_land_singleton_a_sep b ma mb hne hcarA hmovA hsep]
  rcases clause2_false_a_sep b ma mb hne hlegA hlegB hcarA hsep hmovA with h2 | h2
  · simp [h2, hd]
  · simp [hd, beq_eq_false_iff_ne.mpr h2]

theorem landBad_false_b_sep (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hsep : ¬(ma.frm ∈ movers b [ma, mb] ∧ mb.frm ∈ movers b [ma, mb]))
    (hInA : b.inBounds ma.to) (hInB : b.inBounds mb.to)
    (hmemB : mb.frm ∈ movers b [ma, mb]) : landBad b [ma, mb] mb.frm = false := by
  obtain ⟨_, hcarB, hmovB⟩ := (mem_movers_pair b ma mb hne mb.frm).mp hmemB
  have hin : b.inBounds (landMap b [ma, mb] mb.frm) := by
    rcases landMap_mover_b b ma mb hne hlegA hlegB hmovB with h | ⟨h, _⟩ <;> rw [h]
    · exact hInB
    · exact hInA
  have hd : decide (b.inBounds (landMap b [ma, mb] mb.frm)) = true := decide_eq_true hin
  unfold landBad
  rw [filter_land_singleton_b_sep b ma mb hne hcarB hmovB hsep]
  rcases clause2_false_b_sep b ma mb hne hlegA hlegB hcarB hsep hmovB with h2 | h2
  · simp [h2, hd]
  · simp [hd, beq_eq_false_iff_ne.mpr h2]

theorem resolvableB_pair_sep (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to) (hlegB : mb.frm ≠ mb.to)
    (hInA : b.inBounds ma.to) (hInB : b.inBounds mb.to)
    (hsep : ¬(ma.frm ∈ movers b [ma, mb] ∧ mb.frm ∈ movers b [ma, mb])) :
    resolvableB b [ma, mb] = true := by
  have hfilt : (movers b [ma, mb]).filter (landBad b [ma, mb]) = [] := by
    rw [List.filter_eq_nil_iff]
    intro c hc
    obtain ⟨hcor, hcar, _⟩ := (mem_movers_pair b ma mb hne c).mp hc
    rcases hcor with rfl | rfl
    · rw [landBad_false_a_sep b ma mb hne hlegA hlegB hsep hInA hInB hc]; exact Bool.false_ne_true
    · rw [landBad_false_b_sep b ma mb hne hlegA hlegB hsep hInA hInB hc]; exact Bool.false_ne_true
  unfold resolvableB unresolved
  rw [hfilt]; rfl

theorem resolveMoves_cell_pair_sep (hne : ma.frm ≠ mb.frm) (hlegA : ma.frm ≠ ma.to)
    (hlegB : mb.frm ≠ mb.to) (hInA : b.inBounds ma.to) (hInB : b.inBounds mb.to)
    (hsep : ¬(ma.frm ∈ movers b [ma, mb] ∧ mb.frm ∈ movers b [ma, mb]))
    (q : Coord) (hq : b.inBounds q) :
    (resolveMoves b [ma, mb]).cellAt q =
      (match arrivalAt (movers b [ma, mb]) (landMap b [ma, mb]) q with
       | some c => b.cellAt c
       | none   => if memB (movers b [ma, mb]) q then Particle.vacuum else b.cellAt q) := by
  rw [writeBoard_resolveMoves_cell b ma mb q hq,
    if_pos (resolvableB_pair_sep b ma mb hne hlegA hlegB hInA hInB hsep)]

theorem arrivalAt_pair_sep (hne : ma.frm ≠ mb.frm) (_hlegA : ma.frm ≠ ma.to) (_hlegB : mb.frm ≠ mb.to)
    (hsep : ¬(ma.frm ∈ movers b [ma, mb] ∧ mb.frm ∈ movers b [ma, mb])) (q : Coord) :
    arrivalAt (movers b [ma, mb]) (landMap b [ma, mb]) q =
      (if ma.frm ∈ movers b [ma, mb] ∧ landMap b [ma, mb] ma.frm = q then some ma.frm
       else if mb.frm ∈ movers b [ma, mb] ∧ landMap b [ma, mb] mb.frm = q then some mb.frm
       else none) := by
  unfold arrivalAt
  by_cases hA : ma.frm ∈ movers b [ma, mb] ∧ landMap b [ma, mb] ma.frm = q
  · obtain ⟨hmemA, hLAq⟩ := hA
    obtain ⟨_, hcarA, hmovA⟩ := (mem_movers_pair b ma mb hne ma.frm).mp hmemA
    have hfil : (movers b [ma, mb]).filter (fun c => landMap b [ma, mb] c == q) = [ma.frm] := by
      rw [← hLAq]; exact filter_land_singleton_a_sep b ma mb hne hcarA hmovA hsep
    rw [hfil, if_pos ⟨hmemA, hLAq⟩]; rfl
  · rw [if_neg hA]
    by_cases hB : mb.frm ∈ movers b [ma, mb] ∧ landMap b [ma, mb] mb.frm = q
    · obtain ⟨hmemB, hLBq⟩ := hB
      obtain ⟨_, hcarB, hmovB⟩ := (mem_movers_pair b ma mb hne mb.frm).mp hmemB
      have hfil : (movers b [ma, mb]).filter (fun c => landMap b [ma, mb] c == q) = [mb.frm] := by
        rw [← hLBq]; exact filter_land_singleton_b_sep b ma mb hne hcarB hmovB hsep
      rw [hfil, if_pos ⟨hmemB, hLBq⟩]; rfl
    · rw [if_neg hB]
      have hfil : (movers b [ma, mb]).filter (fun c => landMap b [ma, mb] c == q) = [] := by
        rw [List.filter_eq_nil_iff]
        intro c hc heq
        rw [beq_iff_eq] at heq
        obtain ⟨hcor, _, _⟩ := (mem_movers_pair b ma mb hne c).mp hc
        rcases hcor with rfl | rfl
        · exact hA ⟨hc, heq⟩
        · exact hB ⟨hc, heq⟩
      rw [hfil]; rfl

/-! ### The IDENTICAL-move collapse (`ma.frm = mb.frm`, `ma.to = mb.to`)

Two players naming the SAME `(src, dst)` is not a conflict (`clashCoords_pair_iff`: shared source but
`ma.to = mb.to` is neither a fork nor a collide). The reference `dedup`s the two into ONE mover, so the
round behaves as a single piece `s → d`. This is the `hne`-FREE surface the circuit's identical-move
exception (`cIdentV2 = 1 ⇒ cMergeV2` suppressed; both carries fire but agree) matches against. -/

theorem blockedB_identical (hfrm : ma.frm = mb.frm) (hto : ma.to = mb.to) :
    blockedB b [ma, mb] ma = blockedB b [ma, mb] mb := by
  simp only [blockedB, hfrm, hto]

theorem edgeMap_identical (hfrm : ma.frm = mb.frm) (hto : ma.to = mb.to) (c : Coord) :
    edgeMap b [ma, mb] c =
      (if c = ma.frm then (if blockedB b [ma, mb] ma then none else some ma.to) else none) := by
  have hblk := blockedB_identical b ma mb hfrm hto
  unfold edgeMap edgeOf
  by_cases hc : c = ma.frm
  · rw [if_pos hc]
    have h1 : (ma.frm == c) = true := by rw [beq_iff_eq]; exact hc.symm
    have h2 : (mb.frm == c) = true := by rw [beq_iff_eq, ← hfrm]; exact hc.symm
    by_cases hb : blockedB b [ma, mb] ma = true
    · have hbb : blockedB b [ma, mb] mb = true := hblk ▸ hb
      rw [if_pos hb]
      simp [List.filter_cons, h1, h2, hb, hbb, allEqOpt]
    · simp only [Bool.not_eq_true] at hb
      have hbb : blockedB b [ma, mb] mb = false := hblk ▸ hb
      rw [if_neg (by rw [hb]; exact Bool.false_ne_true)]
      simp [List.filter_cons, h1, h2, hb, hbb, hto, allEqOpt]
  · rw [if_neg hc]
    have h1 : (ma.frm == c) = false := by rw [beq_eq_false_iff_ne]; exact fun h => hc h.symm
    have h2 : (mb.frm == c) = false := by rw [beq_eq_false_iff_ne, ← hfrm]; exact fun h => hc h.symm
    simp [List.filter_cons, h1, h2, allEqOpt]

theorem landMap_identical (hfrm : ma.frm = mb.frm) (hto : ma.to = mb.to) (hleg : ma.frm ≠ ma.to) :
    landMap b [ma, mb] ma.frm = (if blockedB b [ma, mb] ma then ma.frm else ma.to) := by
  by_cases hb : blockedB b [ma, mb] ma = true
  · rw [if_pos hb]
    have hE : edgeMap b [ma, mb] ma.frm = none := by
      rw [edgeMap_identical b ma mb hfrm hto, if_pos rfl, if_pos hb]
    rw [landMap_pair_eq b ma mb]
    exact landOf_edge_none (edgeMap b [ma, mb]) (carAt b) 2 3 ma.frm hE
  · simp only [Bool.not_eq_true] at hb
    rw [if_neg (by rw [hb]; exact Bool.false_ne_true)]
    have hEa : edgeMap b [ma, mb] ma.frm = some ma.to := by
      rw [edgeMap_identical b ma mb hfrm hto, if_pos rfl,
        if_neg (by rw [hb]; exact Bool.false_ne_true)]
    have hns : ([ma, mb].any (fun m' => m'.frm == ma.to)) = false := by
      simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff, beq_eq_false_iff_ne]
      exact ⟨hleg, by rw [← hfrm]; exact hleg⟩
    have hcarDa : carAt b ma.to = false := carAt_to_false_of_not_blocked b [ma, mb] ma hb hns
    have hda_ne_sa : ma.to ≠ ma.frm := fun h => hleg h.symm
    have hEda : edgeMap b [ma, mb] ma.to = none := by
      rw [edgeMap_identical b ma mb hfrm hto, if_neg hda_ne_sa]
    have hstop : stopWalk (edgeMap b [ma, mb]) (carAt b) 3 ma.frm = some ma.to := by
      rw [stopWalk_go (edgeMap b [ma, mb]) (carAt b) 2 ma.frm ma.to hEa hcarDa]
      exact stopWalk_dead (edgeMap b [ma, mb]) (carAt b) 1 ma.to hEda
    have htc : twoCyc (edgeMap b [ma, mb]) ma.frm = false := by simp [twoCyc, hEa, hEda]
    rw [landMap_pair_eq b ma mb]
    exact landOf_stop_empty (edgeMap b [ma, mb]) (carAt b) 3 2 ma.frm ma.to htc hstop hda_ne_sa hcarDa

theorem movers_identical (hfrm : ma.frm = mb.frm) (hto : ma.to = mb.to) (hleg : ma.frm ≠ ma.to) :
    movers b [ma, mb] =
      (if carAt b ma.frm = true ∧ blockedB b [ma, mb] ma = false then [ma.frm] else []) := by
  unfold movers moverList
  have hmap : (List.map (·.frm) [ma, mb]).dedup = [ma.frm] := by
    simp only [List.map_cons, List.map_nil]
    rw [List.dedup_cons_of_mem (show ma.frm ∈ [mb.frm] by simp [hfrm]),
      (List.nodup_singleton mb.frm).dedup, hfrm]
  rw [hmap, List.filter_cons, List.filter_nil]
  by_cases hcar : carAt b ma.frm = true
  · by_cases hb : blockedB b [ma, mb] ma = true
    · have hlm : landMap b [ma, mb] ma.frm = ma.frm := by
        rw [landMap_identical b ma mb hfrm hto hleg, if_pos hb]
      rw [if_neg (show ¬(carAt b ma.frm && (landMap b [ma, mb] ma.frm != ma.frm)) = true by
            rw [hlm]; simp),
        if_neg (by rintro ⟨_, h⟩; rw [hb] at h; exact absurd h (by decide))]
    · simp only [Bool.not_eq_true] at hb
      have hlm : landMap b [ma, mb] ma.frm = ma.to := by
        rw [landMap_identical b ma mb hfrm hto hleg, if_neg (by rw [hb]; exact Bool.false_ne_true)]
      rw [if_pos (show (carAt b ma.frm && (landMap b [ma, mb] ma.frm != ma.frm)) = true by
            rw [hlm, hcar]; simp only [Bool.true_and, bne_iff_ne]; exact fun h => hleg h.symm),
        if_pos ⟨hcar, hb⟩]
  · simp only [Bool.not_eq_true] at hcar
    rw [if_neg (show ¬(carAt b ma.frm && (landMap b [ma, mb] ma.frm != ma.frm)) = true by
          rw [hcar]; simp),
      if_neg (by rintro ⟨h, _⟩; rw [hcar] at h; exact absurd h (by decide))]

theorem resolvableB_identical (hfrm : ma.frm = mb.frm) (hto : ma.to = mb.to) (hleg : ma.frm ≠ ma.to)
    (hIn : b.inBounds ma.to) : resolvableB b [ma, mb] = true := by
  unfold resolvableB unresolved
  have hfilt : (movers b [ma, mb]).filter (landBad b [ma, mb]) = [] := by
    rw [List.filter_eq_nil_iff]
    intro c hc
    rw [movers_identical b ma mb hfrm hto hleg] at hc
    by_cases hcond : carAt b ma.frm = true ∧ blockedB b [ma, mb] ma = false
    · rw [if_pos hcond, List.mem_singleton] at hc
      subst hc
      have hb : blockedB b [ma, mb] ma = false := hcond.2
      have hlm : landMap b [ma, mb] ma.frm = ma.to := by
        rw [landMap_identical b ma mb hfrm hto hleg, if_neg (by rw [hb]; exact Bool.false_ne_true)]
      have hns : ([ma, mb].any (fun m' => m'.frm == ma.to)) = false := by
        simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.or_eq_false_iff,
          beq_eq_false_iff_ne]
        exact ⟨hleg, by rw [← hfrm]; exact hleg⟩
      have hcarDa : carAt b ma.to = false := carAt_to_false_of_not_blocked b [ma, mb] ma hb hns
      have hsingle :
          (movers b [ma, mb]).filter (fun c' => landMap b [ma, mb] c' == landMap b [ma, mb] ma.frm)
            = [ma.frm] := by
        rw [movers_identical b ma mb hfrm hto hleg, if_pos hcond]
        simp
      unfold landBad
      rw [hsingle, hlm]
      have hd : decide (b.inBounds ma.to) = true := decide_eq_true hIn
      simp [hcarDa, hd]
    · rw [if_neg hcond] at hc; exact absurd hc List.not_mem_nil
  rw [hfilt]; rfl

theorem arrivalAt_identical (hfrm : ma.frm = mb.frm) (hto : ma.to = mb.to) (hleg : ma.frm ≠ ma.to)
    (q : Coord) :
    arrivalAt (movers b [ma, mb]) (landMap b [ma, mb]) q =
      (if (carAt b ma.frm = true ∧ blockedB b [ma, mb] ma = false) ∧ ma.to = q
       then some ma.frm else none) := by
  unfold arrivalAt
  rw [movers_identical b ma mb hfrm hto hleg]
  by_cases hcond : carAt b ma.frm = true ∧ blockedB b [ma, mb] ma = false
  · rw [if_pos hcond, List.filter_cons, List.filter_nil]
    have hlm : landMap b [ma, mb] ma.frm = ma.to := by
      rw [landMap_identical b ma mb hfrm hto hleg, if_neg (by rw [hcond.2]; exact Bool.false_ne_true)]
    by_cases hq : ma.to = q
    · rw [if_pos (show (landMap b [ma, mb] ma.frm == q) = true by rw [hlm, beq_iff_eq]; exact hq),
        if_pos ⟨hcond, hq⟩]; rfl
    · rw [if_neg (show ¬(landMap b [ma, mb] ma.frm == q) = true by
            rw [hlm]; simp only [beq_iff_eq]; exact hq),
        if_neg (by rintro ⟨_, h⟩; exact hq h)]; rfl
  · rw [if_neg hcond, if_neg (by rintro ⟨h, _⟩; exact hcond h)]; rfl

theorem resolveMoves_cell_identical (hfrm : ma.frm = mb.frm) (hto : ma.to = mb.to)
    (hleg : ma.frm ≠ ma.to) (hIn : b.inBounds ma.to) (q : Coord) (hq : b.inBounds q) :
    (resolveMoves b [ma, mb]).cellAt q =
      (if carAt b ma.frm = true ∧ blockedB b [ma, mb] ma = false then
        (if q = ma.to then b.cellAt ma.frm else if q = ma.frm then Particle.vacuum else b.cellAt q)
       else b.cellAt q) := by
  rw [writeBoard_resolveMoves_cell b ma mb q hq,
    if_pos (resolvableB_identical b ma mb hfrm hto hleg hIn),
    arrivalAt_identical b ma mb hfrm hto hleg q,
    movers_identical b ma mb hfrm hto hleg]
  by_cases hcond : carAt b ma.frm = true ∧ blockedB b [ma, mb] ma = false
  · rw [if_pos hcond, if_pos hcond]
    by_cases hqd : q = ma.to
    · rw [if_pos (show (carAt b ma.frm = true ∧ blockedB b [ma, mb] ma = false) ∧ ma.to = q
            from ⟨hcond, hqd.symm⟩)]
      simp [hqd]
    · rw [if_neg (show ¬((carAt b ma.frm = true ∧ blockedB b [ma, mb] ma = false) ∧ ma.to = q)
            from fun h => hqd h.2.symm)]
      by_cases hqs : q = ma.frm
      · simp [memB, hqs, hleg]
      · have hqs' : ¬ ma.frm = q := fun h => hqs h.symm
        simp [memB, hqd, hqs, hqs']
  · rw [if_neg hcond,
      if_neg (show ¬((carAt b ma.frm = true ∧ blockedB b [ma, mb] ma = false) ∧ ma.to = q)
        from fun h => hcond h.1)]
    simp [memB, hcond]

-- The pair arrival reduction (the mover-landing decision the board-cell correspondence rides).
#assert_axioms arrivalAt_pair
-- The clash bridge and its two `any`-expansion helpers, kernel-clean.
#assert_axioms forkAt_pair
#assert_axioms collideAt_pair
#assert_axioms clashCoords_pair_iff
-- The single-effective-mover collapse (shared-dest single-vacuum edge), kernel-clean.
#assert_axioms resolveMoves_cell_pair_sep
#assert_axioms arrivalAt_pair_sep
#assert_axioms resolvableB_pair_sep
-- The identical-move collapse (shared source AND destination), kernel-clean.
#assert_axioms resolveMoves_cell_identical
#assert_axioms arrivalAt_identical
#assert_axioms movers_identical
#assert_axioms landMap_identical

end PairCollapse

/-! ## §10  ⚑ CONFORMANCE TEST BLOCK — statements HERE, evaluation in `AutomataflRulesFixtures`

One named pin per rules clause, keyed to the audit's divergence table
(`docs/reference/AUTOMATAFL-RULES-CONFORMANCE-AUDIT.md` §A). Every witness the audit's probe
`AUTOMATAFL-RULES-CONFORMANCE-PROBE.lean` used to EXHIBIT a divergence appears behaving
per the rules — D1, D2, D3, D4a, D4b, D5, D6 — so each fixed divergence has a falsifier that
would go red if the fix regressed.

⚑ **THE PINS NO LONGER EVALUATE IN THIS MODULE (2026-08-08).** This module is in the
`Dregg2.FFI` closure — the crypto archive's build — and the 110 `#guard`s that used to run at
elaboration here made every game-fixture regression a hard failure of every Rust proving
target (the compilation-unit coupling the stale-fixture outage measured). The witness boards
and moves STAY here (a `def` body elaborates without running); the EVALUATION — each fact as a
NAMED theorem per GUARD-DISCIPLINE, pinned by `native_decide` + `#assert_compiled` — lives in
`AutomataflRulesFixtures.lean`, rooted in the central guard library: a plain `lake build`
still runs every pin, and a stale fixture reds the guard library instead of the archive.
Facts that mention the private helpers (`mv`, `cfgCol`/`cfgRow`/`cfgFrz`, `noGoals`) stay
expressible through the public evaluation-free `check_* : Bool` definitions below, each
pinned `= true` in the Fixtures module. -/

section Conformance

/-- A move, positionally: `mv who from to`. (Mathlib's token table makes `to := …` in
structure-instance syntax unparseable, so the witnesses use the anonymous constructor.) -/
private def mv (w : Pid) (a d : Coord) : Move := ⟨w, a, d⟩

private def cfgCol : GameConfig := { tieBreak := .column }
private def cfgRow : GameConfig := { tieBreak := .row }
private def cfgFrz : GameConfig := { tieBreak := .freeze }

private def noGoals : GoalAssignment := ⟨[]⟩

/-! ### 1.4 — the automaton square is banned as a SOURCE only (ruling D) -/

def autoBoard : Board := mkBoard 3 [(⟨0, 2⟩, .attractor)] ⟨2, 2⟩
def intoAuto : Move := mv 0 ⟨0, 2⟩ ⟨2, 2⟩
def outOfAuto : Move := mv 0 ⟨2, 2⟩ ⟨0, 2⟩

-- Pinned in `AutomataflRulesFixtures`: destination-legal, blocked, replaced at origin,
-- source-illegal (model.py `POS_CANT_MOVE_THAT`), and 1.5's marked-coordinate refusals.

/-! ### 3.2 (audit D1, CRIT) — the destination is ON the path: a non-moving piece there
BLOCKS, and the mover is replaced at its origin. The old spec DELETED the occupant, with one
move on a 3x3 board. -/

def d1Board : Board := mkBoard 3 [(⟨0, 0⟩, .attractor), (⟨0, 2⟩, .repulsor)] ⟨2, 2⟩
def d1Move : Move := mv 0 ⟨0, 0⟩ ⟨0, 2⟩

-- Pinned in `AutomataflRulesFixtures`: legal, blocked (WAS false — exclusive path), both
-- pieces in place after resolution, and the repulsor still SOMEWHERE on the board.

/-! ### 3.3 — chains continue through vacated / vacuum squares -/

def chainBoard : Board := mkBoard 5 [(⟨0, 0⟩, .attractor)] ⟨4, 4⟩
def ch1 : Move := mv 0 ⟨0, 0⟩ ⟨0, 1⟩
def ch2 : Move := mv 1 ⟨0, 1⟩ ⟨0, 2⟩   -- source is VACUUM

/-! ### 3.4 — the ERRATUM / caterpillar: a piece landing on another move's source
participates in that move too, one edge each. (Both pinned in `AutomataflRulesFixtures`.) -/

def catBoard : Board := mkBoard 5 [(⟨0, 0⟩, .attractor), (⟨0, 1⟩, .repulsor)] ⟨4, 4⟩
def cat1 : Move := mv 0 ⟨0, 0⟩ ⟨0, 1⟩
def cat2 : Move := mv 1 ⟨0, 1⟩ ⟨0, 2⟩

/-! ### 3.6 + the author's rule — a move whose destination does NOT empty simply FAILS TO
EXECUTE ("it doesn't generate a conflict and shouldn't"), and the failure propagates back down
the chain: the leader is blocked, so the follower stays too. -/

def stuckBoard : Board :=
  mkBoard 5 [(⟨0, 0⟩, .attractor), (⟨0, 1⟩, .repulsor), (⟨0, 2⟩, .attractor)] ⟨4, 4⟩
def st1 : Move := mv 0 ⟨0, 0⟩ ⟨0, 1⟩
def st2 : Move := mv 1 ⟨0, 1⟩ ⟨0, 2⟩       -- (0,2) holds a NON-MOVING piece

-- Pinned in `AutomataflRulesFixtures`: leader blocked, follower clear, no clash, no merge,
-- no movers, all three pieces in place — and the ordinary rounds still resolvable.

/-! ### 3.5a (audit D2) — a 2-cycle with pieces on BOTH squares STAYS PUT.
The old spec SWAPPED them. (Pinned in `AutomataflRulesFixtures`.) -/

def d2Board : Board := mkBoard 3 [(⟨0, 0⟩, .attractor), (⟨0, 2⟩, .repulsor)] ⟨2, 2⟩
def d2A : Move := mv 0 ⟨0, 0⟩ ⟨0, 2⟩
def d2B : Move := mv 1 ⟨0, 2⟩ ⟨0, 0⟩

/-! ### 3.5b (audit D3) — the README's own named case: *"a move from an empty square directly
back to some source square — the piece simply doesn't move"*. The old spec moved it.
(Pinned in `AutomataflRulesFixtures`.) -/

def d3Board : Board := mkBoard 3 [(⟨0, 0⟩, .attractor)] ⟨2, 2⟩
def d3A : Move := mv 0 ⟨0, 0⟩ ⟨0, 2⟩
def d3B : Move := mv 1 ⟨0, 2⟩ ⟨0, 0⟩      -- source is VACUUM

/-! ### 3.5c (audit D6) — an EMPTY cycle cannot pull a piece in; the move is nullified.
(Pinned in `AutomataflRulesFixtures`.) -/

def d6Board : Board := mkBoard 5 [(⟨0, 0⟩, .attractor)] ⟨4, 4⟩
def e1 : Move := mv 0 ⟨0, 0⟩ ⟨0, 1⟩
def e2 : Move := mv 1 ⟨0, 1⟩ ⟨0, 2⟩
def e3 : Move := mv 2 ⟨0, 2⟩ ⟨0, 1⟩

/-! ### 3.5d — a >2-cycle with every square carrying ROTATES one position (KEPT).
(Pinned in `AutomataflRulesFixtures`.) -/

def rotBoard : Board :=
  mkBoard 5 [(⟨0, 0⟩, .attractor), (⟨2, 0⟩, .repulsor),
             (⟨2, 2⟩, .attractor), (⟨0, 2⟩, .repulsor)] ⟨4, 4⟩
def r1 : Move := mv 0 ⟨0, 0⟩ ⟨2, 0⟩
def r2 : Move := mv 1 ⟨2, 0⟩ ⟨2, 2⟩
def r3 : Move := mv 2 ⟨2, 2⟩ ⟨0, 2⟩
def r4 : Move := mv 3 ⟨0, 2⟩ ⟨0, 0⟩

/-! ### 2.1 / 2.2 / 2.3 — fork, collide, and the identical-move EXCEPTION (KEPT).
(Pinned in `AutomataflRulesFixtures`; the collide witness needs the private `mv`, so it is a
`check_*` below.) -/

def forkBoard : Board := mkBoard 5 [(⟨0, 0⟩, .attractor)] ⟨4, 4⟩
def fkA : Move := mv 0 ⟨0, 0⟩ ⟨0, 3⟩
def fkB : Move := mv 1 ⟨0, 0⟩ ⟨3, 0⟩
def fkSame : Move := mv 1 ⟨0, 0⟩ ⟨0, 3⟩   -- IDENTICAL to fkA

def collBoard : Board :=
  mkBoard 5 [(⟨0, 0⟩, .attractor), (⟨4, 0⟩, .attractor), (⟨2, 0⟩, .repulsor)] ⟨4, 4⟩

/-- 2.2: ≥2 moves into `(2,0)` from distinct NON-VACUUM sources collide.
(Pinned `= true` in `AutomataflRulesFixtures`.) -/
def check_collide_detected_at_shared_destination : Bool :=
  collideAt collBoard
    [mv 0 ⟨0, 0⟩ ⟨2, 0⟩,
     mv 1 ⟨4, 0⟩ ⟨2, 0⟩] ⟨2, 0⟩

/-! ### 2.4a–d (audit D4a / D4b) — RE-ENTRY, LOCKING, the marked coordinate, and RECURSION.

`Automatafl.lean` dropped a move only if *its own* source was fork-conflicted or *its own*
destination collide-conflicted, so a third move merely MENTIONING the conflicted coordinate
survived and executed. Both witnesses now behave per the rules. -/

-- D4a: destination conflict at (2,0); a third move uses (2,0) as its SOURCE.
def d4A : Move := mv 0 ⟨0, 0⟩ ⟨2, 0⟩
def d4B : Move := mv 1 ⟨4, 0⟩ ⟨2, 0⟩
def d4C : Move := mv 2 ⟨2, 0⟩ ⟨2, 4⟩

/-- D4a: the round demands re-entry. (Pinned `= true` in `AutomataflRulesFixtures`.) -/
def check_d4a_round_demands_reentry : Bool :=
  (roundStep cfgCol noGoals (openRound collBoard [0, 1, 2]) [d4A, d4B, d4C]).isAgain

/-- D4a: the contested destination is MARKED. (Pinned in `AutomataflRulesFixtures`.) -/
def check_d4a_marks_the_contested_destination : Bool :=
  decide ((roundStep cfgCol noGoals (openRound collBoard [0, 1, 2]) [d4A, d4B, d4C]).marks
    = [(⟨2, 0⟩ : Coord)])

/-- D4a: the third move is INVALIDATED (it was `conflictResolve = [d4C]` and it EXECUTED).
(Pinned in `AutomataflRulesFixtures`.) -/
def check_d4a_invalidates_every_touching_move : Bool :=
  decide ((roundStep cfgCol noGoals (openRound collBoard [0, 1, 2]) [d4A, d4B, d4C]).locked
    = ([] : List Move))

/-- D4a: all three seats re-enter. (Pinned in `AutomataflRulesFixtures`.) -/
def check_d4a_all_three_seats_reenter : Bool :=
  decide ((roundStep cfgCol noGoals (openRound collBoard [0, 1, 2]) [d4A, d4B, d4C]).waiting
    = [0, 1, 2])

-- D4b: fork conflict at (0,0); a third move TARGETS (0,0).
def d5Board : Board := mkBoard 5 [(⟨0, 0⟩, .attractor), (⟨0, 4⟩, .repulsor)] ⟨4, 4⟩
def d5A : Move := mv 0 ⟨0, 0⟩ ⟨0, 2⟩
def d5B : Move := mv 1 ⟨0, 0⟩ ⟨2, 0⟩
def d5C : Move := mv 2 ⟨0, 4⟩ ⟨0, 0⟩

/-- D4b: the forked source is MARKED. (Pinned in `AutomataflRulesFixtures`.) -/
def check_d4b_marks_the_forked_source : Bool :=
  decide ((roundStep cfgCol noGoals (openRound d5Board [0, 1, 2]) [d5A, d5B, d5C]).marks
    = [(⟨0, 0⟩ : Coord)])

/-- D4b: the third move (merely TARGETING the conflicted coordinate) is invalidated too.
(Pinned in `AutomataflRulesFixtures`.) -/
def check_d4b_invalidates_every_touching_move : Bool :=
  decide ((roundStep cfgCol noGoals (openRound d5Board [0, 1, 2]) [d5A, d5B, d5C]).locked
    = ([] : List Move))

/-- D4b: all three seats re-enter. (Pinned in `AutomataflRulesFixtures`.) -/
def check_d4b_all_three_seats_reenter : Bool :=
  decide ((roundStep cfgCol noGoals (openRound d5Board [0, 1, 2]) [d5A, d5B, d5C]).waiting
    = [0, 1, 2])

-- LOCKING: a player not involved in the conflict has their move STAND, and does not re-enter.
def lockBoard : Board :=
  mkBoard 5 [(⟨0, 0⟩, .attractor), (⟨4, 0⟩, .attractor), (⟨0, 4⟩, .repulsor)] ⟨4, 4⟩
def lkC : Move := mv 2 ⟨4, 0⟩ ⟨4, 2⟩

/-- LOCKING: the uninvolved seat's move STANDS. (Pinned in `AutomataflRulesFixtures`.) -/
def check_uninvolved_move_is_locked : Bool :=
  decide ((roundStep cfgCol noGoals (openRound lockBoard [0, 1, 2]) [d5A, d5B, lkC]).locked
    = [lkC])

/-- LOCKING: only the conflicted seats re-enter. (Pinned in `AutomataflRulesFixtures`.) -/
def check_only_conflicted_seats_reenter : Bool :=
  decide ((roundStep cfgCol noGoals (openRound lockBoard [0, 1, 2]) [d5A, d5B, lkC]).waiting
    = [0, 1])

-- RECURSION (2.4d): a marked square is illegal next round, and a clean second round RESOLVES.
def reenter : Move := mv 0 ⟨0, 4⟩ ⟨0, 2⟩
def stillMarked : Move := mv 1 ⟨0, 0⟩ ⟨3, 0⟩  -- source still marked

/-- 2.4d: the clean second round RESOLVES, moving the re-entered piece.
(Pinned in `AutomataflRulesFixtures`.) -/
def check_second_round_resolves_the_reentered_move : Bool :=
  decide (((runTurn cfgCol noGoals (openRound d5Board [0, 1, 2])
      [[d5A, d5B, d5C], [reenter, stillMarked]]).map
      (fun r => r.1.cellAt ⟨0, 2⟩)) = some Particle.repulsor)

/-- 2.4d: the still-marked source's move FAILS next round — its piece stays.
(Pinned in `AutomataflRulesFixtures`.) -/
def check_marked_source_move_fails_next_round : Bool :=
  decide (((runTurn cfgCol noGoals (openRound d5Board [0, 1, 2])
      [[d5A, d5B, d5C], [reenter, stillMarked]]).map
      (fun r => r.1.cellAt ⟨0, 0⟩)) = some Particle.attractor)

/-- 2.4d: one round is NOT enough — the turn is still awaiting re-entry.
(Pinned in `AutomataflRulesFixtures`.) -/
def check_one_round_is_not_enough : Bool :=
  decide ((runTurn cfgCol noGoals (openRound d5Board [0, 1, 2]) [[d5A, d5B, d5C]]).isSome
    = false)

/-! ### 3.7 (audit D5, CRIT) — a vacuum CONFLUENCE is now a CONFLICT.

Two chains converge on (2,0) through vacuum waypoints. Neither the fork nor the collide
clause fires (both waypoint sources are vacuum), and the old spec awarded the square by
MOVE-LIST ORDER and DELETED the loser — the permutation `[m3,m4,m1,m2]` produced a different
board, which is what refuted `FairnessObligation`. Here it is detected and conflicted. -/

def mgBoard : Board := mkBoard 5 [(⟨0, 0⟩, .attractor), (⟨4, 0⟩, .repulsor)] ⟨4, 4⟩
def m1 : Move := mv 0 ⟨0, 0⟩ ⟨1, 0⟩
def m2 : Move := mv 1 ⟨1, 0⟩ ⟨2, 0⟩   -- vacuum source
def m3 : Move := mv 2 ⟨4, 0⟩ ⟨3, 0⟩
def m4 : Move := mv 3 ⟨3, 0⟩ ⟨2, 0⟩   -- vacuum source

-- Pinned in `AutomataflRulesFixtures`: fork/collide silent, the merge clause fires at (2,0),
-- the round is not resolvable, no piece destroyed under EITHER order. The three `roundStep`
-- facts need the private configs, so they are `check_*`s:

/-- 3.7: the confluence is MARKED. (Pinned in `AutomataflRulesFixtures`.) -/
def check_merge_marks_the_confluence : Bool :=
  decide ((roundStep cfgCol noGoals (openRound mgBoard [0, 1, 2, 3]) [m1, m2, m3, m4]).marks
    = [(⟨2, 0⟩ : Coord)])

/-- 3.7: the two moves not touching the confluence are LOCKED.
(Pinned in `AutomataflRulesFixtures`.) -/
def check_merge_locks_the_uninvolved_moves : Bool :=
  decide ((roundStep cfgCol noGoals (openRound mgBoard [0, 1, 2, 3]) [m1, m2, m3, m4]).locked
    = [m1, m3])

/-- 3.7: exactly the two confluence seats re-enter. (Pinned in `AutomataflRulesFixtures`.) -/
def check_merge_reenters_the_involved_seats : Bool :=
  decide ((roundStep cfgCol noGoals (openRound mgBoard [0, 1, 2, 3]) [m1, m2, m3, m4]).waiting
    = [1, 3])

/-! ### 4.x — the automaton, unchanged; 4.5 — the tie-break is SELECTABLE (ruling B) -/

def tieBoard : Board := mkBoard 5 [(⟨2, 4⟩, .attractor), (⟨4, 2⟩, .attractor)] ⟨2, 2⟩

-- The axes-compare-equal fact is public and pinned directly in `AutomataflRulesFixtures`;
-- the stepped positions need the private configs, so they are `check_*`s:

/-- 4.5: `column` breaks the equal-priority tie along Y. (Pinned in `AutomataflRulesFixtures`.) -/
def check_column_tiebreak_steps_along_y : Bool :=
  decide ((automatonStepCfg cfgCol tieBoard).automaton = (⟨2, 3⟩ : Coord))

/-- 4.5: `row` breaks it along X. (Pinned in `AutomataflRulesFixtures`.) -/
def check_row_tiebreak_steps_along_x : Bool :=
  decide ((automatonStepCfg cfgRow tieBoard).automaton = (⟨3, 2⟩ : Coord))

/-- 4.5: `freeze` does not move. (Pinned in `AutomataflRulesFixtures`.) -/
def check_freeze_tiebreak_stays_put : Bool :=
  decide ((automatonStepCfg cfgFrz tieBoard).automaton = (⟨2, 2⟩ : Coord))

-- the priority cascade itself (unchanged from `Automatafl.lean`)
def demoBoard : Board := mkBoard 5 [(⟨2, 4⟩, .attractor)] ⟨2, 2⟩
def repBoard : Board := mkBoard 5 [(⟨2, 1⟩, .repulsor)] ⟨2, 2⟩

/-- 4.x: one step toward the attractor. (Pinned in `AutomataflRulesFixtures`.) -/
def check_demo_step_toward_attractor : Bool :=
  decide ((automatonStepCfg cfgCol demoBoard).automaton = (⟨2, 3⟩ : Coord))

/-- 4.x: flee the repulsor. (Pinned in `AutomataflRulesFixtures`.) -/
def check_rep_step_flees_repulsor : Bool :=
  decide ((automatonStepCfg cfgCol repBoard).automaton = (⟨2, 3⟩ : Coord))

/-- The default config IS the old automaton (the Leg-A bridge, witnessed on `demoBoard`).
(Pinned in `AutomataflRulesFixtures`.) -/
def check_default_config_is_old_automaton_demo : Bool :=
  decide ((automatonStepCfg cfgCol demoBoard).automaton = (automatonStep demoBoard).automaton)

/-- The Leg-A bridge witnessed on the tie board too. (Pinned in `AutomataflRulesFixtures`.) -/
def check_default_config_is_old_automaton_tie : Bool :=
  decide ((automatonStepCfg cfgCol tieBoard).automaton = (automatonStep tieBoard).automaton)

/-! ### 5.1 / 5.2 — SETUP: two corners in the same row each / one corner each.
(All six pinned in `AutomataflRulesFixtures` — well-formedness both polarities, the deployed
corner-for-corner assignment, and the seat list at the deployed size.) -/

/-! ### 5.4 — the win fires on the automaton MOVING INTO a corner, not sitting on one.

`Automatafl.lean`'s own witness read `winner demoBoard [(⟨2,2⟩, 7)] = some 7` — a win
on a board where the automaton had not moved at all. It FLIPS. The no-move refusal is public
and pinned directly; the two entry facts step through the private `cfgCol`: -/

/-- 5.4: the win fires on ENTRY of the goal square. (Pinned in `AutomataflRulesFixtures`.) -/
def check_win_fires_on_entry : Bool :=
  decide (winOnEntry demoBoard (automatonStepCfg cfgCol demoBoard) ⟨[(⟨2, 3⟩, 3)]⟩ = some 3)

/-- 5.4: no win on the VACATED square. (Pinned in `AutomataflRulesFixtures`.) -/
def check_no_win_on_vacated_square : Bool :=
  decide (winOnEntry demoBoard (automatonStepCfg cfgCol demoBoard) ⟨[(⟨2, 2⟩, 3)]⟩ = none)

/-! ### The stock 11x11 opening — orientation pinned by raycast.
(Cell and raycast pins are public and live in `AutomataflRulesFixtures`; the symmetric-freeze
fact steps through the private `cfgCol`.) -/

/-- The opening is symmetric: all four rays are equidistant repulsors, so nothing moves.
(Pinned in `AutomataflRulesFixtures`.) -/
def check_stock_opening_freezes_the_automaton : Bool :=
  decide ((automatonStepCfg cfgCol stockTwoPlayer).automaton = (⟨5, 5⟩ : Coord))

/-! ### §10.10 — the m = 2 collapse closed forms at the audit witnesses (non-vacuity).
(The direct-computation pins — `landMap`/`movers`/`resolvableB` at the caterpillar,
flowthrough, occluded-stayer, 2-cycle and blocked-independent witnesses — are all public and
live in `AutomataflRulesFixtures`.) -/

-- the closed-form THEOREMS fire at the witnesses (hypotheses satisfiable ⇒ non-vacuous)
example : landMap catBoard [cat1, cat2] cat1.frm = cat1.to :=
  landMap_pair_caterpillar_a catBoard cat1 cat2 (by decide) (by decide) (by decide) (by decide)
    (by decide) (by decide) (by decide)
example : landMap chainBoard [ch1, ch2] ch1.frm = ch2.to :=
  landMap_pair_flowthrough_a chainBoard ch1 ch2 (by decide) (by decide) (by decide) (by decide)
    (by decide) (by decide) (by decide)
example : landMap stuckBoard [st1, st2] st1.frm = st1.frm :=
  landMap_pair_stuck_a stuckBoard st1 st2 (by decide) (by decide) (by decide) (by decide) (by decide)
example : landMap d2Board [d2A, d2B] d2A.frm = d2A.frm :=
  landMap_pair_twoCycle_a d2Board d2A d2B (by decide) (by decide) (by decide) (by decide) (by decide)
example : resolvableB catBoard [cat1, cat2] = true :=
  resolvableB_pair catBoard cat1 cat2 (by decide) (by decide) (by decide) (by decide) (by decide)
    (by decide)


end Conformance

/-! ## §11  Axiom hygiene -/

#assert_all_clean [
  moveLegalB_iff,
  adjudicate_seated,
  adjudicate_sound,
  adjudicate_draw_level,
  stock_opening_adjudicates_draw,
  adjudicate_decisive_witness,
  adjudicate_decisive_witness_mirror,
  adjudicate_midline_draws,
  chooseOffsetCfg_mem,
  chooseOffsetCfg_column,
  automatonOffsetCfg_column,
  automatonStepCfg_column_eq,
  automatonOffsetCfg_bounded,
  automatonStepCfg_preserves_inBounds,
  winOnEntry_sound,
  winOnEntry_corner,
  stockGoals2_seats,
  stockGoals2_seats_contains_zero,
  stockGoals2_seats_contains_one,
  landMap_of_not_src,
  landMap_of_not_mover,
  filter_land_eq_singleton,
  resolve_conserves,
  allEqOpt_perm,
  uniqueOf_perm,
  edgeMap_perm,
  landMap_perm,
  movers_perm,
  resolvableB_perm,
  resolve_perm,
  clashCoords_perm,
  edgeMap_pair,
  landMap_pair_a,
  landMap_pair_b,
  movers_pair,
  landMap_movers_distinct,
  resolvableB_pair,
  writeBoard_resolveMoves_cell,
  resolveMoves_cell_pair
]

end Dregg2.Games.AutomataflRules
