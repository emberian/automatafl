/-
# AutomataflFFI — the wire-oracle teeth EVALUATION, out of the crypto archive's build

`AutomataflFFI.lean` sits in the `Dregg2.FFI` closure (the crypto archive's build root), and
until 2026-08-08 its §5 teeth ran 52 `#guard` wire pins at elaboration — so any game-fixture
regression was a hard failure of every Rust proving target in the workspace. The witness boards
(`demoBoard`, `cycBoard`, `blockBoard`) remain in `AutomataflFFI.lean`; THIS module is where the
pins are RUN — each former `#guard` as a NAMED theorem per GUARD-DISCIPLINE, proved by
`native_decide` and pinned `#assert_compiled`. Rooted in the central guard library and reachable
from `Dregg2.FFI` by NOTHING, so a plain `lake build` still evaluates every pin while
`lake build Dregg2.FFI` never does. Every pinned expression mentions only PUBLIC names, so the
expressions moved verbatim; the section narratives moved with them.
-/
import Automatafl.FFI
open Automatafl.Tactics


namespace Dregg2.Games.AutomataflFFI

open Dregg2.Games.Automatafl
open Dregg2.Games.AutomataflRules

/-! ### The codec and the stock objects -/

/-- The codec round-trips a real board: decode ∘ encode is the identity on the wire. -/
theorem mid_roundtrips_demo_board :
    (rulesFFI ("mid " ++ encodeBoard demoBoard ++ " 0 0")
      == "1 " ++ encodeBoard demoBoard) = true := by native_decide

theorem mid_roundtrips_stock_board :
    (rulesFFI ("mid " ++ encodeBoard stockTwoPlayer ++ " 0 0")
      == "1 " ++ encodeBoard stockTwoPlayer) = true := by native_decide

/-- The stock board is the 11×11 two-player opening, automaton dead centre at (5,5). -/
theorem stock_verb_is_the_eleven_by_eleven_opening :
    (rulesFFI "stock" ==
      "1 11 " ++
      "11001110011" ++   -- y = 0   `rroorrroorr`
      "00021112000" ++   -- y = 1   `oooarrraooo`
      "00000000000" ++
      "00000000000" ++
      "22000000022" ++   -- y = 4   `aaoooooooaa`
      "11000300011" ++   -- y = 5   `rrooodooorr` — the automaton dead centre
      "22000000022" ++   -- y = 6
      "00000000000" ++
      "00000000000" ++
      "00021112000" ++   -- y = 9
      "11001110011" ++   -- y = 10
      " 5 5 1") = true := by native_decide

/-- The stock two-player goals: seat 0 owns the `y = 0` corners, seat 1 the `y = 10` corners. -/
theorem goals_verb_is_stock_two_player :
    (rulesFFI "goals 11" == "1 4 0 0 0 10 0 0 0 10 1 10 10 1") = true := by native_decide

/-- §8's demo step: the automaton walks one square toward the attractor, (2,2) → (2,3). -/
theorem step_verb_walks_toward_attractor :
    (rulesFFI ("step 0 " ++ encodeBoard demoBoard) ==
      "1 " ++ encodeBoard (mkBoard 5 [(⟨2, 4⟩, Particle.attractor)] ⟨2, 3⟩)) = true := by
  native_decide

/-- The whole automaton decision on the demo board: no X decision, a `towardAttractor` at
distance 2 on Y, offset (0, +1). -/
theorem sense_verb_reports_whole_decision :
    (rulesFFI ("sense 0 " ++ encodeBoard demoBoard)
      == "1 0 3 0 3 2 2 0 3 0 0 0 0 1 1 2 0 0 1") = true := by native_decide

/-! ### The two audit answers the deleted Rust oracle got wrong -/

/-- ⚑ THE 2-CYCLE. Both pieces stay put — the board is unchanged. (`reference.rs::resolve_mid`
performed the SWAP; `resolve_witness.rs` documented the divergence rather than fixing it.) -/
theorem two_cycle_stays_put_on_wire :
    (rulesFFI ("mid " ++ encodeBoard cycBoard ++ " 0 2 0 0 0 0 1 1 0 1 0 0") ==
      "1 " ++ encodeBoard cycBoard) = true := by native_decide

/-- ⚑ THE INCLUSIVE PATH CHECK. A move onto a square held by a piece that nobody moves FAILS,
and the mover is replaced at its origin — the board is unchanged. (`reference.rs::occluded`
scanned the strict interior only, so the mover overwrote the occupant and DESTROYED it.) -/
theorem inclusive_path_check_blocks_on_wire :
    (rulesFFI ("mid " ++ encodeBoard blockBoard ++ " 0 1 0 0 0 0 3") ==
      "1 " ++ encodeBoard blockBoard) = true := by native_decide

/-- A plain move executes: the attractor at (0,0) walks to (0,2) on the empty file. -/
theorem plain_move_executes_on_wire :
    (rulesFFI ("mid " ++ encodeBoard cycBoard ++ " 0 1 0 0 1 0 3") ==
      "1 " ++ encodeBoard (mkBoard 5 [(⟨0, 3⟩, Particle.repulsor),
                                      (⟨0, 0⟩, Particle.attractor)] ⟨4, 4⟩)) = true := by
  native_decide

/-! ### Ruling (D) legality on the wire -/

/-- ⚑ RULING (D): the automaton square is banned as a move SOURCE ONLY.
(`reference.rs::move_valid` banned both endpoints — `logic/src/game.rs`'s reading, not the
README's.) -/
theorem automaton_square_illegal_as_source_on_wire :
    (rulesFFI ("legal " ++ encodeBoard cycBoard ++ " 0 0 4 4 0 4") == "1 0") = true := by
  native_decide

/-- Naming it as a DESTINATION is LEGAL to propose (and then fails to execute, above). -/
theorem automaton_square_legal_as_destination_on_wire :
    (rulesFFI ("legal " ++ encodeBoard cycBoard ++ " 0 0 0 0 4 0") == "1 1") = true := by
  native_decide

/-- A marked coordinate is illegal at EITHER endpoint, for everyone. -/
theorem marked_endpoint_illegal_on_wire :
    (rulesFFI ("legal " ++ encodeBoard cycBoard ++ " 1 0 2 0 0 0 0 2") == "1 0") = true := by
  native_decide

theorem unmarked_endpoint_legal_on_wire :
    (rulesFFI ("legal " ++ encodeBoard cycBoard ++ " 1 0 2 0 0 0 0 4") == "1 1") = true := by
  native_decide

/-- The three plain illegalities: `from == to` … -/
theorem same_square_move_illegal_on_wire :
    (rulesFFI ("legal " ++ encodeBoard cycBoard ++ " 0 0 0 0 0 0") == "1 0") = true := by
  native_decide

/-- … off-axis … -/
theorem off_axis_move_illegal_on_wire :
    (rulesFFI ("legal " ++ encodeBoard cycBoard ++ " 0 0 0 0 1 3") == "1 0") = true := by
  native_decide

/-- … out of bounds. -/
theorem out_of_bounds_move_illegal_on_wire :
    (rulesFFI ("legal " ++ encodeBoard cycBoard ++ " 0 0 0 0 0 9") == "1 0") = true := by
  native_decide

/-- A NEGATIVE coordinate is illegal, exactly as the OOB sentinel it decodes to. -/
theorem negative_coordinate_illegal_on_wire :
    (rulesFFI ("legal " ++ encodeBoard cycBoard ++ " 0 0 0 0 0 -1") == "1 0") = true := by
  native_decide

/-! ### `targets` — the PROPOSABLE set -/

/-- The LEGAL TARGETS of (0,0) on the 5x5 board: the rest of row 0 and the rest of column 0 —
the rook line, minus the source itself. (0,1) holds a repulsor and stays legal to NAME:
blocking is resolution's job, not legality's (ruling D / clause 3.2). -/
theorem targets_verb_is_the_rook_line :
    (rulesFFI ("targets " ++ encodeBoard cycBoard ++ " 0 0 0 0") ==
      "1 8 1 0 2 0 3 0 4 0 0 1 0 2 0 3 0 4") = true := by native_decide

/-- From the automaton's own square nothing is legal (it is banned as a SOURCE). -/
theorem targets_from_automaton_square_empty :
    (rulesFFI ("targets " ++ encodeBoard cycBoard ++ " 0 0 4 4") == "1 0") = true := by
  native_decide

/-! ### `livetargets` — the destinations that would EXECUTE

⚑ These are the teeth for the wound `targets_verb_is_the_rook_line` DOCUMENTS: `targets` from
`(0,0)` on `cycBoard` includes `(0,1)` — where a repulsor stands — and `(0,2)`/`(0,3)`/`(0,4)`
BEHIND it, and it is right to, because naming an occupied square is a legal proposal. A surface
reading that set as "squares you can move to" lights straight through the blocker, which is what
`dregg-automatafl/src/surface.rs` painted until this verb existed. -/

/-- The same source, the same board, the EXECUTABLE half: the empty row-0 rook line only. The
four column-0 squares are gone — `(0,1)` is the repulsor's own square (a destination-inclusive
block) and `(0,2)`/`(0,3)`/`(0,4)` sit behind it. -/
theorem livetargets_is_the_executable_rook_line :
    (rulesFFI ("livetargets " ++ encodeBoard cycBoard ++ " 0 0 0 0 0") ==
      "1 4 1 0 2 0 3 0 4 0") = true := by native_decide

/-- ⚑ CONDITIONAL, NOT ABSOLUTE. Hand the SAME question the fact that seat 1 is moving the
repulsor off `(0,1)`: a mover's source is PASSABLE, so all eight proposable squares become
live — including `(0,1)` itself, which empties. The two sets COINCIDE here, which is exactly
why the surface may not treat "blocked" as "illegal". -/
theorem livetargets_with_public_mover_coincides_with_targets :
    (rulesFFI ("livetargets " ++ encodeBoard cycBoard ++ " 0 1 1 0 1 2 1 0 0 0") ==
      rulesFFI ("targets " ++ encodeBoard cycBoard ++ " 0 0 0 0")) = true := by native_decide

/-- From the automaton's own square nothing executes either (nothing is even proposable). -/
theorem livetargets_from_automaton_square_empty :
    (rulesFFI ("livetargets " ++ encodeBoard cycBoard ++ " 0 0 0 4 4") == "1 0") = true := by
  native_decide

/-- Fail-closed on a truncated wire, like every other verb. -/
theorem livetargets_fails_closed_bare : (rulesFFI "livetargets" == "0") = true := by
  native_decide

theorem livetargets_fails_closed_truncated :
    (rulesFFI ("livetargets " ++ encodeBoard cycBoard ++ " 0 0 0") == "0") = true := by
  native_decide

/-! ### ⚑ THE SHIPPED BOARD, AND THE EXACT SQUARES A PLAYER WAS LIED TO ABOUT

ember selected the stock opening's attractor at `⟨3,1⟩` on the live 11×11 table and the surface
lit `⟨3,10⟩` — the far end of the file, BEHIND the attractor standing on `⟨3,9⟩` — plus the
whole of row 1 behind the three repulsors at `⟨4,1⟩`/`⟨5,1⟩`/`⟨6,1⟩`. Nine of the twenty
painted squares could not have executed. Both polarities are pinned so neither half can regress
into the other. -/

theorem stock_targets_count_twenty :
    ((targetsOf stockTwoPlayer [] 0 ⟨3, 1⟩).length == 20) = true := by native_decide

theorem stock_livetargets_count_eleven :
    ((liveTargetsOf stockTwoPlayer [] [] 0 ⟨3, 1⟩).length == 11) = true := by native_decide

/-- PROPOSABLE, and it must stay so — the ruleset lets you name it. -/
theorem stock_far_corner_is_proposable :
    ((targetsOf stockTwoPlayer [] 0 ⟨3, 1⟩).contains ⟨3, 10⟩) = true := by native_decide

theorem stock_row_end_is_proposable :
    ((targetsOf stockTwoPlayer [] 0 ⟨3, 1⟩).contains ⟨8, 1⟩) = true := by native_decide

/-- NOT LIVE — the two squares from the screenshot, by name. -/
theorem stock_far_corner_is_not_live :
    (!((liveTargetsOf stockTwoPlayer [] [] 0 ⟨3, 1⟩).contains ⟨3, 10⟩)) = true := by
  native_decide

theorem stock_row_end_is_not_live :
    (!((liveTargetsOf stockTwoPlayer [] [] 0 ⟨3, 1⟩).contains ⟨8, 1⟩)) = true := by
  native_decide

/-- NON-VACUOUS: the ray does reach `⟨3,8⟩`, the square in FRONT of the blocker, so the live
set is a real rook line and not an empty one. -/
theorem stock_square_before_blocker_is_live :
    ((liveTargetsOf stockTwoPlayer [] [] 0 ⟨3, 1⟩).contains ⟨3, 8⟩) = true := by native_decide

/-- And the whole live set, in the wire's own row-major order. -/
theorem stock_livetargets_wire_row_major :
    (rulesFFI ("livetargets " ++ encodeBoard stockTwoPlayer ++ " 0 0 0 3 1") ==
      "1 11 3 0 0 1 1 1 2 1 3 2 3 3 3 4 3 5 3 6 3 7 3 8") = true := by native_decide

/-! ### `clash` and `round` -/

/-- A FORK (one source, two destinations) is a conflict on that source … -/
theorem fork_clash_reported_on_wire :
    (rulesFFI ("clash " ++ encodeBoard cycBoard ++ " 0 2 0 0 0 0 3 1 0 0 3 0")
      == "1 1 0 0") = true := by native_decide

/-- … and a clean round has none. -/
theorem clean_round_reports_no_clash :
    (rulesFFI ("clash " ++ encodeBoard cycBoard ++ " 0 1 0 0 1 0 3") == "1 0") = true := by
  native_decide

/-- `round` on a clean round RESOLVES to the same board `mid`/`step` compose to, with no win. -/
theorem round_verb_resolves_clean_round :
    (rulesFFI ("round 0 " ++ encodeBoard cycBoard ++ " 0 0 0 2 0 1 1 0 0 1 0 3") ==
      "1 R -1 " ++ encodeBoard (mkBoard 5 [(⟨0, 3⟩, Particle.repulsor),
                                           (⟨0, 0⟩, Particle.attractor)] ⟨4, 4⟩)) = true := by
  native_decide

/-- A conflicted round comes back AGAIN, with the contested coordinate marked and its seat
re-entered. -/
theorem round_verb_returns_again_on_conflict :
    (rulesFFI ("round 0 " ++ encodeBoard cycBoard ++ " 0 0 0 2 0 1 2 0 0 0 0 3 1 0 0 3 0") ==
      "1 A 1 0 0 0 2 0 1") = true := by native_decide

/-! ### Fail-closed dispatch -/

/-- No verb … -/
theorem empty_wire_fails_closed : (rulesFFI "" == "0") = true := by native_decide

/-- … an unknown verb … -/
theorem unknown_verb_fails_closed : (rulesFFI "bogus" == "0") = true := by native_decide

/-- … a truncated board … -/
theorem truncated_board_fails_closed : (rulesFFI "mid 5" == "0") = true := by native_decide

/-- … a lying cell count … -/
theorem lying_cell_count_fails_closed :
    (rulesFFI "mid 5 000 0 0 1 0 0" == "0") = true := by native_decide

/-- … and a bad tie-break token. -/
theorem bad_tie_token_fails_closed :
    (rulesFFI "step 7 5 0000000000000000000000000 4 4 1" == "0") = true := by native_decide

/-! ### `adjudicate` — the capped match's terminal rule

The stock opening is a GENUINE draw, not an accident of the encoding: the automaton starts at
`⟨5,5⟩`, seat 0 owns the `y = 0` corners and seat 1 the `y = 10` corners, so on an 11×11 both
seats sit exactly 5 away. The two direct pins below are the anti-vacuity poles — nudge the
automaton one row and the adjudication actually picks a winner, so `-1` above is a verdict
rather than a stuck default. -/

theorem adjudicate_stock_opening_is_a_draw :
    (rulesFFI ("adjudicate " ++ encodeBoard stockTwoPlayer ++ " "
      ++ encodeGoals (stockGoals2 11)) == "1 -1") = true := by native_decide

theorem adjudicate_nudge_north_picks_seat_zero :
    (adjudicateCapped ⟨5, 4⟩ (stockGoals2 11) == some 0) = true := by native_decide

theorem adjudicate_nudge_south_picks_seat_one :
    (adjudicateCapped ⟨5, 6⟩ (stockGoals2 11) == some 1) = true := by native_decide

/-- A malformed `adjudicate` wire fails CLOSED, like every other verb. -/
theorem adjudicate_bare_fails_closed : (rulesFFI "adjudicate" == "0") = true := by native_decide

theorem adjudicate_truncated_fails_closed :
    (rulesFFI "adjudicate 5" == "0") = true := by native_decide

/-! ### `dist` — the two numbers behind the verdict

The played surface shows a THREAT reading per seat ("N steps from a goal"). These pins tie it
to `goalDistance`, so the number on screen is the one `adjudicateCapped` actually compares and
the one `adjudicate_sound` is a theorem about.

Non-vacuity is the point of the three poles: the stock opening is 10/10 with a `-1` verdict (a
dead heat, matching `stock_opening_adjudicates_draw`), and the two nudged boards report
DIFFERENT numbers with a decisive verdict — so a reader can see that the pair moves, and moves
together. `⟨10,2⟩` is the position the play harness actually froze on
(`adjudicate_decisive_witness`): 2 from seat 0's `⟨10,0⟩`, 8 from seat 1's `⟨10,10⟩`. -/

theorem dist_stock_opening_dead_heat :
    (rulesFFI ("dist " ++ encodeBoard stockTwoPlayer ++ " " ++ encodeGoals (stockGoals2 11))
      == "1 10 10 -1") = true := by native_decide

theorem dist_frozen_position_seat_zero :
    (rulesFFI ("dist " ++ encodeBoard (mkBoard 11 [] ⟨10, 2⟩) ++ " " ++
      encodeGoals (stockGoals2 11)) == "1 2 8 0") = true := by native_decide

theorem dist_mirror_position_seat_one :
    (rulesFFI ("dist " ++ encodeBoard (mkBoard 11 [] ⟨3, 8⟩) ++ " " ++
      encodeGoals (stockGoals2 11)) == "1 11 5 1") = true := by native_decide

/-- A seat that owns NO corner reports `-1` rather than `0`: an unseated player is not adjacent
to a goal, and `adjudicate` hands the match to the seat that does own one
(`adjudicate_seated`). -/
theorem dist_unseated_seat_reports_minus_one :
    (rulesFFI ("dist " ++ encodeBoard (mkBoard 11 [] ⟨5, 5⟩) ++ " 1 0 0 0")
      == "1 10 -1 0") = true := by native_decide

/-- Fail-closed, like every other verb. -/
theorem dist_bare_fails_closed : (rulesFFI "dist" == "0") = true := by native_decide

theorem dist_truncated_fails_closed : (rulesFFI "dist 5" == "0") = true := by native_decide

/-! ## §10 — R2 Variant Verbs (`preset`, `midcfg`, `stepcfg`, `roundcfg`) -/

/-- `preset 7` produces the 7×7 Blitz opening board with automaton at `(3,3)`. -/
theorem preset_blitz7_opening :
    ((rulesFFI "preset 7").startsWith "1 7 " && (rulesFFI "preset 7").endsWith " 3 3 1") = true := by
  native_decide

/-- `preset 9` produces the 9×9 Tactical opening board with automaton at `(4,4)`. -/
theorem preset_tactical9_opening :
    ((rulesFFI "preset 9").startsWith "1 9 " && (rulesFFI "preset 9").endsWith " 4 4 1") = true := by
  native_decide

/-- `preset 11` matches `stock`. -/
theorem preset_classic11_eq_stock :
    (rulesFFI "preset 11" == rulesFFI "stock") = true := by
  native_decide

/-- `bunchBefore` (`conf=1`): repulsor at `(0,0)` moving to `(2,0)` and attractor at `(2,2)` moving
to `(2,0)` both stop 1 cell short of the contested merge point `(2,0)` — at `(1,0)` and `(2,1)` —
leaving `(2,0)` empty without re-entry per `MERGE_RESOLUTION_DESIGN.md`! -/
theorem midcfg_bunch_before_resolves_without_conflict :
    let b3 := "3 100030002 1 1 1"
    -- cfg: tie=0 conf=1 swap=0 cd=0 fling=0 ac=1; marks=0; moves: (0,0)->(2,0), (2,2)->(2,0)
    (rulesFFI s!"midcfg 0 1 0 0 0 1 {b3} 0 2 0 0 0 2 0 1 2 2 2 0" ==
      "1 3 010032000 1 1 1") = true := by
  native_decide

/-- `bunchBefore` (`conf=1`) 3-mover caterpillar + merge collision:
`(0,1)->(0,0)` behind `(0,0)->(1,0)` colliding with `(2,0)->(1,0)`.
`(1,0)` stays empty, `(0,0)` and `(2,0)` stay put, and `(0,1)` backs up to `(0,1)` —
conserving all 3 pieces and remaining invariant under move-list permutation! -/
theorem midcfg_bunch_before_caterpillar_conserves_and_perm_invariant :
    let b3 := "3 102130000 1 1 1"
    let out1 := rulesFFI s!"midcfg 0 1 0 0 0 1 {b3} 0 3 0 0 1 0 0 0 0 0 1 0 1 2 0 1 0"
    let out2 := rulesFFI s!"midcfg 0 1 0 0 0 1 {b3} 0 3 1 2 0 1 0 0 0 0 1 0 0 0 1 0 0"
    (out1 == "1 3 102130000 1 1 1" && out2 == "1 3 102130000 1 1 1") = true := by
  native_decide

/-- `annihilate` (`conf=2`): colliding movers `(0,0)->(2,0)` and `(2,2)->(2,0)` destroy each other,
leaving vacuum at `(0,0)`, `(2,2)`, and `(2,0)`! -/
theorem midcfg_annihilate_destroys_colliders :
    let b3 := "3 100030002 1 1 1"
    (rulesFFI s!"midcfg 0 2 0 0 0 1 {b3} 0 2 0 0 0 2 0 1 2 2 2 0" ==
      "1 3 000030000 1 1 1") = true := by
  native_decide

/-- `twoCycleSwaps = true` (`swap=1`): repulsor at `(0,0)` and attractor at `(1,0)` swap places! -/
theorem midcfg_two_cycle_swaps_when_enabled :
    let b3 := "3 120030000 1 1 1"
    (rulesFFI s!"midcfg 0 0 1 0 0 1 {b3} 0 2 0 0 0 1 0 1 1 0 0 0" ==
      "1 3 210030000 1 1 1") = true := by
  native_decide

/-- `railgunFling = true` (`fling=1`): automaton at `(0,2)` with attractor at `(4,2)` flings all
the way to `(3,2)` (1 square adjacent to the attractor) in a single step! -/
theorem stepcfg_railgun_fling_slides_to_adjacent :
    let b5 := "5 0000000000300020000000000 0 2 1"
    -- cfg: tie=0 conf=0 swap=0 cd=0 fling=1 ac=1; lastDir=0
    (rulesFFI s!"stepcfg 0 0 0 0 1 1 0 {b5}" ==
      "1 5 0000000000000320000000000 3 2 1") = true := by
  native_decide

/-- `cooldownTurns = 2` (`cd=2`): moving `(0,0)->(2,0)` resolves the round and records `(2,0)` with
cooldown 2 in the returned cooldown map (`1 2 0 2`). -/
theorem roundcfg_records_cooldown :
    let b3 := "3 100030000 1 1 1"
    -- cfg: tie=0 conf=0 swap=0 cd=2 fling=0 ac=1; ldir=0; b3; goals=0; marks=0; locked=0; waiting=1 0; cds=0; subs=1 (0,0)->(2,0)
    (rulesFFI s!"roundcfg 0 0 0 2 0 1 0 {b3} 0 0 0 1 0 0 1 0 0 0 2 0").endsWith " 1 2 0 2 0" = true := by
  native_decide

#assert_compiled mid_roundtrips_demo_board
#assert_compiled mid_roundtrips_stock_board
#assert_compiled stock_verb_is_the_eleven_by_eleven_opening
#assert_compiled goals_verb_is_stock_two_player
#assert_compiled step_verb_walks_toward_attractor
#assert_compiled sense_verb_reports_whole_decision
#assert_compiled two_cycle_stays_put_on_wire
#assert_compiled inclusive_path_check_blocks_on_wire
#assert_compiled plain_move_executes_on_wire
#assert_compiled automaton_square_illegal_as_source_on_wire
#assert_compiled automaton_square_legal_as_destination_on_wire
#assert_compiled marked_endpoint_illegal_on_wire
#assert_compiled unmarked_endpoint_legal_on_wire
#assert_compiled same_square_move_illegal_on_wire
#assert_compiled off_axis_move_illegal_on_wire
#assert_compiled out_of_bounds_move_illegal_on_wire
#assert_compiled negative_coordinate_illegal_on_wire
#assert_compiled targets_verb_is_the_rook_line
#assert_compiled targets_from_automaton_square_empty
#assert_compiled livetargets_is_the_executable_rook_line
#assert_compiled livetargets_with_public_mover_coincides_with_targets
#assert_compiled livetargets_from_automaton_square_empty
#assert_compiled livetargets_fails_closed_bare
#assert_compiled livetargets_fails_closed_truncated
#assert_compiled stock_targets_count_twenty
#assert_compiled stock_livetargets_count_eleven
#assert_compiled stock_far_corner_is_proposable
#assert_compiled stock_row_end_is_proposable
#assert_compiled stock_far_corner_is_not_live
#assert_compiled stock_row_end_is_not_live
#assert_compiled stock_square_before_blocker_is_live
#assert_compiled stock_livetargets_wire_row_major
#assert_compiled fork_clash_reported_on_wire
#assert_compiled clean_round_reports_no_clash
#assert_compiled round_verb_resolves_clean_round
#assert_compiled round_verb_returns_again_on_conflict
#assert_compiled empty_wire_fails_closed
#assert_compiled unknown_verb_fails_closed
#assert_compiled truncated_board_fails_closed
#assert_compiled lying_cell_count_fails_closed
#assert_compiled bad_tie_token_fails_closed
#assert_compiled adjudicate_stock_opening_is_a_draw
#assert_compiled adjudicate_nudge_north_picks_seat_zero
#assert_compiled adjudicate_nudge_south_picks_seat_one
#assert_compiled adjudicate_bare_fails_closed
#assert_compiled adjudicate_truncated_fails_closed
#assert_compiled dist_stock_opening_dead_heat
#assert_compiled dist_frozen_position_seat_zero
#assert_compiled dist_mirror_position_seat_one
#assert_compiled dist_unseated_seat_reports_minus_one
#assert_compiled dist_bare_fails_closed
#assert_compiled dist_truncated_fails_closed
#assert_compiled preset_blitz7_opening
#assert_compiled preset_tactical9_opening
#assert_compiled preset_classic11_eq_stock
#assert_compiled midcfg_bunch_before_resolves_without_conflict
#assert_compiled midcfg_bunch_before_caterpillar_conserves_and_perm_invariant
#assert_compiled midcfg_annihilate_destroys_colliders
#assert_compiled midcfg_two_cycle_swaps_when_enabled
#assert_compiled stepcfg_railgun_fling_slides_to_adjacent
#assert_compiled roundcfg_records_cooldown

end Dregg2.Games.AutomataflFFI
