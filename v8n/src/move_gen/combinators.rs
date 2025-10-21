//! Composable move generation combinators.
//!
//! These combinators allow move generators to be composed, combined, and adapted
//! to create more sophisticated and exhaustive testing scenarios.

use super::MoveGenerator;
use automatafl_logic::*;
use rand::Rng;
use smallvec::SmallVec;

// ============================================================================
// Sequential Combinator: Try generators in sequence until enough moves
// ============================================================================

/// Tries generators in sequence until enough moves are produced.
///
/// Useful for fallback behavior: try a specialized generator first,
/// then fall back to a more general one if needed.
pub struct SequentialGen {
    generators: Vec<Box<dyn MoveGenerator>>,
    min_moves: usize,
}

impl SequentialGen {
    pub fn new(generators: Vec<Box<dyn MoveGenerator>>, min_moves: usize) -> Self {
        Self {
            generators,
            min_moves,
        }
    }
}

impl MoveGenerator for SequentialGen {
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        let mut moves = SmallVec::new();

        for generator in &mut self.generators {
            moves = generator.generate_moves(game);
            if moves.len() >= self.min_moves {
                break;
            }
        }

        moves
    }
}

// ============================================================================
// Merged Combinator: Combine moves from multiple generators
// ============================================================================

/// Merges moves from multiple generators, taking the first move from each player.
///
/// When multiple generators produce moves for the same player, takes the first one.
/// Useful for creating diverse test scenarios by combining different strategies.
pub struct MergedGen {
    generators: Vec<Box<dyn MoveGenerator>>,
}

impl MergedGen {
    pub fn new(generators: Vec<Box<dyn MoveGenerator>>) -> Self {
        Self { generators }
    }
}

impl MoveGenerator for MergedGen {
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        let mut moves = SmallVec::new();
        let mut seen_players = vec![false; game.player_count as usize];

        for generator in &mut self.generators {
            for m in generator.generate_moves(game) {
                if !seen_players[m.who.0 as usize] {
                    moves.push(m);
                    seen_players[m.who.0 as usize] = true;
                }
            }
        }

        moves
    }
}

// ============================================================================
// Filtered Combinator: Filter moves based on a predicate
// ============================================================================

/// Filters moves from a generator based on a predicate function.
///
/// Useful for creating constrained test scenarios (e.g., only short moves,
/// only moves toward/away from automaton, etc.).
pub struct FilteredGen<F>
where
    F: FnMut(&Game, &Move) -> bool,
{
    generator: Box<dyn MoveGenerator>,
    predicate: F,
}

impl<F> FilteredGen<F>
where
    F: FnMut(&Game, &Move) -> bool,
{
    pub fn new(generator: Box<dyn MoveGenerator>, predicate: F) -> Self {
        Self {
            generator,
            predicate,
        }
    }
}

impl<F> MoveGenerator for FilteredGen<F>
where
    F: FnMut(&Game, &Move) -> bool,
{
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        self.generator
            .generate_moves(game)
            .into_iter()
            .filter(|m| (self.predicate)(game, m))
            .collect()
    }
}

// ============================================================================
// Transformed Combinator: Transform moves from a generator
// ============================================================================

/// Transforms moves from a generator using a transformation function.
///
/// Useful for modifying moves in systematic ways (e.g., reversing directions,
/// scaling distances, etc.).
pub struct TransformedGen<F>
where
    F: FnMut(&Game, Move) -> Move,
{
    generator: Box<dyn MoveGenerator>,
    transform: F,
}

impl<F> TransformedGen<F>
where
    F: FnMut(&Game, Move) -> Move,
{
    pub fn new(generator: Box<dyn MoveGenerator>, transform: F) -> Self {
        Self {
            generator,
            transform,
        }
    }
}

impl<F> MoveGenerator for TransformedGen<F>
where
    F: FnMut(&Game, Move) -> Move,
{
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        self.generator
            .generate_moves(game)
            .into_iter()
            .map(|m| (self.transform)(game, m))
            .collect()
    }
}

// ============================================================================
// Weighted Combinator: Randomly choose from generators with weights
// ============================================================================

/// Randomly selects a generator based on weights.
///
/// Useful for creating varied test scenarios where different strategies
/// are used with different probabilities.
pub struct WeightedGen {
    generators: Vec<(u32, Box<dyn MoveGenerator>)>,
    rng: rand::rngs::StdRng,
}

impl WeightedGen {
    pub fn new(generators: Vec<(u32, Box<dyn MoveGenerator>)>) -> Self {
        use rand::{random, SeedableRng};
        Self {
            generators,
            rng: rand::rngs::StdRng::seed_from_u64(random()),
        }
    }

