use std::collections::HashSet;

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

        if self.round == RoundState::GameOver {
            return ProposeFeedback::Rejected(GameOver);
        }
        if self.locked_players.contains(&m.who) {
            return ProposeFeedback::Rejected(WaitYourTurn);
        }
        if m.from == m.to {
            return ProposeFeedback::Rejected(MustMove);
        }
        if !(m.from.x == m.to.x || m.from.y == m.to.y) {
            return ProposeFeedback::Rejected(AxisAlignedOnly);
        }

        let mut cfs = CoordsFeedback {
            data: SmallVec::new(),
        };
        let from_ok = consider(&mut cfs, &self.board, m.from);
        let to_ok = consider(&mut cfs, &self.board, m.to);

        if from_ok && to_ok {
            self.pending_moves.push(m);
            if self.pending_moves.len() == self.player_count as usize {
                ProposeFeedback::AcceptedAndReady
            } else {
                ProposeFeedback::Accepted
            }
        } else {
            ProposeFeedback::Rejected(SeeCoords(cfs))
        }
    }

    #[instrument]
    fn resolve_conflicts(&mut self) -> Result<SmallVec<[Move; 2]>, SmallVec<[Move; 2]>> {
        // 1. Group moves by source and destination.
        let mut moves_from = VecMap::<SmallVec<[Move; 2]>>::new();
        let mut moves_to = VecMap::<SmallVec<[Move; 2]>>::new();

        for &m in &self.pending_moves {
            moves_from
                .entry(m.from.to_key(self.board.size.x))
                .or_insert(SmallVec::new())
                .push(m);
            moves_to
                .entry(m.to.to_key(self.board.size.x))
                .or_insert(SmallVec::new())
                .push(m);
        }

        // 2. Identify all coordinates where a conflict occurs.
        let mut conflicted_coords = HashSet::new();

        // Source conflicts: Multiple unique destinations from one source.
        for (_, moves) in &moves_from {
            if moves.iter().map(|m| m.to).collect::<HashSet<_>>().len() > 1 {
                conflicted_coords.insert(moves[0].from);
            }
        }

        // Destination conflicts: Multiple unique, non-vacuum sources to one destination.
        for (_, moves) in &moves_to {
            let unique_sources = moves
                .iter()
                .filter(|m| !self.board.is_vacuum(m.from))
                .map(|m| m.from)
                .collect::<HashSet<_>>();

            if unique_sources.len() > 1 {
                conflicted_coords.insert(moves[0].to);
            }
        }

        // 3. If there are no conflicts, we're done.
        if conflicted_coords.is_empty() {
            return Ok(self.pending_moves.clone());
        }

        // 4. Otherwise, collect all moves touching conflicted coordinates and mark the board.
        let mut conflict_moves = SmallVec::new();
        for m in &self.pending_moves {
            if conflicted_coords.contains(&m.from) || conflicted_coords.contains(&m.to) {
                conflict_moves.push(*m);
            }
        }

        for coord in conflicted_coords {
            self.board.mark_conflict(coord);
        }

        Err(conflict_moves)
    }

    pub fn apply_moves(
        &mut self,
        moves_to_apply: SmallVec<[Move; 2]>,
    ) -> SmallVec<[(Move, MoveResult); 2]> {
        let mut results = SmallVec::<[(Move, MoveResult); 2]>::new();

        self.board.clear_marks();
        for m in &moves_to_apply {
            self.board.mark_passable(m.from);
        }

        let mut move_graph = vec_map::VecMap::new();
        let mut initial_pieces = vec_map::VecMap::new();

        for &m in &moves_to_apply {
            let delta = m.to - m.from;
            let axis = delta.axial_unit();
            let mut is_occluded = false;
            for offset in 1..delta.displacement() {
                let c = m.from + axis.scale(offset as isize);
                if self.board.particles[c.ix()].occludes() {
                    results.push((m, MoveResult::OccupiedAt(c)));
                    is_occluded = true;
                    break;
                }
            }

            if !is_occluded {
                let from_key = m.from.to_key(self.board.size.x);
                move_graph.insert(from_key, m.to);
                if !self.board.is_vacuum(m.from) {
                    initial_pieces.insert(from_key, self.board.particles[m.from.ix()].what);
                }
            }
        }

        let mut final_placements = vec_map::VecMap::new();
        let mut resolved_keys: HashSet<usize> = std::collections::HashSet::new();

        for (start_key, &_particle) in &initial_pieces {
            if resolved_keys.contains(&start_key) {
                continue;
            }

            // 1. Trace the full path to find its structure (chain, cycle, etc.).
            let mut path: SmallVec<[usize; 4]> = smallvec::smallvec![start_key];
            let mut current_key = start_key;
            let cycle_start_index = loop {
                if let Some(next_coord) = move_graph.get(current_key) {
                    let next_key = next_coord.to_key(self.board.size.x);
                    if let Some(index) = path.iter().position(|&k: &usize| k == next_key) {
                        break Some(index); // Cycle detected
                    }
                    path.push(next_key);
                    current_key = next_key;
                } else {
                    break None; // End of a simple chain
                }
            };

            // Mark all squares in this path sequence as handled.
            for k in &path {
                resolved_keys.insert(*k);
            }

            // 2. Apply rules based on the path's structure.
            if let Some(index) = cycle_start_index {
                // Path has a chain leading into a cycle.
                let chain_part = &path[..index];
                let cycle_part = &path[index..];

                // Rule: A piece moving into a cycle of ONLY empty squares does not move.
                let is_empty_cycle = !cycle_part.iter().any(|k| initial_pieces.contains_key(*k));
                if let Some(last_piece_idx) = chain_part
                    .iter()
                    .rposition(|k| initial_pieces.contains_key(*k))
                {
                    if is_empty_cycle {
                        let key = chain_part[last_piece_idx];
                        final_placements.insert(key, initial_pieces[&key]); // Piece stays put.
                    }
                }

                // Rule: Pieces that start inside a cycle rotate.
                for i in 0..cycle_part.len() {
                    let from_k = cycle_part[i];
                    if let Some(&p) = initial_pieces.get(from_k) {
                        let to_k = cycle_part[(i + 1) % cycle_part.len()];
                        final_placements.insert(to_k, p);
                    }
                }

                // Rule: Pieces in the chain move towards the cycle.
                for i in 0..chain_part.len() {
                    let from_k = chain_part[i];
                    if final_placements.contains_key(from_k) {
                        continue;
                    } // Already handled.

                    if let Some(&p) = initial_pieces.get(from_k) {
                        // Destination is the next piece in the chain, or the start of the cycle.
                        let mut dest_k = cycle_part[0];
                        for j in (i + 1)..chain_part.len() {
                            if initial_pieces.contains_key(chain_part[j]) {
                                dest_k = chain_part[j];
                                break;
                            }
                        }
                        final_placements.insert(dest_k, p);
                    }
                }
            } else {
                // Path is a simple chain with no cycles.
                for i in 0..path.len() {
                    let from_k = path[i];
                    if let Some(&p) = initial_pieces.get(from_k) {
                        // Destination is the next piece in the chain, or the end of the chain.
                        let mut dest_k = *path.last().unwrap();
                        for j in (i + 1)..path.len() {
                            if initial_pieces.contains_key(path[j]) {
                                dest_k = path[j];
                                break;
                            }
                        }
                        final_placements.insert(dest_k, p);
                    }
                }
            }
        }
        for key in initial_pieces.keys() {
            let coord = Coord {
                x: (key % self.board.size.x as usize) as u8,
                y: (key / self.board.size.x as usize) as u8,
            };
            self.board.place(coord, Particle::Vacuum);
        }
        for (key, particle) in final_placements {
            let coord = Coord {
                x: (key % self.board.size.x as usize) as u8,
                y: (key / self.board.size.x as usize) as u8,
            };
            self.board.place(coord, particle);
        }

        // Report success for all moves that weren't occluded.
        moves_to_apply.into_iter().for_each(|m| {
            let is_occluded = results.iter().any(|(res_m, _)| res_m == &m);
            if !is_occluded {
                if initial_pieces.contains_key(m.from.to_key(self.board.size.x)) {
                    results.push((m, MoveResult::Applied));
                } else {
                    results.push((m, MoveResult::NoSource));
                }
            }
        });

        self.board.clear_marks();
        results
    }

    #[instrument]
    pub fn try_complete_round(&mut self) -> CompleteRoundFeedback {
        let needed_moves = self.player_count as usize - self.pending_moves.len();
        if needed_moves != 0 {
            if self.round == RoundState::Fresh && !self.pending_moves.is_empty() {
                self.round = RoundState::PartiallySubmitted;
            }
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
                        // Successful round, clear everything for the next turn.
                        self.board.clear_marks();
                        self.locked_players.clear();
                        self.pending_moves.clear();
                        self.round = RoundState::Fresh;
                    }
                }
                CompleteRoundFeedback::CompletedMoves(results)
            }
            Err(moves_conflicted) => {
                self.round = RoundState::ResolvingConflict;

                self.locked_players.clear();
                for m in &self.pending_moves {
                    if !moves_conflicted.contains(&m) {
                        self.locked_players.push(m.who);
                    }
                }

                // Remove conflicted moves so players can resubmit.
                self.pending_moves.retain(|m| !moves_conflicted.contains(m));

                CompleteRoundFeedback::Conflict(ConflictStatus {
                    conflicted_moves: moves_conflicted,
                    locked_players: self.locked_players.clone(),
                })
            }
        }
    }
}
