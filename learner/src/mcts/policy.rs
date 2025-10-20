use automatafl_fastlogic::Move;
use rand::Rng;
use std::collections::HashMap;

/// Move selection policy with temperature-based sampling
pub struct MoveSelector;

impl MoveSelector {
    /// Select a move using temperature-based sampling
    ///
    /// Temperature τ controls exploration:
    /// - τ = 0: deterministic (choose most visited move)
    /// - τ = 1: proportional to visit counts
    /// - τ > 1: more uniform (more exploration)
    ///
    /// Visit count distribution is raised to power (1/τ) before sampling
    pub fn select_move(
        visit_counts: &HashMap<Move, u32>,
        temperature: f32,
        rng: &mut impl Rng,
    ) -> Option<Move> {
        if visit_counts.is_empty() {
            return None;
        }

        if temperature < 0.01 {
            // Deterministic: choose most visited
            return visit_counts
                .iter()
                .max_by_key(|(_, count)| *count)
                .map(|(m, _)| *m);
        }

        // Temperature-based sampling
        let moves: Vec<Move> = visit_counts.keys().copied().collect();
        let counts: Vec<u32> = moves.iter().map(|m| visit_counts[m]).collect();

        // Apply temperature: raise counts to power (1/temperature)
        let inv_temp = 1.0 / temperature;
        let probs: Vec<f32> = counts
            .iter()
            .map(|&c| (c as f32).powf(inv_temp))
            .collect();

        // Normalize to get probability distribution
        let total: f32 = probs.iter().sum();
        let normalized: Vec<f32> = probs.iter().map(|p| p / total).collect();

        // Sample from distribution
        let sample = rng.r#gen::<f32>();
        let mut cumulative = 0.0;

        for (i, &prob) in normalized.iter().enumerate() {
            cumulative += prob;
            if sample <= cumulative {
                return Some(moves[i]);
            }
        }

        // Fallback (shouldn't happen with proper normalization)
        Some(moves[moves.len() - 1])
    }

    /// Convert visit counts to probability distribution for training
    ///
    /// Returns a vector where index i corresponds to the action index
    /// and the value is the probability of that action
    pub fn visit_counts_to_policy(
        visit_counts: &HashMap<Move, u32>,
        action_space_size: usize,
    ) -> Vec<f32> {
        let mut policy = vec![0.0; action_space_size];

        let total: u32 = visit_counts.values().sum();
        if total == 0 {
            return policy;
        }

        for (m, &count) in visit_counts {
            let action_idx = crate::network::BoardEncoder::move_to_action(m);
            policy[action_idx] = (count as f32) / (total as f32);
        }

        policy
    }

    /// Get the most visited move (for deterministic play)
    pub fn best_move(visit_counts: &HashMap<Move, u32>) -> Option<Move> {
        visit_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(m, _)| *m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automatafl_fastlogic::{Coord, Pid};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn create_test_moves() -> Vec<Move> {
        vec![
            Move {
                who: Pid(0),
                from: Coord { x: 0, y: 0 },
                to: Coord { x: 1, y: 0 },
            },
            Move {
                who: Pid(0),
                from: Coord { x: 0, y: 0 },
                to: Coord { x: 0, y: 1 },
            },
            Move {
                who: Pid(0),
                from: Coord { x: 1, y: 1 },
                to: Coord { x: 2, y: 1 },
            },
        ]
    }

    #[test]
    fn test_deterministic_selection() {
        let moves = create_test_moves();
        let mut visit_counts = HashMap::new();
        visit_counts.insert(moves[0], 10);
        visit_counts.insert(moves[1], 5);
        visit_counts.insert(moves[2], 20); // Most visited

        let mut rng = StdRng::seed_from_u64(42);
        let selected = MoveSelector::select_move(&visit_counts, 0.0, &mut rng);

        assert_eq!(selected, Some(moves[2]));
    }

    #[test]
    fn test_temperature_sampling() {
        let moves = create_test_moves();
        let mut visit_counts = HashMap::new();
        visit_counts.insert(moves[0], 10);
        visit_counts.insert(moves[1], 5);
        visit_counts.insert(moves[2], 20);

        let mut rng = StdRng::seed_from_u64(42);

        // With temperature, should be able to select different moves
        let mut selections = HashMap::new();
        for _ in 0..100 {
            let selected = MoveSelector::select_move(&visit_counts, 1.0, &mut rng).unwrap();
            *selections.entry(selected).or_insert(0) += 1;
        }

        // Should have selected multiple different moves
        assert!(selections.len() > 1);
    }

    #[test]
    fn test_visit_counts_to_policy() {
        let moves = create_test_moves();
        let mut visit_counts = HashMap::new();
        visit_counts.insert(moves[0], 10);
        visit_counts.insert(moves[1], 5);
        visit_counts.insert(moves[2], 5);

        let policy = MoveSelector::visit_counts_to_policy(&visit_counts, 121 * 121);

        // Total probability should be 1.0
        let total: f32 = policy.iter().sum();
        assert!((total - 1.0).abs() < 0.001);

        // Most visited move should have highest probability
        let move0_idx = crate::network::BoardEncoder::move_to_action(&moves[0]);
        assert!(policy[move0_idx] > 0.4); // 10/20 = 0.5
    }

    #[test]
    fn test_best_move() {
        let moves = create_test_moves();
        let mut visit_counts = HashMap::new();
        visit_counts.insert(moves[0], 10);
        visit_counts.insert(moves[1], 5);
        visit_counts.insert(moves[2], 20);

        let best = MoveSelector::best_move(&visit_counts);
        assert_eq!(best, Some(moves[2]));
    }

    #[test]
    fn test_empty_visit_counts() {
        let visit_counts = HashMap::new();
        let mut rng = StdRng::seed_from_u64(42);

        let selected = MoveSelector::select_move(&visit_counts, 1.0, &mut rng);
        assert_eq!(selected, None);

        let best = MoveSelector::best_move(&visit_counts);
        assert_eq!(best, None);
    }
}
