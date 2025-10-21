use crate::*;
use serde::{Deserialize, Serialize};
use vec_map::VecMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub winner: Option<Pid>,
    /// Players who cannot submit moves during RoundState::ResolvingConflict
    pub locked_players: SmallVec<[Pid; 2]>,
    pub board: Board,
    pub round: RoundState,
    /// Submitted moves so far
    pub pending_moves: SmallVec<[Move; 2]>,
    /// Goal locations, when the automaton enters one of these that player wins.
    pub goals: SmallVec<[(Coord, Pid); 4]>,
    pub player_count: u8,
    pub use_column_rule: bool,
}

impl Game {
    /// Create a new game using the given board.
    pub fn new(board: Board, player_count: u8, use_column_rule: bool) -> Game {
        Game {
            winner: None,
            locked_players: SmallVec::new(),
            board,
            round: RoundState::Fresh,
            pending_moves: SmallVec::new(),
            goals: SmallVec::new(),
            player_count,
            use_column_rule,
        }
    }

    /// Propose a move, returning some feedback about it, and true if the state
    /// machine is ready to advance (try_complete_round preconditions are met).
    ///
    /// Returns false if try_complete_round would panic.
    #[instrument]
    pub fn propose_move(&mut self, m: Move) -> ProposeFeedback {
        use MoveFeedback::*;

        let mut cfs = CoordsFeedback {
            data: SmallVec::new(),
        };

        // rules for a single coord, returns false if we shouldn't continue
        fn consider(cfs: &mut CoordsFeedback, b: &Board, c: Coord) -> bool {
            use CoordFeedback::*;
            let feedback = if !b.inbounds(c) {
                Oob
            } else if b.is_automaton(c) {
                Automaton
            } else if b.is_conflict(c) {
                Conflict
            } else {
                Ok
            };
            let res = feedback == Ok;
            cfs.data.push((c, feedback));
            res
        }


        return ProposeFeedback::Rejected(if self.round == RoundState::GameOver {
            GameOver
        } else if self.locked_players.contains(&m.who) {
            WaitYourTurn
        } else if m.from == m.to {
            MustMove
        } else if !(m.from.x == m.to.x || m.from.y == m.to.y) {
            AxisAlignedOnly
        } else {
            let from_ok = consider(&mut cfs, &self.board, m.from);
            let to_ok = consider(&mut cfs, &self.board, m.to);
            if from_ok && to_ok {
                self.pending_moves.push(m);
                if self.pending_moves.len() == self.player_count as usize {
                    return ProposeFeedback::AcceptedAndReady;
                } else {
                    return ProposeFeedback::Accepted;
                }
            } else {
                SeeCoords(cfs)
            }
        });
    }

    /// Returns Ok with the list of applied move to apply, or else the list of
    /// conflicting moves.
    #[instrument]
    fn resolve_conflicts(&mut self) -> Result<SmallVec<[Move; 2]>, SmallVec<[Move; 2]>> {
        debug_assert!(self.pending_moves.len() == self.player_count as usize);

        let mut seen_pairs = SmallVec::<[(Coord, Coord); 2]>::new();

        let mut seen_from = SmallVec::<[Coord; 2]>::new();
        let mut seen_to = SmallVec::<[Coord; 2]>::new();

        let mut conflict_moves = SmallVec::<[Move; 2]>::new();
        let mut locked_moves = SmallVec::<[Move; 2]>::new();

        for &m in &self.pending_moves {
            let mut conflict = false;
            let this_pair = (m.from, m.to);
            if seen_pairs.contains(&this_pair) {
                // multiple players specifying the same move is OK!
                locked_moves.push(m);
                continue;
            }
            seen_pairs.push(this_pair);

            // See if there's a source conflict...
            if seen_from.contains(&m.from) {
                trace!("marking source conflict on {coord}", coord = m.from);
                self.board.mark_conflict(m.from);
                conflict = true;
            } else {
                seen_from.push(m.from);
            }

            // Or a dest conflict...
            if seen_to.contains(&m.to) {
                trace!("marking dest conflict on {coord}", coord = m.to);
                self.board.mark_conflict(m.to);
                conflict = true;
            } else {
                seen_to.push(m.to);
            }

            if conflict {
                conflict_moves.push(m);
                // We conflicted with some previous move, pull them out of the
                // locked list and into the conflict list.
                for p in locked_moves.drain_filter(|p| p.from == m.from || p.to == m.to) {
                    trace!("caused conflict with player {pid:?}", pid = p.who);
                    conflict_moves.push(p);
                }
            } else {
                // We'll consider this move locked unless someone else conflicts with it.
                locked_moves.push(m);
            }
        }

        if conflict_moves.len() == 0 {
            Ok(locked_moves)
        } else {
            Err(conflict_moves)
        }
    }

