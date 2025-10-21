use super::MoveGenerator;
use automatafl_logic::*;
use rand::Rng;
use smallvec::SmallVec;
use std::collections::HashSet;

/// Find all movable pieces on the board (non-vacuum, non-automaton)
pub fn find_movable_pieces(board: &Board) -> Vec<Coord> {
    let mut pieces = Vec::new();
    for y in 0..board.size.y {
        for x in 0..board.size.x {
            let coord = Coord { x, y };
            if !board.is_vacuum(coord) && !board.is_automaton(coord) && !board.is_conflict(coord) {
                pieces.push(coord);
            }
        }
    }
    pieces
}

/// Generate a random valid move for a piece
pub fn random_valid_move<R: Rng>(
    rng: &mut R,
    board: &Board,
    from: Coord,
) -> Option<Coord> {
    // Choose random direction
    let directions = [Delta::XP, Delta::XN, Delta::YP, Delta::YN];
    let dir = directions[rng.gen_range(0..4)];

    // Choose random distance (1 to 5 cells)
    let max_dist = 5.min(board.size.x.max(board.size.y) as usize);
    let dist = rng.gen_range(1..=max_dist);

    let to = from + dir.scale(dist as isize);

    // Check if the move is valid (in bounds, not to automaton, not to conflict)
    if board.inbounds(to) && !board.is_automaton(to) && !board.is_conflict(to) && from != to {
        Some(to)
    } else {
        None
    }
}

// ============================================================================
// Random Move Generator
// ============================================================================

/// Generates random valid moves for all players
pub struct RandomMoveGen {
    rng: rand::rngs::StdRng,
}

impl RandomMoveGen {
    pub fn new(seed: u64) -> Self {
        use rand::SeedableRng;
        Self {
            rng: rand::rngs::StdRng::seed_from_u64(seed),
        }
    }

    pub fn new_random() -> Self {
        use rand::{SeedableRng, Rng};
        Self {
            rng: rand::rngs::StdRng::seed_from_u64(rand::random()),
        }
    }
}

impl MoveGenerator for RandomMoveGen {
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        let mut moves = SmallVec::new();
        let pieces = find_movable_pieces(&game.board);

        if pieces.is_empty() {
            return moves;
        }

        for player_id in 0..game.player_count {
            // Skip locked players
            let pid = Pid(player_id);
            if game.locked_players.contains(&pid) {
                continue;
            }

            // Pick a random piece and generate a random move
            let mut attempts = 0;
            while attempts < 10 {
                let from = pieces[self.rng.gen_range(0..pieces.len())];
                if let Some(to) = random_valid_move(&mut self.rng, &game.board, from) {
                    moves.push(Move { who: pid, from, to });
                    break;
                }
                attempts += 1;
            }
        }

        moves
    }
}

// ============================================================================
// Conflict-Seeking Move Generator
// ============================================================================

/// Generates moves that intentionally create conflicts
pub struct ConflictSeekingGen {
    rng: rand::rngs::StdRng,
}

impl ConflictSeekingGen {
    pub fn new(seed: u64) -> Self {
        use rand::SeedableRng;
        Self {
            rng: rand::rngs::StdRng::seed_from_u64(seed),
        }
    }
}

impl MoveGenerator for ConflictSeekingGen {
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        let mut moves = SmallVec::new();
        let pieces = find_movable_pieces(&game.board);

        if pieces.is_empty() || game.player_count < 2 {
            return moves;
        }

        // Strategy: Try to move different pieces to the same destination (destination conflict)
        // or move the same piece to different destinations (source conflict)

        if self.rng.gen_bool(0.5) {
            // Destination conflict: multiple pieces to same spot
            let target = pieces[self.rng.gen_range(0..pieces.len())];

            for player_id in 0..game.player_count.min(2) {
                let pid = Pid(player_id);
                if game.locked_players.contains(&pid) {
                    continue;
                }

                if player_id == 0 {
                    // First player picks a random piece
                    let from = pieces[self.rng.gen_range(0..pieces.len())];
                    if from != target && game.board.inbounds(target) {
                        moves.push(Move {
                            who: pid,
                            from,
                            to: target,
                        });
                    }
                } else {
                    // Second player tries to move to the same target
                    let from = pieces[self.rng.gen_range(0..pieces.len())];
                    if from != target && game.board.inbounds(target) && from != moves[0].from {
                        moves.push(Move {
                            who: pid,
                            from,
                            to: target,
                        });
                    }
                }
            }
        } else {
            // Source conflict: same piece to different destinations
            let shared_source = pieces[self.rng.gen_range(0..pieces.len())];

            for player_id in 0..game.player_count.min(2) {
                let pid = Pid(player_id);
                if game.locked_players.contains(&pid) {
                    continue;
                }

                if let Some(to) = random_valid_move(&mut self.rng, &game.board, shared_source) {
                    moves.push(Move {
                        who: pid,
                        from: shared_source,
                        to,
                    });
                }
            }
        }

        // Fill in remaining players with random moves
        for player_id in moves.len() as u8..game.player_count {
            let pid = Pid(player_id);
            if game.locked_players.contains(&pid) {
                continue;
            }

            let from = pieces[self.rng.gen_range(0..pieces.len())];
            if let Some(to) = random_valid_move(&mut self.rng, &game.board, from) {
                moves.push(Move { who: pid, from, to });
            }
        }

