use automatafl_logic::*;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::path::Path;

/// A recorded game session with initial state and move sequence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRecording {
    /// Metadata about the recording
    pub metadata: RecordingMetadata,
    /// Initial board state
    pub initial_board: Board,
    /// Number of players in the game
    pub player_count: u8,
    /// Whether to use the column rule
    pub use_column_rule: bool,
    /// Goal locations
    pub goals: SmallVec<[(Coord, Pid); 4]>,
    /// Sequence of rounds (each round contains moves from all players)
    pub rounds: Vec<Round>,
}

/// Metadata about a recording
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingMetadata {
    /// Human-readable description
    pub description: String,
    /// When this was recorded (ISO 8601 format)
    pub recorded_at: Option<String>,
    /// Total number of rounds
    pub round_count: usize,
    /// Final game outcome
    pub outcome: Option<GameOutcome>,
}

/// A single round of gameplay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Round {
    /// Moves submitted by all players this round
    pub moves: SmallVec<[Move; 2]>,
    /// Result of completing the round
    pub result: RoundResult,
}

/// Result of completing a round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoundResult {
    /// Moves completed successfully
    Completed(SmallVec<[(Move, MoveResult); 2]>),
    /// Conflict occurred (may require resubmission)
    Conflict(ConflictStatus),
}

/// Final outcome of a game
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameOutcome {
    /// A player won
    Winner(Pid),
    /// Game ended without a winner (max rounds, etc.)
    Draw,
    /// Recording was interrupted/incomplete
    Incomplete,
}

impl GameRecording {
    /// Create a new empty recording
    pub fn new(
        description: String,
        initial_board: Board,
        player_count: u8,
        use_column_rule: bool,
    ) -> Self {
        Self {
            metadata: RecordingMetadata {
                description,
                recorded_at: None,
                round_count: 0,
                outcome: None,
            },
            initial_board,
            player_count,
            use_column_rule,
            goals: SmallVec::new(),
            rounds: Vec::new(),
        }
    }

    /// Add a round to the recording
    pub fn add_round(&mut self, moves: SmallVec<[Move; 2]>, result: RoundResult) {
        self.rounds.push(Round { moves, result });
        self.metadata.round_count = self.rounds.len();
    }

    /// Set the final outcome
    pub fn set_outcome(&mut self, outcome: GameOutcome) {
        self.metadata.outcome = Some(outcome);
    }

    /// Save to a postcard binary file (compact, for benchmarks)
    #[cfg(feature = "postcard")]
    pub fn save_postcard(&self, path: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let bytes = postcard::to_stdvec(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        std::fs::write(path, bytes)
    }

    /// Load from a postcard binary file
    #[cfg(feature = "postcard")]
    pub fn load_postcard(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let bytes = std::fs::read(path)?;
        postcard::from_bytes(&bytes).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })
    }

    /// Save to a RON text file (human-readable, for tests)
    #[cfg(test)]
    pub fn save_ron(&self, path: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let pretty_config = ron::ser::PrettyConfig::new()
            .depth_limit(4)
            .separate_tuple_members(true)
            .enumerate_arrays(true);

        let s = ron::ser::to_string_pretty(self, pretty_config).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        std::fs::write(path, s)
    }

    /// Load from a RON text file
    #[cfg(test)]
    pub fn load_ron(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let s = std::fs::read_to_string(path)?;
        ron::from_str(&s).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })
    }

    /// Replay the recording and return the final game state
    ///
    /// This validates that the recording is internally consistent
    /// and returns the final game state after all rounds have been played.
    pub fn replay(&self) -> Result<Game, ReplayError> {
        let mut game = Game::new(
            self.initial_board.clone(),
            self.player_count,
            self.use_column_rule,
        );
        game.goals = self.goals.clone();

        for (round_idx, round) in self.rounds.iter().enumerate() {
            // Propose all moves for this round
            for &m in &round.moves {
                match game.propose_move(m) {
                    ProposeFeedback::Rejected(reason) => {
                        return Err(ReplayError::MoveRejected {
                            round: round_idx,
                            move_: m,
                            reason,
                        });
                    }
                    _ => {}
                }
            }

            // Complete the round
            let result = game.try_complete_round();

            // Validate the result matches what was recorded
            match (&result, &round.result) {
                (CompleteRoundFeedback::CompletedMoves(_), RoundResult::Completed(_)) => {
                    // Both completed successfully
                }
                (CompleteRoundFeedback::Conflict(_), RoundResult::Conflict(_)) => {
                    // Both had conflicts
                }
                _ => {
                    return Err(ReplayError::ResultMismatch {
                        round: round_idx,
                        expected: format!("{:?}", round.result),
                        got: format!("{:?}", result),
                    });
                }
            }
        }

        Ok(game)
    }

    /// Validate the recording without fully replaying it
    ///
    /// This performs basic sanity checks on the recording structure.
    pub fn validate(&self) -> Result<(), String> {
        if self.player_count == 0 {
            return Err("Player count must be > 0".to_string());
        }

        if self.rounds.len() != self.metadata.round_count {
            return Err(format!(
                "Metadata round count ({}) doesn't match actual rounds ({})",
                self.metadata.round_count,
                self.rounds.len()
            ));
        }

        // Check board size is reasonable
        if self.initial_board.size.x == 0 || self.initial_board.size.y == 0 {
            return Err("Board size must be > 0".to_string());
        }

        Ok(())
    }
}

/// Errors that can occur during replay
#[derive(Debug, Clone)]
pub enum ReplayError {
    /// A move was rejected that should have been accepted
    MoveRejected {
        round: usize,
        move_: Move,
        reason: MoveFeedback,
    },
    /// The result didn't match what was recorded
    ResultMismatch {
        round: usize,
        expected: String,
        got: String,
    },
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ReplayError::MoveRejected { round, move_, reason } => {
                write!(
                    f,
                    "Round {}: Move {:?} was rejected: {}",
                    round, move_, reason
                )
            }
            ReplayError::ResultMismatch { round, expected, got } => {
                write!(
                    f,
                    "Round {}: Result mismatch - expected {:?}, got {:?}",
                    round, expected, got
                )
            }
        }
    }
}

impl std::error::Error for ReplayError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_recording() {
        let board = Board::stock_testing();
        let rec = GameRecording::new("Test".to_string(), board, 2, true);

        assert_eq!(rec.rounds.len(), 0);
        assert_eq!(rec.metadata.round_count, 0);
        rec.validate().expect("Empty recording should be valid");
    }

    #[test]
    fn test_replay_empty_recording() {
        let board = Board::stock_testing();
        let rec = GameRecording::new("Test".to_string(), board, 2, true);

        let game = rec.replay().expect("Replay should succeed");
        assert_eq!(game.round, RoundState::Fresh);
    }
}
