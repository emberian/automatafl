use std::collections::{HashMap, HashSet};

use crate::*;
use petgraph::algo::tarjan_scc;
use petgraph::graph::DiGraph;
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
    pub merge_resolution_mode: MergeResolutionMode,
    pub cycle_behavior_mode: CycleBehaviorMode,
}

impl Game {
    /// Create a new game using the given board with explicit mode configuration.
    pub fn new(
        board: Board,
        player_count: u8,
        use_column_rule: bool,
        merge_resolution_mode: MergeResolutionMode,
        cycle_behavior_mode: CycleBehaviorMode,
    ) -> Game {
        Game {
            winner: None,
            locked_players: SmallVec::new(),
            board,
            round: RoundState::Fresh,
            pending_moves: SmallVec::new(),
            goals: SmallVec::new(),
            player_count,
            use_column_rule,
            merge_resolution_mode,
            cycle_behavior_mode,
        }
    }

    /// Create a new game with default modes (BunchedStacking, RotatePieces).
    /// Convenience method for backward compatibility and simple use cases.
    pub fn new_default_modes(board: Board, player_count: u8, use_column_rule: bool) -> Game {
        Game::new(
            board,
            player_count,
            use_column_rule,
            MergeResolutionMode::DetectAndConflict,
            CycleBehaviorMode::RotatePieces,
        )
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

        return ProposeFeedback::Rejected(if self.round == RoundState::GameOver {
            GameOver
        } else if self.locked_players.contains(&m.who) {
            WaitYourTurn
        } else if m.from == m.to {
            MustMove
        } else if !(m.from.x == m.to.x || m.from.y == m.to.y) {
            AxisAlignedOnly
        } else {
            let mut cfs = CoordsFeedback {
                data: SmallVec::new(),
            };
            let from_ok = consider(&mut cfs, &self.board, m.from);
            let to_ok = consider(&mut cfs, &self.board, m.to);

            if from_ok && to_ok {
                self.pending_moves.push(m);
                if self.pending_moves.len() == self.player_count as usize {
                    return ProposeFeedback::AcceptedAndReady;
                } else {
                    return ProposeFeedback::Accepted;
                }
            }

            SeeCoords(cfs)
        });
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
        // ONLY check this if merge_resolution_mode is DetectAndConflict
        // Other modes (Annihilate, BunchBeforeMerge, BunchedStacking) handle merges during apply_moves
        if self.merge_resolution_mode == MergeResolutionMode::DetectAndConflict {
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

        // --- Phase 1: Pre-computation and Graph Building ---

        // Mark all source squares as passable for occlusion checks
        for m in &moves_to_apply {
            self.board.mark_passable(m.from);
        }

        let mut graph = DiGraph::<Coord, ()>::new();
        let mut coord_to_node = VecMap::new();
        let mut initial_piece_coords = VecMap::new();

        // Build the graph from valid, non-occluded moves
        for &m in &moves_to_apply {
            let delta = m.to - m.from;
            let axis = delta.axial_unit();
            let mut is_occluded = false;
            // Check for pieces blocking the path
            for offset in 1..delta.displacement() {
                let c = m.from + axis.scale(offset as isize);
                if self.board.particles[c.ix()].occludes() {
                    results.push((m, MoveResult::OccupiedAt(c)));
                    is_occluded = true;
                    break;
                }
            }

            if !is_occluded {
                // Add nodes for `from` and `to` if they don't exist
                let from_node = *coord_to_node
                    .entry(m.from.to_key(self.board.size.x))
                    .or_insert_with(|| graph.add_node(m.from));
                let to_node = *coord_to_node
                    .entry(m.to.to_key(self.board.size.x))
                    .or_insert_with(|| graph.add_node(m.to));

                graph.add_edge(from_node, to_node, ());

                // Record the piece at the source, if any
                if !self.board.is_vacuum(m.from) {
                    initial_piece_coords
                        .insert(m.from.to_key(self.board.size.x), self.board.particles[m.from.ix()].what);
                }
            }
        }

        // --- Phase 2: Graph Decomposition into Strongly Connected Components (SCCs) ---
        let sccs = tarjan_scc(&graph);

        // Track where each piece wants to go, along with path information
        #[derive(Debug, Clone)]
        struct PieceJourney {
            particle: Particle,
            source: Coord,
            destination: Coord,
            path: Vec<Coord>, // Full path from source to destination (inclusive)
            path_length: usize,
        }

        let mut piece_journeys: Vec<PieceJourney> = Vec::new();
        let mut resolved_piece_coords = HashSet::new();
        let mut empty_cycle_coords = HashSet::new();

        // --- Phase 3: Process Cycles (SCCs with size > 1 or self-loops) ---
        for scc in &sccs {
            let is_cycle = scc.len() > 1 || (scc.len() == 1 && graph.find_edge(scc[0], scc[0]).is_some());
            if !is_cycle {
                continue;
            }

            let scc_coords: HashSet<Coord> = scc.iter().map(|&node| graph[node]).collect();
            let pieces_in_cycle: Vec<Coord> = initial_piece_coords
                .keys()
                .map(|k| Coord { x: (k % self.board.size.x as usize) as u8, y: (k / self.board.size.x as usize) as u8 })
                .filter(|c| scc_coords.contains(c))
                .collect();

            if pieces_in_cycle.is_empty() {
                // Rule: This is an empty cycle. It cannot pull pieces in.
                for coord in scc_coords {
                    empty_cycle_coords.insert(coord);
                }
            } else {
                // Rule: This is a populated cycle.
                // Determine cycle behavior based on cycle length and configuration.

                let is_two_cycle = scc.len() == 2;
                let should_rotate = !is_two_cycle && self.cycle_behavior_mode == CycleBehaviorMode::RotatePieces;

                for start_coord in pieces_in_cycle {
                    let piece = self.board.particles[start_coord.ix()].what;
                    let dest_coord = if should_rotate {
                        // >2-cycle with RotatePieces mode: advance one position
                        let start_node = coord_to_node[&start_coord.to_key(self.board.size.x)];
                        let dest_node = graph.neighbors(start_node).next().unwrap();
                        graph[dest_node]
                    } else {
                        // 2-cycle (always stay) OR >2-cycle with NoMovement mode: piece stays in place
                        start_coord
                    };

                    let path = if dest_coord == start_coord {
                        vec![start_coord]
                    } else {
                        vec![start_coord, dest_coord]
                    };

                    piece_journeys.push(PieceJourney {
                        particle: piece,
                        source: start_coord,
                        destination: dest_coord,
                        path: path.clone(),
                        path_length: path.len() - 1,
                    });
                    resolved_piece_coords.insert(start_coord);
                }
            }
        }

        // --- Phase 4: Process Chains and pieces moving into cycles ---
        for (start_coord_key, &particle) in &initial_piece_coords {
             let start_coord = Coord {
                x: (start_coord_key % self.board.size.x as usize) as u8,
                y: (start_coord_key / self.board.size.x as usize) as u8,
            };
            if resolved_piece_coords.contains(&start_coord) {
                continue; // Already handled in a cycle
            }

            let mut path = vec![start_coord];
            let mut current_node = coord_to_node[&start_coord_key];

            loop {
                if let Some(next_node) = graph.neighbors(current_node).next() {
                    let next_coord = graph[next_node];

                    // Rule: Stop if we hit a square that is the start of another moving piece.
                    if initial_piece_coords.contains_key(next_coord.to_key(self.board.size.x)) {
                         path.push(next_coord);
                         break;
                    }

                    // Rule: Stop if moving into an empty cycle; nullifies the move.
                    if empty_cycle_coords.contains(&next_coord) {
                        path = vec![start_coord]; // Piece stays put
                        break;
                    }

                    path.push(next_coord);
                    current_node = next_node;
                } else {
                    // End of a chain
                    break;
                }
            }

            let final_coord = *path.last().unwrap();
            piece_journeys.push(PieceJourney {
                particle,
                source: start_coord,
                destination: final_coord,
                path: path.clone(),
                path_length: path.len() - 1,
            });
        }

        // --- Phase 4.5: Merge Resolution ---
        // Group journeys by destination to detect merges
        let mut journeys_by_dest: HashMap<usize, Vec<PieceJourney>> = HashMap::new();
        for journey in piece_journeys {
            journeys_by_dest
                .entry(journey.destination.to_key(self.board.size.x))
                .or_insert_with(Vec::new)
                .push(journey);
        }

        // Build final placements, resolving merges based on mode
        let mut final_placements = VecMap::new();
        let mut occupied_squares: HashSet<usize> = HashSet::new(); // Track squares occupied during bunching

        for (dest_key, mut journeys) in journeys_by_dest {
            if journeys.len() == 1 {
                // No collision - piece moves to destination
                final_placements.insert(dest_key, journeys[0].particle);
                occupied_squares.insert(dest_key);
            } else {
                // Multiple pieces want same destination - apply merge resolution mode
                match self.merge_resolution_mode {
                    MergeResolutionMode::Annihilate => {
                        // All pieces destroyed - don't add any to final_placements
                    }
                    MergeResolutionMode::BunchBeforeMerge => {
                        // Exactly like BunchedStacking, but merge point acts impassable
                        // Mark the merge point as occupied BEFORE processing
                        occupied_squares.insert(dest_key);

                        // Sort by path length: shortest first ("push through" order)
                        journeys.sort_by_key(|j| j.path_length);

                        // Process each journey in order, placing pieces as far as possible
                        // (but they can never reach the merge point since it's marked occupied)
                        for journey in journeys {
                            let mut placed = false;
                            for coord in journey.path.iter().rev() {
                                let coord_key = coord.to_key(self.board.size.x);
                                if !occupied_squares.contains(&coord_key) {
                                    final_placements.insert(coord_key, journey.particle);
                                    occupied_squares.insert(coord_key);
                                    placed = true;
                                    break;
                                }
                            }
                            // If entire path occupied, piece stays at source
                            if !placed && !journey.path.is_empty() {
                                let source_key = journey.source.to_key(self.board.size.x);
                                final_placements.insert(source_key, journey.particle);
                                occupied_squares.insert(source_key);
                            }
                        }
                    }
                    MergeResolutionMode::BunchedStacking => {
                        // Sort by path length: shortest first ("push through" order)
                        journeys.sort_by_key(|j| j.path_length);

                        // Process each journey in order, placing pieces as far as possible
                        for journey in journeys {
                            // Try to place at each position along path, starting from destination
                            let mut placed = false;
                            for coord in journey.path.iter().rev() {
                                let coord_key = coord.to_key(self.board.size.x);
                                if !occupied_squares.contains(&coord_key) {
                                    final_placements.insert(coord_key, journey.particle);
                                    occupied_squares.insert(coord_key);
                                    placed = true;
                                    break;
                                }
                            }
                            // If entire path occupied, piece doesn't move (stays at source, which should be in path[0])
                            if !placed && !journey.path.is_empty() {
                                let source_key = journey.source.to_key(self.board.size.x);
                                final_placements.insert(source_key, journey.particle);
                                occupied_squares.insert(source_key);
                            }
                        }
                    }
                    MergeResolutionMode::DetectAndConflict => {
                        // This should have been caught in resolve_conflicts
                        // If we get here, treat as annihilate
                    }
                }
            }
        }

        // --- Phase 6: Update Board State ---
        // Clear all original piece locations
        for key in initial_piece_coords.keys() {
             let coord = Coord {
                x: (key % self.board.size.x as usize) as u8,
                y: (key / self.board.size.x as usize) as u8,
            };
            self.board.place(coord, Particle::Vacuum);
        }
        // Place pieces in their final destinations
        for (key, particle) in final_placements {
            let coord = Coord {
                x: (key % self.board.size.x as usize) as u8,
                y: (key / self.board.size.x as usize) as u8,
            };
            self.board.place(coord, particle);
        }

        // --- Phase 7: Report Final Results ---
        moves_to_apply.into_iter().for_each(|m| {
            // If the move was occluded, its result is already present.
            let is_occluded = results.iter().any(|(res_m, _)| res_m == &m);
            if !is_occluded {
                if initial_piece_coords.contains_key(m.from.to_key(self.board.size.x)) {
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