    pub fn with_seed(generators: Vec<(u32, Box<dyn MoveGenerator>)>, seed: u64) -> Self {
        use rand::SeedableRng;
        Self {
            generators,
            rng: rand::rngs::StdRng::seed_from_u64(seed),
        }
    }
}

impl MoveGenerator for WeightedGen {
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        if self.generators.is_empty() {
            return SmallVec::new();
        }

        let total_weight: u32 = self.generators.iter().map(|(w, _)| w).sum();
        let choice = self.rng.random_range(0..total_weight);

        let mut cumulative = 0;
        for (weight, generator) in &mut self.generators {
            cumulative += *weight;
            if choice < cumulative {
                return generator.generate_moves(game);
            }
        }

        // Fallback to last generator (shouldn't happen)
        self.generators.last_mut().unwrap().1.generate_moves(game)
    }
}

// ============================================================================
// Adaptive Combinator: Choose generator based on game state
// ============================================================================

/// Adaptively selects a generator based on game state analysis.
///
/// The selector function analyzes the game and returns the index of the
/// generator to use. Useful for creating intelligent test scenarios that
/// adapt to the current board state.
pub struct AdaptiveGen<F>
where
    F: FnMut(&Game) -> usize,
{
    generators: Vec<Box<dyn MoveGenerator>>,
    selector: F,
}

impl<F> AdaptiveGen<F>
where
    F: FnMut(&Game) -> usize,
{
    pub fn new(generators: Vec<Box<dyn MoveGenerator>>, selector: F) -> Self {
        Self {
            generators,
            selector,
        }
    }
}

impl<F> MoveGenerator for AdaptiveGen<F>
where
    F: FnMut(&Game) -> usize,
{
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        let index = (self.selector)(game).min(self.generators.len().saturating_sub(1));
        self.generators[index].generate_moves(game)
    }
}

// ============================================================================
// Round Robin Combinator: Cycle through generators
// ============================================================================

/// Cycles through generators in round-robin fashion.
///
/// Each call to generate_moves uses the next generator in sequence.
/// Useful for creating deterministic varied test scenarios.
pub struct RoundRobinGen {
    generators: Vec<Box<dyn MoveGenerator>>,
    current_index: usize,
}

impl RoundRobinGen {
    pub fn new(generators: Vec<Box<dyn MoveGenerator>>) -> Self {
        Self {
            generators,
            current_index: 0,
        }
    }
}

impl MoveGenerator for RoundRobinGen {
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        if self.generators.is_empty() {
            return SmallVec::new();
        }

        let moves = self.generators[self.current_index].generate_moves(game);
        self.current_index = (self.current_index + 1) % self.generators.len();
        moves
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_builders::*;
    use crate::move_gen::basic::*;

    #[test]
    fn test_sequential_gen() {
        let board = Board::stock_testing();
        let game = Game::new(board, 2, true);

        let mut generator = SequentialGen::new(
            vec![
                Box::new(ChainMakingGen::new(42)),
                Box::new(RandomMoveGen::new(42)),
            ],
            2,
        );

        let moves = generator.generate_moves(&game);
        assert!(!moves.is_empty());
    }

    #[test]
    fn test_weighted_gen() {
        let board = Board::stock_testing();
        let game = Game::new(board, 2, true);

        let mut generator = WeightedGen::with_seed(
            vec![
                (1, Box::new(ConflictSeekingGen::new(42))),
                (9, Box::new(RandomMoveGen::new(43))),
            ],
            12345,
        );

        let moves = generator.generate_moves(&game);
        assert!(!moves.is_empty());
    }

    #[test]
    fn test_round_robin_gen() {
        let board = Board::stock_testing();
        let game = Game::new(board, 2, true);

        let mut generator = RoundRobinGen::new(vec![
            Box::new(RandomMoveGen::new(42)),
            Box::new(ConflictSeekingGen::new(43)),
            Box::new(ChainMakingGen::new(44)),
        ]);

        // Generate moves three times, should cycle through generators
        let _moves1 = generator.generate_moves(&game);
        let _moves2 = generator.generate_moves(&game);
        let _moves3 = generator.generate_moves(&game);
    }

    #[test]
    fn test_filtered_gen() {
        let board = Board::stock_testing();
        let game = Game::new(board, 2, true);

        // Filter to only allow moves by player 0
        let mut generator = FilteredGen::new(
            Box::new(RandomMoveGen::new(42)),
            |_game, m| m.who.0 == 0,
        );

        let moves = generator.generate_moves(&game);
        assert!(moves.iter().all(|m| m.who.0 == 0));
    }
}
