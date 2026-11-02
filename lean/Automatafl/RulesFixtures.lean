/-
# AutomataflRules — the conformance-block EVALUATION, out of the crypto archive's build

`AutomataflRules.lean` sits in the `Dregg2.FFI` closure (the crypto archive's build root), and
until 2026-08-08 its §10 conformance block ran 110 `#guard` fixture pins at elaboration — so any
game-fixture regression was a hard failure of every Rust proving target in the workspace (the
compilation-unit coupling the stale-fixture outage measured). The WITNESS BOARDS and moves
remain in `AutomataflRules.lean`; THIS module is where the facts are RUN — each former `#guard`
as a NAMED theorem per GUARD-DISCIPLINE (a fact worth asserting is worth naming), proved by
`native_decide` and pinned `#assert_compiled`. It is rooted in the central guard library and
reachable from `Dregg2.FFI` by NOTHING, so:

  * a plain `lake build` still evaluates every pin — no loss of checking;
  * `lake build Dregg2.FFI` (what `dregg-lean-ffi/build.rs` and the seed scripts run) never
    does — a red pin here can no longer take the archive down.

Facts that mention the parent's private helpers (`mv`, `cfgCol`/`cfgRow`/`cfgFrz`, `noGoals`)
are pinned through the public evaluation-free `check_* : Bool` definitions the parent exposes.
Sections mirror the parent's §10, keyed to the audit's divergence table
(`docs/reference/AUTOMATAFL-RULES-CONFORMANCE-AUDIT.md` §A).
-/
import Automatafl.Rules
open Automatafl.Tactics


namespace Dregg2.Games.AutomataflRules

open Dregg2.Games.Automatafl

/-! ### 1.4 — the automaton square is banned as a SOURCE only (ruling D) -/

/-- Naming the automaton square as a DESTINATION is legal to propose … -/
theorem into_automaton_is_legal_to_propose : moveLegalB autoBoard [] intoAuto = true := by
  native_decide

/-- … and simply FAILS to execute: the square is occupied, so the move is blocked … -/
theorem into_automaton_is_blocked : blockedB autoBoard [intoAuto] intoAuto = true := by
  native_decide

/-- … and the mover is replaced at its origin. -/
theorem into_automaton_mover_replaced_at_origin :
    (resolveMoves autoBoard [intoAuto]).cellAt ⟨0, 2⟩ = Particle.attractor := by native_decide

theorem into_automaton_square_keeps_automaton :
    (resolveMoves autoBoard [intoAuto]).cellAt ⟨2, 2⟩ = Particle.automaton := by native_decide

/-- Naming it as a SOURCE is illegal (model.py `POS_CANT_MOVE_THAT`). -/
theorem out_of_automaton_is_illegal : moveLegalB autoBoard [] outOfAuto = false := by
  native_decide

/-! ### 1.5 — a marked coordinate is illegal as source AND destination, for everyone -/

theorem marked_source_is_illegal : moveLegalB autoBoard [⟨0, 2⟩] intoAuto = false := by
  native_decide

theorem marked_destination_is_illegal : moveLegalB autoBoard [⟨2, 2⟩] intoAuto = false := by
  native_decide

/-! ### 3.2 (audit D1, CRIT) — the destination is ON the path -/

theorem d1_move_is_legal : moveLegalB d1Board [] d1Move = true := by native_decide

/-- WAS false (exclusive path). -/
theorem d1_destination_occupant_blocks : blockedB d1Board [d1Move] d1Move = true := by
  native_decide

theorem d1_mover_replaced_at_origin :
    (resolveMoves d1Board [d1Move]).cellAt ⟨0, 0⟩ = Particle.attractor := by native_decide

theorem d1_occupant_survives :
    (resolveMoves d1Board [d1Move]).cellAt ⟨0, 2⟩ = Particle.repulsor := by native_decide

/-- The repulsor is still SOMEWHERE on the board (the old spec's scan found none). -/
theorem d1_repulsor_still_on_board :
    ((List.range 3).any (fun x => (List.range 3).any (fun y =>
      (resolveMoves d1Board [d1Move]).cellAt ⟨x, y⟩ == Particle.repulsor))) = true := by
  native_decide

/-! ### 3.3 — chains continue through vacated / vacuum squares -/

theorem chain_through_vacuum_reaches_far_square :
    (resolveMoves chainBoard [ch1, ch2]).cellAt ⟨0, 2⟩ = Particle.attractor := by native_decide

theorem chain_through_vacuum_vacates_source :
    (resolveMoves chainBoard [ch1, ch2]).cellAt ⟨0, 0⟩ = Particle.vacuum := by native_decide

/-! ### 3.4 — the ERRATUM / caterpillar -/

theorem caterpillar_vacates_first_source :
    (resolveMoves catBoard [cat1, cat2]).cellAt ⟨0, 0⟩ = Particle.vacuum := by native_decide

theorem caterpillar_follower_rides_vacated_source :
    (resolveMoves catBoard [cat1, cat2]).cellAt ⟨0, 1⟩ = Particle.attractor := by native_decide

theorem caterpillar_leader_advances :
    (resolveMoves catBoard [cat1, cat2]).cellAt ⟨0, 2⟩ = Particle.repulsor := by native_decide

/-! ### 3.6 + the author's rule — a blocked chain fails to execute, and is NOT a conflict -/

/-- The leader is blocked … -/
theorem stuck_leader_is_blocked : blockedB stuckBoard [st1, st2] st2 = true := by native_decide

/-- … but the follower's own path is clear. -/
theorem stuck_follower_path_is_clear : blockedB stuckBoard [st1, st2] st1 = false := by
  native_decide

theorem stuck_round_is_not_a_clash : clashCoords stuckBoard [st1, st2] = ([] : List Coord) := by
  native_decide

theorem stuck_round_is_not_a_merge : unresolved stuckBoard [st1, st2] = ([] : List Coord) := by
  native_decide

theorem stuck_round_moves_nothing : movers stuckBoard [st1, st2] = ([] : List Coord) := by
  native_decide

theorem stuck_first_piece_stays :
    (resolveMoves stuckBoard [st1, st2]).cellAt ⟨0, 0⟩ = Particle.attractor := by native_decide

theorem stuck_second_piece_stays :
    (resolveMoves stuckBoard [st1, st2]).cellAt ⟨0, 1⟩ = Particle.repulsor := by native_decide

theorem stuck_blocker_stays :
    (resolveMoves stuckBoard [st1, st2]).cellAt ⟨0, 2⟩ = Particle.attractor := by native_decide

/-- The guard is not swallowing the ordinary cases: real resolutions ARE resolvable. -/
theorem cat_round_is_resolvable : resolvableB catBoard [cat1, cat2] = true := by native_decide

theorem chain_round_is_resolvable : resolvableB chainBoard [ch1, ch2] = true := by native_decide

/-! ### 3.5a (audit D2) — a 2-cycle with pieces on BOTH squares STAYS PUT -/

theorem two_cycle_is_not_a_clash : clashCoords d2Board [d2A, d2B] = ([] : List Coord) := by
  native_decide

theorem two_cycle_detected : twoCyc (edgeMap d2Board [d2A, d2B]) ⟨0, 0⟩ = true := by
  native_decide

/-- WAS repulsor (the old spec SWAPPED them). -/
theorem two_cycle_first_piece_stays :
    (resolveMoves d2Board [d2A, d2B]).cellAt ⟨0, 0⟩ = Particle.attractor := by native_decide

/-- WAS attractor. -/
theorem two_cycle_second_piece_stays :
    (resolveMoves d2Board [d2A, d2B]).cellAt ⟨0, 2⟩ = Particle.repulsor := by native_decide

/-! ### 3.5b (audit D3) — the empty-square-back-to-a-source case stays put -/

/-- WAS vacuum (the old spec moved it). -/
theorem empty_return_move_leaves_piece_put :
    (resolveMoves d3Board [d3A, d3B]).cellAt ⟨0, 0⟩ = Particle.attractor := by native_decide

/-- WAS attractor. -/
theorem empty_return_move_fills_nothing :
    (resolveMoves d3Board [d3A, d3B]).cellAt ⟨0, 2⟩ = Particle.vacuum := by native_decide

/-! ### 3.5c (audit D6) — an EMPTY cycle cannot pull a piece in -/

theorem empty_cycle_walk_never_terminates :
    stopWalk (edgeMap d6Board [e1, e2, e3]) (carAt d6Board) 4 ⟨0, 0⟩ = none := by native_decide

/-- WAS vacuum. -/
theorem empty_cycle_cannot_pull_a_piece :
    (resolveMoves d6Board [e1, e2, e3]).cellAt ⟨0, 0⟩ = Particle.attractor := by native_decide

/-- WAS attractor. -/
theorem empty_cycle_far_square_stays_empty :
    (resolveMoves d6Board [e1, e2, e3]).cellAt ⟨0, 2⟩ = Particle.vacuum := by native_decide

/-! ### 3.5d — a >2-cycle with every square carrying ROTATES one position (KEPT) -/

theorem rotation_carries_first_piece :
    (resolveMoves rotBoard [r1, r2, r3, r4]).cellAt ⟨2, 0⟩ = Particle.attractor := by
  native_decide

theorem rotation_carries_second_piece :
    (resolveMoves rotBoard [r1, r2, r3, r4]).cellAt ⟨2, 2⟩ = Particle.repulsor := by
  native_decide

theorem rotation_carries_third_piece :
    (resolveMoves rotBoard [r1, r2, r3, r4]).cellAt ⟨0, 2⟩ = Particle.attractor := by
  native_decide

theorem rotation_carries_fourth_piece :
    (resolveMoves rotBoard [r1, r2, r3, r4]).cellAt ⟨0, 0⟩ = Particle.repulsor := by
  native_decide

theorem rotation_round_is_resolvable : resolvableB rotBoard [r1, r2, r3, r4] = true := by
  native_decide

/-! ### 2.1 / 2.2 / 2.3 — fork, collide, and the identical-move EXCEPTION (KEPT) -/

/-- 2.1. -/
theorem fork_detected_at_shared_source : forkAt [fkA, fkB] ⟨0, 0⟩ = true := by native_decide

theorem fork_clashes_at_the_source :
    clashCoords forkBoard [fkA, fkB] = [(⟨0, 0⟩ : Coord)] := by native_decide

/-- 2.3: two players naming the SAME move is not a conflict … -/
theorem identical_moves_are_not_a_fork : forkAt [fkA, fkSame] ⟨0, 0⟩ = false := by native_decide

theorem identical_moves_are_not_a_clash :
    clashCoords forkBoard [fkA, fkSame] = ([] : List Coord) := by native_decide

/-- … and it resolves as ONE move. -/
theorem identical_moves_make_one_mover :
    movers forkBoard [fkA, fkSame] = [(⟨0, 0⟩ : Coord)] := by native_decide

theorem identical_moves_resolve_as_one :
    (resolveMoves forkBoard [fkA, fkSame]).cellAt ⟨0, 3⟩ = Particle.attractor := by
  native_decide

theorem identical_moves_vacate_the_source :
    (resolveMoves forkBoard [fkA, fkSame]).cellAt ⟨0, 0⟩ = Particle.vacuum := by native_decide

/-- 2.2 (through the parent's `check_*` — the witness needs the private `mv`). -/
theorem collide_detected_at_shared_destination :
    check_collide_detected_at_shared_destination = true := by native_decide

/-! ### 2.4a–d (audit D4a / D4b) — RE-ENTRY, LOCKING, the marked coordinate, RECURSION
(all through the parent's `check_*`s — `roundStep`/`runTurn` take the private configs) -/

theorem d4a_round_demands_reentry : check_d4a_round_demands_reentry = true := by native_decide

theorem d4a_marks_the_contested_destination :
    check_d4a_marks_the_contested_destination = true := by native_decide

theorem d4a_invalidates_every_touching_move :
    check_d4a_invalidates_every_touching_move = true := by native_decide

theorem d4a_all_three_seats_reenter : check_d4a_all_three_seats_reenter = true := by
  native_decide

theorem d4b_marks_the_forked_source : check_d4b_marks_the_forked_source = true := by
  native_decide

theorem d4b_invalidates_every_touching_move :
    check_d4b_invalidates_every_touching_move = true := by native_decide

theorem d4b_all_three_seats_reenter : check_d4b_all_three_seats_reenter = true := by
  native_decide

theorem uninvolved_move_is_locked : check_uninvolved_move_is_locked = true := by native_decide

theorem only_conflicted_seats_reenter : check_only_conflicted_seats_reenter = true := by
  native_decide

theorem second_round_resolves_the_reentered_move :
    check_second_round_resolves_the_reentered_move = true := by native_decide

theorem marked_source_move_fails_next_round :
    check_marked_source_move_fails_next_round = true := by native_decide

theorem one_round_is_not_enough : check_one_round_is_not_enough = true := by native_decide

/-! ### 3.7 (audit D5, CRIT) — a vacuum CONFLUENCE is now a CONFLICT -/

/-- Fork/collide silent … -/
theorem merge_is_invisible_to_fork_and_collide :
    clashCoords mgBoard [m1, m2, m3, m4] = ([] : List Coord) := by native_decide

/-- … the merge clause fires. -/
theorem merge_clause_fires_at_the_confluence :
    unresolved mgBoard [m1, m2, m3, m4] = [(⟨2, 0⟩ : Coord)] := by native_decide

theorem merge_round_is_not_resolvable : resolvableB mgBoard [m1, m2, m3, m4] = false := by
  native_decide

theorem merge_marks_the_confluence : check_merge_marks_the_confluence = true := by native_decide

theorem merge_locks_the_uninvolved_moves : check_merge_locks_the_uninvolved_moves = true := by
  native_decide

theorem merge_reenters_the_involved_seats :
    check_merge_reenters_the_involved_seats = true := by native_decide

/-- And no piece is destroyed … -/
theorem merge_destroys_no_piece_attractor :
    (resolveMoves mgBoard [m1, m2, m3, m4]).cellAt ⟨0, 0⟩ = Particle.attractor := by
  native_decide

theorem merge_destroys_no_piece_repulsor :
    (resolveMoves mgBoard [m1, m2, m3, m4]).cellAt ⟨4, 0⟩ = Particle.repulsor := by
  native_decide

/-- … under EITHER order (the audit's permutation) … -/
theorem merge_keeps_the_repulsor_under_permutation :
    ((List.range 5).any (fun x => (List.range 5).any (fun y =>
      (resolveMoves mgBoard [m3, m4, m1, m2]).cellAt ⟨x, y⟩ == Particle.repulsor))) = true := by
  native_decide

theorem merge_resolution_is_order_independent :
    ((List.range 5).all (fun x => (List.range 5).all (fun y =>
      (resolveMoves mgBoard [m1, m2, m3, m4]).cellAt ⟨x, y⟩
        == (resolveMoves mgBoard [m3, m4, m1, m2]).cellAt ⟨x, y⟩))) = true := by native_decide

/-! ### 4.x — the automaton, unchanged; 4.5 — the tie-break is SELECTABLE (ruling B) -/

theorem tie_board_axes_compare_equal :
    decisionCmp
      (evaluateAxis (tieBoard.raycast ⟨2, 2⟩ .xp) (tieBoard.raycast ⟨2, 2⟩ .xn))
      (evaluateAxis (tieBoard.raycast ⟨2, 2⟩ .yp) (tieBoard.raycast ⟨2, 2⟩ .yn))
      = Ordering.eq := by native_decide

theorem column_tiebreak_steps_along_y : check_column_tiebreak_steps_along_y = true := by
  native_decide

theorem row_tiebreak_steps_along_x : check_row_tiebreak_steps_along_x = true := by native_decide

theorem freeze_tiebreak_stays_put : check_freeze_tiebreak_stays_put = true := by native_decide

theorem demo_step_toward_attractor : check_demo_step_toward_attractor = true := by native_decide

theorem rep_step_flees_repulsor : check_rep_step_flees_repulsor = true := by native_decide

/-- The default config IS the old automaton (the Leg-A bridge, witnessed). -/
theorem default_config_is_old_automaton_demo :
    check_default_config_is_old_automaton_demo = true := by native_decide

theorem default_config_is_old_automaton_tie :
    check_default_config_is_old_automaton_tie = true := by native_decide

/-! ### 5.1 / 5.2 — SETUP: two corners in the same row each / one corner each -/

theorem stock_two_player_goals_well_formed : (stockGoals2 11).WellFormed2 11 = true := by
  native_decide

theorem stock_four_player_goals_well_formed : (stockGoals4 11).WellFormed4 11 = true := by
  native_decide

/-- The DEPLOYED assignment, corner for corner: `reference.rs::GOAL_CORNERS_2P` at n = 11. -/
theorem stock_goals_match_deployed_corners :
    stockGoals2 11
      = GoalAssignment.mk [(⟨0, 0⟩, 0), (⟨10, 0⟩, 0), (⟨0, 10⟩, 1), (⟨10, 10⟩, 1)] := by
  native_decide

/-- Its seat list is EXACTLY the two seats (`stockGoals2_seats`, at the deployed size). -/
theorem stock_goals_seat_exactly_two : (stockGoals2 11).seats = [0, 1] := by native_decide

/-- `model.py::DEFAULT_GOALS[2]` repeats (10,0) and gives seat 1 a COLUMN — NOT well-formed. -/
theorem prototype_goal_bug_is_not_well_formed :
    (GoalAssignment.mk
      [(⟨0, 0⟩, 0), (⟨10, 0⟩, 0), (⟨10, 0⟩, 1), (⟨10, 10⟩, 1)]).WellFormed2 11 = false := by
  native_decide

/-- Two corners in a COLUMN is not a legal two-player setup either. -/
theorem column_corners_are_not_a_legal_setup :
    (GoalAssignment.mk
      [(⟨0, 0⟩, 0), (⟨0, 10⟩, 0), (⟨10, 0⟩, 1), (⟨10, 10⟩, 1)]).WellFormed2 11 = false := by
  native_decide

/-! ### 5.4 — the win fires on the automaton MOVING INTO a corner, not sitting on one -/

/-- WAS some 7 in `Automatafl.lean`'s own witness — a "win" with no move at all. It FLIPS. -/
theorem win_requires_the_automaton_to_move :
    winOnEntry demoBoard demoBoard ⟨[(⟨2, 2⟩, 7)]⟩ = none := by native_decide

theorem win_fires_on_entry : check_win_fires_on_entry = true := by native_decide

theorem no_win_on_vacated_square : check_no_win_on_vacated_square = true := by native_decide

/-! ### The stock 11x11 opening — orientation pinned by raycast -/

theorem stock_board_automaton_dead_centre :
    stockTwoPlayer.cellAt ⟨5, 5⟩ = Particle.automaton := by native_decide

/-- Corners are occupied. -/
theorem stock_board_corners_occupied : stockTwoPlayer.cellAt ⟨0, 0⟩ = Particle.repulsor := by
  native_decide

/-- Pins [y][x] vs [x][y]. -/
theorem stock_board_orientation_pinned : stockTwoPlayer.cellAt ⟨3, 1⟩ = Particle.attractor := by
  native_decide

/-- The transpose is NOT the board. -/
theorem stock_board_transpose_is_not_the_board :
    stockTwoPlayer.cellAt ⟨1, 3⟩ = Particle.vacuum := by native_decide

theorem stock_board_east_ray_hits_repulsor :
    (stockTwoPlayer.raycast ⟨5, 5⟩ .xp).what = Particle.repulsor := by native_decide

theorem stock_board_east_ray_distance : (stockTwoPlayer.raycast ⟨5, 5⟩ .xp).dist = 4 := by
  native_decide

theorem stock_board_north_ray_distance : (stockTwoPlayer.raycast ⟨5, 5⟩ .yp).dist = 4 := by
  native_decide

theorem stock_opening_freezes_the_automaton :
    check_stock_opening_freezes_the_automaton = true := by native_decide

/-! ### §10.10 — the m = 2 collapse closed forms at the audit witnesses (non-vacuity) -/

/-- Caterpillar (§3.4): the follower rides onto the vacated leader source … -/
theorem caterpillar_follower_landing_closed_form :
    landMap catBoard [cat1, cat2] ⟨0, 0⟩ = (⟨0, 1⟩ : Coord) := by native_decide

/-- … and the leader lands on its own dest. -/
theorem caterpillar_leader_landing_closed_form :
    landMap catBoard [cat1, cat2] ⟨0, 1⟩ = (⟨0, 2⟩ : Coord) := by native_decide

theorem caterpillar_movers_closed_form :
    movers catBoard [cat1, cat2] = [(⟨0, 0⟩ : Coord), ⟨0, 1⟩] := by native_decide

theorem caterpillar_resolvable_closed_form : resolvableB catBoard [cat1, cat2] = true := by
  native_decide

/-- Flowthrough (§3.3): the piece flows through the vacuum waypoint all the way to `db`. -/
theorem flowthrough_landing_closed_form :
    landMap chainBoard [ch1, ch2] ⟨0, 0⟩ = (⟨0, 2⟩ : Coord) := by native_decide

theorem flowthrough_resolvable_closed_form : resolvableB chainBoard [ch1, ch2] = true := by
  native_decide

/-- Occluded-stayer (§3.6, D-style): leader blocked ⇒ both stay … -/
theorem occluded_stayer_follower_stays :
    landMap stuckBoard [st1, st2] ⟨0, 0⟩ = (⟨0, 0⟩ : Coord) := by native_decide

theorem occluded_stayer_leader_stays :
    landMap stuckBoard [st1, st2] ⟨0, 1⟩ = (⟨0, 1⟩ : Coord) := by native_decide

theorem occluded_stayer_movers_empty : movers stuckBoard [st1, st2] = ([] : List Coord) := by
  native_decide

/-- … still resolvable (no lost piece). -/
theorem occluded_stayer_still_resolvable : resolvableB stuckBoard [st1, st2] = true := by
  native_decide

/-- 2-cycle (§3.5a): both stay. -/
theorem two_cycle_first_landing_closed_form :
    landMap d2Board [d2A, d2B] ⟨0, 0⟩ = (⟨0, 0⟩ : Coord) := by native_decide

theorem two_cycle_second_landing_closed_form :
    landMap d2Board [d2A, d2B] ⟨0, 2⟩ = (⟨0, 2⟩ : Coord) := by native_decide

theorem two_cycle_resolvable_closed_form : resolvableB d2Board [d2A, d2B] = true := by
  native_decide

/-- Independent (§1.4): blocked (dest occupied) ⇒ the mover stays. -/
theorem blocked_single_mover_stays_closed_form :
    landMap d1Board [d1Move] ⟨0, 0⟩ = (⟨0, 0⟩ : Coord) := by native_decide

#assert_compiled into_automaton_is_legal_to_propose
#assert_compiled into_automaton_is_blocked
#assert_compiled into_automaton_mover_replaced_at_origin
#assert_compiled into_automaton_square_keeps_automaton
#assert_compiled out_of_automaton_is_illegal
#assert_compiled marked_source_is_illegal
#assert_compiled marked_destination_is_illegal
#assert_compiled d1_move_is_legal
#assert_compiled d1_destination_occupant_blocks
#assert_compiled d1_mover_replaced_at_origin
#assert_compiled d1_occupant_survives
#assert_compiled d1_repulsor_still_on_board
#assert_compiled chain_through_vacuum_reaches_far_square
#assert_compiled chain_through_vacuum_vacates_source
#assert_compiled caterpillar_vacates_first_source
#assert_compiled caterpillar_follower_rides_vacated_source
#assert_compiled caterpillar_leader_advances
#assert_compiled stuck_leader_is_blocked
#assert_compiled stuck_follower_path_is_clear
#assert_compiled stuck_round_is_not_a_clash
#assert_compiled stuck_round_is_not_a_merge
#assert_compiled stuck_round_moves_nothing
#assert_compiled stuck_first_piece_stays
#assert_compiled stuck_second_piece_stays
#assert_compiled stuck_blocker_stays
#assert_compiled cat_round_is_resolvable
#assert_compiled chain_round_is_resolvable
#assert_compiled two_cycle_is_not_a_clash
#assert_compiled two_cycle_detected
#assert_compiled two_cycle_first_piece_stays
#assert_compiled two_cycle_second_piece_stays
#assert_compiled empty_return_move_leaves_piece_put
#assert_compiled empty_return_move_fills_nothing
#assert_compiled empty_cycle_walk_never_terminates
#assert_compiled empty_cycle_cannot_pull_a_piece
#assert_compiled empty_cycle_far_square_stays_empty
#assert_compiled rotation_carries_first_piece
#assert_compiled rotation_carries_second_piece
#assert_compiled rotation_carries_third_piece
#assert_compiled rotation_carries_fourth_piece
#assert_compiled rotation_round_is_resolvable
#assert_compiled fork_detected_at_shared_source
#assert_compiled fork_clashes_at_the_source
#assert_compiled identical_moves_are_not_a_fork
#assert_compiled identical_moves_are_not_a_clash
#assert_compiled identical_moves_make_one_mover
#assert_compiled identical_moves_resolve_as_one
#assert_compiled identical_moves_vacate_the_source
#assert_compiled collide_detected_at_shared_destination
#assert_compiled d4a_round_demands_reentry
#assert_compiled d4a_marks_the_contested_destination
#assert_compiled d4a_invalidates_every_touching_move
#assert_compiled d4a_all_three_seats_reenter
#assert_compiled d4b_marks_the_forked_source
#assert_compiled d4b_invalidates_every_touching_move
#assert_compiled d4b_all_three_seats_reenter
#assert_compiled uninvolved_move_is_locked
#assert_compiled only_conflicted_seats_reenter
#assert_compiled second_round_resolves_the_reentered_move
#assert_compiled marked_source_move_fails_next_round
#assert_compiled one_round_is_not_enough
#assert_compiled merge_is_invisible_to_fork_and_collide
#assert_compiled merge_clause_fires_at_the_confluence
#assert_compiled merge_round_is_not_resolvable
#assert_compiled merge_marks_the_confluence
#assert_compiled merge_locks_the_uninvolved_moves
#assert_compiled merge_reenters_the_involved_seats
#assert_compiled merge_destroys_no_piece_attractor
#assert_compiled merge_destroys_no_piece_repulsor
#assert_compiled merge_keeps_the_repulsor_under_permutation
#assert_compiled merge_resolution_is_order_independent
#assert_compiled tie_board_axes_compare_equal
#assert_compiled column_tiebreak_steps_along_y
#assert_compiled row_tiebreak_steps_along_x
#assert_compiled freeze_tiebreak_stays_put
#assert_compiled demo_step_toward_attractor
#assert_compiled rep_step_flees_repulsor
#assert_compiled default_config_is_old_automaton_demo
#assert_compiled default_config_is_old_automaton_tie
#assert_compiled stock_two_player_goals_well_formed
#assert_compiled stock_four_player_goals_well_formed
#assert_compiled stock_goals_match_deployed_corners
#assert_compiled stock_goals_seat_exactly_two
#assert_compiled prototype_goal_bug_is_not_well_formed
#assert_compiled column_corners_are_not_a_legal_setup
#assert_compiled win_requires_the_automaton_to_move
#assert_compiled win_fires_on_entry
#assert_compiled no_win_on_vacated_square
#assert_compiled stock_board_automaton_dead_centre
#assert_compiled stock_board_corners_occupied
#assert_compiled stock_board_orientation_pinned
#assert_compiled stock_board_transpose_is_not_the_board
#assert_compiled stock_board_east_ray_hits_repulsor
#assert_compiled stock_board_east_ray_distance
#assert_compiled stock_board_north_ray_distance
#assert_compiled stock_opening_freezes_the_automaton
#assert_compiled caterpillar_follower_landing_closed_form
#assert_compiled caterpillar_leader_landing_closed_form
#assert_compiled caterpillar_movers_closed_form
#assert_compiled caterpillar_resolvable_closed_form
#assert_compiled flowthrough_landing_closed_form
#assert_compiled flowthrough_resolvable_closed_form
#assert_compiled occluded_stayer_follower_stays
#assert_compiled occluded_stayer_leader_stays
#assert_compiled occluded_stayer_movers_empty
#assert_compiled occluded_stayer_still_resolvable
#assert_compiled two_cycle_first_landing_closed_form
#assert_compiled two_cycle_second_landing_closed_form
#assert_compiled two_cycle_resolvable_closed_form
#assert_compiled blocked_single_mover_stays_closed_form

end Dregg2.Games.AutomataflRules