    pub fn apply_moves(
        &mut self,
        moves_to_apply: SmallVec<[Move; 2]>,
    ) -> SmallVec<[(Move, MoveResult); 2]> {
        let mut results = SmallVec::<[(Move, MoveResult); 2]>::new();
        let mut move_graph = VecMap::new();
        let mut source_particles = VecMap::new();

        // Mark all source squares as passable for occlusion checks
        for m in &moves_to_apply {
            self.board.mark_passable(m.from);
        }

        // 1. Validate moves based on occlusion by STATIC pieces and build the move graph.
        //    We intentionally include moves from empty squares to build the full graph.
        let mut valid_moves = SmallVec::<[Move; 2]>::new();
        for &m in &moves_to_apply {
            // Check for occlusion by non-moving pieces.
            let delta = m.to - m.from;
            let axis = delta.axial_unit();
            let mut occluded = false;
            // Check path *between* from and to
            for offset in 1..delta.displacement() {
                let c = m.from + axis.scale(offset as isize);
                if self.board.particles[c.ix()].occludes() {
                    results.push((m, MoveResult::OccupiedAt(c)));
                    occluded = true;
                    break;
                }
            }

            if !occluded {
                let from_key = m.from.to_key(self.board.size.x);
                move_graph.insert(from_key, m.to);
                if !self.board.is_vacuum(m.from) {
                    source_particles.insert(from_key, self.board.particles[m.from.ix()].what);
                }
                valid_moves.push(m);
            }
        }

        // 2. Determine final placement for each particle.
        let mut final_placements = VecMap::new();
        let mut visited_sources = VecMap::new(); // To avoid reprocessing chains

        for (start_key, &particle) in &source_particles {
            if visited_sources.contains_key(start_key) {
                continue;
            }

            let mut path_keys: SmallVec<[usize; 5]> = smallvec::smallvec![start_key];

            visited_sources.insert(start_key, ());
            let mut current_key = start_key;

            // Trace the chain of moves.
            loop {
                if let Some(next_pos) = move_graph.get(current_key) {
                    let next_key = next_pos.to_key(self.board.size.x);
                    // Cycle detected
                    if let Some(cycle_start_index) = path_keys.iter().position(|&k| k == next_key) {
                        let cycle_path = &path_keys[cycle_start_index..];
                        // If the start of the cycle is an empty square, the piece does not move.
                        if !source_particles.contains_key(next_key) {
                            final_placements.insert(start_key, particle);
                        } else {
                            // All pieces in the cycle rotate.
                            for i in 0..cycle_path.len() {
                                let from_key = cycle_path[i];
                                let to_key = cycle_path[(i + 1) % cycle_path.len()];
                                final_placements.insert(to_key, source_particles[&from_key]);
                            }
                        }
                        // Mark all nodes in the cycle as visited.
                        for &key in cycle_path {
                            visited_sources.insert(key, ());
                        }
                        break;
                    }

                    path_keys.push(next_key);
                    visited_sources.insert(next_key, ());
                    current_key = next_key;
                } else {
                    // End of a simple chain. The particle moves to the end of the chain.
                    final_placements.insert(current_key, particle);
                    break;
                }
            }
        }

        // 3. Apply moves: "lift" all moving pieces, then "place" them.
        for key in source_particles.keys() {
            let x = (key % self.board.size.x as usize) as u8;
            let y = (key / self.board.size.x as usize) as u8;
            self.board.place(Coord { x, y }, Particle::Vacuum);
        }
        for (key, particle) in final_placements {
            let x = (key % self.board.size.x as usize) as u8;
            let y = (key / self.board.size.x as usize) as u8;
            self.board.place(Coord { x, y }, particle);
        }

        // 4. Record success for all valid moves. This is a simplification; a more robust
        //    implementation might track which moves truly resulted in a state change.
        //    For now, if it was part of a valid graph, we'll call it Applied.
        for m in valid_moves {
            // Check if move was already marked as failed (e.g., occluded)
            if !results.iter().any(|(res_m, _)| res_m == &m) {
                results.push((m, MoveResult::Applied));
            }
        }

        // Add NoSource for moves from empty squares that weren't part of a longer chain.
        for &m in &moves_to_apply {
            if self.board.is_vacuum(m.from)
                && !move_graph.values().any(|&to_coord| to_coord == m.from)
            {
                if !results.iter().any(|(res_m, _)| res_m == &m) {
                    results.push((m, MoveResult::NoSource));
                }
            }
        }

        results
    }

    /// Return the list of move results if everything was gucci, else enter conflict resolution.
    #[instrument]
    pub fn try_complete_round(&mut self) -> CompleteRoundFeedback {
        let needed_moves = self.player_count as usize - self.pending_moves.len();
        if needed_moves != 0 {
            return CompleteRoundFeedback::WaitingForPlayers(needed_moves);
        }

        match self.resolve_conflicts() {
            Ok(moves_to_apply) => {
                let results = self.apply_moves(moves_to_apply);
                self.update_automaton();

                match self
                    .goals
                    .iter()
                    .copied()
                    .find(|(c, _)| c == &self.board.automaton_location)
                {
                    Some((_, who)) => {
                        self.round = RoundState::GameOver;
                        self.winner = Some(who);
                    }
                    None => {
                        self.board.clear_marks();
                        self.locked_players.clear();
                        self.round = RoundState::Fresh;
                    }
                }

                // Clear pending moves for the next round
                self.pending_moves.clear();

                CompleteRoundFeedback::CompletedMoves(results)
            }
            Err(moves_conflicted) => {
                self.round = RoundState::ResolvingConflict;

                // Lock the NON-conflicted players (they keep their moves)
                for m in &self.pending_moves {
                    if !moves_conflicted.contains(&m) && !self.locked_players.contains(&m.who) {
                        self.locked_players.push(m.who);
                    }
                }

                // Remove conflicted moves so conflicted players can resubmit
                self.pending_moves
                    .retain(|e| !moves_conflicted.contains(&e));

                CompleteRoundFeedback::Conflict(ConflictStatus {
                    conflicted_moves: moves_conflicted,
                    locked_players: self.locked_players.clone(),
                })
            }
        }
    }
}