        moves
    }
}

// ============================================================================
// Chain-Making Move Generator
// ============================================================================

/// Generates moves that create chains and cycles of pieces
pub struct ChainMakingGen {
    rng: rand::rngs::StdRng,
}

impl ChainMakingGen {
    pub fn new(seed: u64) -> Self {
        use rand::SeedableRng;
        Self {
            rng: rand::rngs::StdRng::seed_from_u64(seed),
        }
    }
}

impl MoveGenerator for ChainMakingGen {
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        let mut moves = SmallVec::new();
        let pieces = find_movable_pieces(&game.board);

        if pieces.len() < game.player_count as usize {
            // Not enough pieces, fall back to random
            let seed: u64 = rand::random();
            return RandomMoveGen::new(seed).generate_moves(game);
        }

        // Try to create a chain or cycle
        let chain_length = game.player_count.min(pieces.len() as u8);

        // Pick pieces for the chain
        let mut chain_pieces = Vec::new();
        let mut used_indices = HashSet::new();

        for _ in 0..chain_length {
            let mut idx;
            loop {
                idx = self.rng.gen_range(0..pieces.len());
                if !used_indices.contains(&idx) {
                    break;
                }
            }
            used_indices.insert(idx);
            chain_pieces.push(pieces[idx]);
        }

        // Decide if this is a cycle or a chain
        let is_cycle = self.rng.gen_bool(0.3);

        for (i, &from) in chain_pieces.iter().enumerate() {
            let pid = Pid(i as u8);
            if game.locked_players.contains(&pid) {
                continue;
            }

            // Determine destination: next piece in chain (or first for cycle)
            let to = if i == chain_pieces.len() - 1 {
                if is_cycle {
                    chain_pieces[0] // Cycle back to start
                } else {
                    // Last piece in chain, move to a random valid spot
                    if let Some(dest) = random_valid_move(&mut self.rng, &game.board, from) {
                        dest
                    } else {
                        continue;
                    }
                }
            } else {
                chain_pieces[i + 1] // Move to next piece in chain
            };

            if from != to {
                moves.push(Move { who: pid, from, to });
            }
        }

        moves
    }
}

// ============================================================================
// Goal-Oriented Move Generator
// ============================================================================

/// Generates moves that strategically move pieces toward goals
pub struct GoalOrientedGen {
    rng: rand::rngs::StdRng,
}

impl GoalOrientedGen {
    pub fn new(seed: u64) -> Self {
        use rand::SeedableRng;
        Self {
            rng: rand::rngs::StdRng::seed_from_u64(seed),
        }
    }
}

impl MoveGenerator for GoalOrientedGen {
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        let mut moves = SmallVec::new();
        let pieces = find_movable_pieces(&game.board);

        if pieces.is_empty() {
            return moves;
        }

        for player_id in 0..game.player_count {
            let pid = Pid(player_id);
            if game.locked_players.contains(&pid) {
                continue;
            }

            // Find this player's goal (if any)
            let goal = game.goals.iter().find(|(_, p)| *p == pid).map(|(c, _)| c);

            let from = pieces[self.rng.gen_range(0..pieces.len())];

            let to = if let Some(&goal_coord) = goal {
                // Try to move toward the goal
                let delta = goal_coord - from;

                // Prefer the axis with larger difference
                let preferred_dir = if delta.dx.abs() > delta.dy.abs() {
                    if delta.dx > 0 {
                        Delta::XP
                    } else {
                        Delta::XN
                    }
                } else if delta.dy != 0 {
                    if delta.dy > 0 {
                        Delta::YP
                    } else {
                        Delta::YN
                    }
                } else {
                    // Already at goal on one axis, pick random
                    [Delta::XP, Delta::XN, Delta::YP, Delta::YN][self.rng.gen_range(0..4)]
                };

                // Move in that direction
                let dist = self.rng.gen_range(1..=3) as isize;
                let candidate = from + preferred_dir.scale(dist);

                if game.board.inbounds(candidate)
                    && !game.board.is_automaton(candidate)
                    && !game.board.is_conflict(candidate)
                {
                    candidate
                } else {
                    // Fallback to random
                    random_valid_move(&mut self.rng, &game.board, from).unwrap_or(from)
                }
            } else {
                // No goal, just random
                random_valid_move(&mut self.rng, &game.board, from).unwrap_or(from)
            };

            if from != to {
                moves.push(Move { who: pid, from, to });
            }
        }

        moves
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_movable_pieces() {
        let board = Board::stock_testing();
        let pieces = find_movable_pieces(&board);

        // Should find repulsors and attractors, but not automaton or vacuum
        assert!(!pieces.is_empty());
        assert!(!pieces.contains(&board.automaton_location));
    }

    #[test]
    fn test_random_move_gen() {
        let board = Board::stock_testing();
        let game = Game::new_default_modes(board, 2, true);
        let mut generator = RandomMoveGen::new(42);

        let moves = generator.generate_moves(&game);
        assert_eq!(moves.len(), 2); // Should generate moves for both players
    }

    #[test]
    fn test_chain_making_gen() {
        let board = Board::stock_testing();
        let game = Game::new_default_modes(board, 2, true);
        let mut generator = ChainMakingGen::new(42);

        let moves = generator.generate_moves(&game);
        assert!(!moves.is_empty());
    }
}
