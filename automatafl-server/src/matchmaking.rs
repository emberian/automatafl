//! Matchmaking queue and open lobby coordination for Automatafl (`R5`).
//!
//! Supports:
//! - Automatic matchmaking queues grouped by compatible `GameConfig` (or `BoardPreset` pool).
//! - Ticket tracking (`SeekStatus::{Queued, Matched, Cancelled}`) so clients
//!   polling or listening can transition directly into their assigned seat with a private
//!   `seat_token` without leaking the opponent's token.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::protocol::{BoardPreset, GameConfig, Seat};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeekRequest {
    #[serde(default)]
    pub player_name: Option<String>,
    #[serde(default)]
    pub config: GameConfig,
    /// When true, matches any queued player with the same `BoardPreset` even if minor
    /// rule toggles differ (adopting the earlier seeker's config). When false, requires
    /// an exact `GameConfig` match.
    #[serde(default = "default_true")]
    pub any_variant_with_same_board: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SeekRequest {
    fn default() -> Self {
        Self {
            player_name: None,
            config: GameConfig::default(),
            any_variant_with_same_board: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SeekStatus {
    Queued {
        ticket_id: String,
        preset: BoardPreset,
        position_in_queue: usize,
    },
    Matched {
        ticket_id: String,
        game_id: String,
        seat: Seat,
        seat_token: String,
    },
    Cancelled {
        ticket_id: String,
    },
}

#[derive(Clone, Debug)]
struct QueuedSeeker {
    ticket_id: String,
    request: SeekRequest,
}

#[derive(Clone, Debug)]
pub struct MatchAssignment {
    pub ticket_a: String,
    pub name_a: Option<String>,
    pub ticket_b: String,
    pub name_b: Option<String>,
    pub config: GameConfig,
}

pub const DEFAULT_MAX_COMPLETED_TICKETS: usize = 512;

#[derive(Debug)]
pub struct MatchmakingQueue {
    next_ticket_seq: u64,
    waiting: VecDeque<QueuedSeeker>,
    statuses: HashMap<String, SeekStatus>,
    completed_order: VecDeque<String>,
    max_completed_tickets: usize,
}

impl Default for MatchmakingQueue {
    fn default() -> Self {
        Self::with_max_completed_tickets(DEFAULT_MAX_COMPLETED_TICKETS)
    }
}

impl MatchmakingQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_completed_tickets(max_completed_tickets: usize) -> Self {
        Self {
            next_ticket_seq: 0,
            waiting: VecDeque::new(),
            statuses: HashMap::new(),
            completed_order: VecDeque::new(),
            max_completed_tickets: max_completed_tickets.max(2),
        }
    }

    /// Enqueue a matchmaking request. If a compatible opponent is already waiting,
    /// pops that opponent and returns `Ok((ticket_id, Some(MatchAssignment)))` so the
    /// caller can instantiate the `GameSession` and call `record_match`.
    pub fn enqueue(
        &mut self,
        request: SeekRequest,
    ) -> Result<(String, Option<MatchAssignment>), String> {
        if request.config.max_turns == 0 {
            return Err("max_turns must be > 0".to_string());
        }
        self.next_ticket_seq += 1;
        let ticket_id = format!("seek-{}", self.next_ticket_seq);

        let compatible_idx = self
            .waiting
            .iter()
            .position(|queued| Self::configs_compatible(&queued.request, &request));

        if let Some(idx) = compatible_idx {
            let first = self.waiting.remove(idx).expect("index just located");
            let assignment = MatchAssignment {
                ticket_a: first.ticket_id,
                name_a: first.request.player_name,
                ticket_b: ticket_id.clone(),
                name_b: request.player_name,
                config: first.request.config,
            };
            self.refresh_positions();
            Ok((ticket_id, Some(assignment)))
        } else {
            let preset = request.config.preset;
            self.waiting.push_back(QueuedSeeker {
                ticket_id: ticket_id.clone(),
                request,
            });
            self.refresh_positions();
            let pos = self.waiting.len();
            self.statuses.insert(
                ticket_id.clone(),
                SeekStatus::Queued {
                    ticket_id: ticket_id.clone(),
                    preset,
                    position_in_queue: pos,
                },
            );
            Ok((ticket_id, None))
        }
    }

    /// Record the created game and seat tokens for a pair of matched tickets,
    /// and prune old completed tickets beyond capacity.
    pub fn record_match(
        &mut self,
        assignment: &MatchAssignment,
        game_id: &str,
        token_a: &str,
        token_b: &str,
    ) {
        self.statuses.insert(
            assignment.ticket_a.clone(),
            SeekStatus::Matched {
                ticket_id: assignment.ticket_a.clone(),
                game_id: game_id.to_string(),
                seat: Seat::A,
                seat_token: token_a.to_string(),
            },
        );
        self.statuses.insert(
            assignment.ticket_b.clone(),
            SeekStatus::Matched {
                ticket_id: assignment.ticket_b.clone(),
                game_id: game_id.to_string(),
                seat: Seat::B,
                seat_token: token_b.to_string(),
            },
        );
        self.completed_order.push_back(assignment.ticket_a.clone());
        self.completed_order.push_back(assignment.ticket_b.clone());
        self.prune_completed(self.max_completed_tickets);
    }

    pub fn status(&self, ticket_id: &str) -> Option<SeekStatus> {
        self.statuses.get(ticket_id).cloned()
    }

    pub fn cancel(&mut self, ticket_id: &str) -> Option<SeekStatus> {
        if let Some(idx) = self.waiting.iter().position(|q| q.ticket_id == ticket_id) {
            self.waiting.remove(idx);
            self.refresh_positions();
            let status = SeekStatus::Cancelled {
                ticket_id: ticket_id.to_string(),
            };
            self.statuses.insert(ticket_id.to_string(), status.clone());
            self.completed_order.push_back(ticket_id.to_string());
            self.prune_completed(self.max_completed_tickets);
            return Some(status);
        }
        self.statuses.get(ticket_id).cloned()
    }

    /// Evict oldest completed (`Matched` or `Cancelled`) tickets so `statuses` stays bounded.
    pub fn prune_completed(&mut self, max_completed: usize) {
        while self.completed_order.len() > max_completed {
            if let Some(old_id) = self.completed_order.pop_front() {
                if !self.waiting.iter().any(|q| q.ticket_id == old_id) {
                    self.statuses.remove(&old_id);
                }
            }
        }
    }

    pub fn waiting_count(&self) -> usize {
        self.waiting.len()
    }

    pub fn completed_count(&self) -> usize {
        self.completed_order.len()
    }

    fn refresh_positions(&mut self) {
        for (i, queued) in self.waiting.iter().enumerate() {
            self.statuses.insert(
                queued.ticket_id.clone(),
                SeekStatus::Queued {
                    ticket_id: queued.ticket_id.clone(),
                    preset: queued.request.config.preset,
                    position_in_queue: i + 1,
                },
            );
        }
    }

    fn configs_compatible(a: &SeekRequest, b: &SeekRequest) -> bool {
        if a.config == b.config {
            return true;
        }
        a.any_variant_with_same_board
            && b.any_variant_with_same_board
            && a.config.preset == b.config.preset
    }
}
