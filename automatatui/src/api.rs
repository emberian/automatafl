use automatafl_logic::{
    Coord, Game, Move, MoveFeedback, MoveResult as LogicMoveResult, Pid, RoundState,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct GameSnapshot {
    pub id: Uuid,
    pub game: Game,
    pub players: Vec<PlayerSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerSummary {
    pub uuid: Uuid,
    pub pid: Pid,
    pub displayname: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GameEvent {
    PlayerJoined {
        player: PlayerSummary,
    },
    MoveQueued {
        pid: Pid,
    },
    MoveRejected {
        pid: Pid,
        feedback: MoveFeedback,
    },
    ConflictsDetected {
        moves: Vec<Move>,
    },
    MovesResolved {
        results: Vec<AppliedMove>,
        automaton: Option<AutomatonMotion>,
        winner: Option<Pid>,
    },
    RoundState {
        round: RoundState,
        locked: Vec<Pid>,
    },
}

#[derive(Debug, Deserialize)]
pub struct AppliedMove {
    pub mv: Move,
    pub outcome: LogicMoveResult,
}

#[derive(Debug, Deserialize)]
pub struct AutomatonMotion {
    pub from: Coord,
    pub to: Coord,
}

#[derive(Debug, Deserialize)]
pub struct EventEnvelope {
    pub seq: u64,
    pub event: GameEvent,
}

#[derive(Debug, Deserialize)]
pub struct EventsResponse {
    pub events: Vec<EventEnvelope>,
    pub next_seq: u64,
}

#[derive(Debug, Deserialize)]
pub struct MoveAttemptResult {
    pub feedback: MoveFeedback,
    pub enqueued: bool,
}

#[derive(Serialize)]
pub struct RegisterPlayerRequest<'a> {
    pub displayname: &'a str,
    pub password: &'a str,
}

#[derive(Serialize)]
pub struct PerformMoveRequest {
    pub from: Coord,
    pub to: Coord,
}
