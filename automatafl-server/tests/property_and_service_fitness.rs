//! Comprehensive property-based (`proptest`), cross-engine JS/Rust differential,
//! stateful API state-machine, and composed end-to-end service fitness & load test suite.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use tower::ServiceExt;

use automatafl_client::app::{ClientApp, ClientMode, KeyCommand};
use automatafl_client::remote::RemoteClient;
use automatafl_server::api::{
    build_router, AppState, CreateGameResponse, JoinGameResponse, ReplayExport,
};
use automatafl_server::bot::BotStrategy;
use automatafl_server::matchmaking::{SeekRequest, SeekStatus};
use automatafl_server::oracle;
use automatafl_server::persistence::GameStore;
use automatafl_server::protocol::{
    Board, BoardPreset, CommitMode, ConfluenceMode, Coord, GameConfig, Move, RoundOutcome, Seat,
    TieBreakMode, ATT, AUTO, REP, VAC,
};
use automatafl_server::session::{FinishReason, GameSession, GameView, Phase};
use automatafl_server::web_ui::INDEX_HTML;

// ============================================================================
// Helper: In-process Axum request dispatcher for fast stateful proptest steps
// ============================================================================

async fn send_req(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(tok) = token {
        builder = builder.header("Authorization", format!("Bearer {tok}"));
    }
    let req_body = match body {
        Some(v) => {
            builder = builder.header("Content-Type", "application/json");
            Body::from(serde_json::to_vec(&v).unwrap())
        }
        None => Body::empty(),
    };
    let resp = app
        .clone()
        .oneshot(builder.body(req_body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let val = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, val)
}

// ============================================================================
// Requirement 2: Stateful Property-Based Session & API State Machine Fuzzing
// ============================================================================

#[derive(Clone, Copy, Debug)]
enum TokenKind {
    ValidSeatA,
    ValidSeatB,
    SpectatorNone,
    InvalidToken,
    PreJoinCreatorToken,
}

#[derive(Clone, Copy, Debug)]
enum ViewerKind {
    SeatA,
    SeatB,
    Spectator,
    InvalidToken,
    PreJoinToken,
}

#[derive(Clone, Debug)]
enum MoveOrGarbage {
    IndexedMove { piece_idx: usize, target_idx: usize },
    RawMove { from: Coord, to: Coord },
    GarbageHash(String),
}

#[derive(Clone, Debug)]
enum ExpectedRevKind {
    None,
    Current,
    StaleOffset(u64),
    FutureOffset(u64),
}

#[derive(Clone, Debug)]
enum FuzzAction {
    SelectPiece {
        seat: Seat,
        coord: Option<Coord>,
        token_kind: TokenKind,
    },
    SubmitMove {
        seat: Seat,
        from: Coord,
        to: Coord,
        expected_rev: ExpectedRevKind,
        token_kind: TokenKind,
    },
    CommitHash {
        seat: Seat,
        move_or_garbage: MoveOrGarbage,
        salt: String,
        expected_rev: ExpectedRevKind,
        token_kind: TokenKind,
    },
    RevealMove {
        seat: Seat,
        from: Coord,
        to: Coord,
        salt: String,
        use_matching_committed: bool,
        token_kind: TokenKind,
    },
    TriggerResolve {
        token_kind: TokenKind,
    },
    Resign {
        seat: Seat,
        token_kind: TokenKind,
    },
    PollView {
        viewer_kind: ViewerKind,
    },
    EvictAndLazyReloadFromDisk,
    VerifyReplay,
}

fn arb_seat() -> impl Strategy<Value = Seat> {
    prop_oneof![Just(Seat::A), Just(Seat::B)]
}

fn arb_token_kind() -> impl Strategy<Value = TokenKind> {
    prop_oneof![
        3 => Just(TokenKind::ValidSeatA),
        3 => Just(TokenKind::ValidSeatB),
        1 => Just(TokenKind::SpectatorNone),
        1 => Just(TokenKind::InvalidToken),
        1 => Just(TokenKind::PreJoinCreatorToken),
    ]
}

fn arb_viewer_kind() -> impl Strategy<Value = ViewerKind> {
    prop_oneof![
        Just(ViewerKind::SeatA),
        Just(ViewerKind::SeatB),
        Just(ViewerKind::Spectator),
        Just(ViewerKind::InvalidToken),
        Just(ViewerKind::PreJoinToken),
    ]
}

fn arb_expected_rev() -> impl Strategy<Value = ExpectedRevKind> {
    prop_oneof![
        3 => Just(ExpectedRevKind::None),
        3 => Just(ExpectedRevKind::Current),
        2 => (1u64..=5).prop_map(ExpectedRevKind::StaleOffset),
        1 => (1u64..=5).prop_map(ExpectedRevKind::FutureOffset),
    ]
}

fn arb_coord() -> impl Strategy<Value = Coord> {
    (-1i32..=11, -1i32..=11)
}

fn arb_game_config() -> impl Strategy<Value = GameConfig> {
    (
        prop_oneof![
            Just(BoardPreset::Mini5x5),
            Just(BoardPreset::Blitz7x7),
            Just(BoardPreset::Tactical9x9),
            Just(BoardPreset::Classic11x11),
        ],
        prop_oneof![
            Just(TieBreakMode::Column),
            Just(TieBreakMode::Row),
            Just(TieBreakMode::Freeze),
            Just(TieBreakMode::Momentum {
                prefer_same_axis: true
            }),
            Just(TieBreakMode::Momentum {
                prefer_same_axis: false
            }),
        ],
        prop_oneof![
            Just(ConfluenceMode::Conflict),
            Just(ConfluenceMode::BunchBefore),
            Just(ConfluenceMode::Annihilate),
        ],
        any::<bool>(),
        0u32..=3,
        any::<bool>(),
        1u32..=3,
        prop_oneof![
            Just(CommitMode::AutoRevealOnBothSealed),
            Just(CommitMode::ExplicitCommitReveal),
        ],
    )
        .prop_map(
            |(
                preset,
                tie_break,
                confluence,
                two_cycle_swaps,
                cooldown_turns,
                railgun_fling,
                automata_count,
                commit_mode,
            )| GameConfig {
                preset,
                tie_break,
                confluence,
                two_cycle_swaps,
                cooldown_turns,
                max_turns: 40,
                railgun_fling,
                automata_count,
                commit_mode,
            },
        )
}

fn arb_fuzz_action() -> impl Strategy<Value = FuzzAction> {
    prop_oneof![
        3 => (arb_seat(), proptest::option::of(arb_coord()), arb_token_kind()).prop_map(
            |(seat, coord, token_kind)| FuzzAction::SelectPiece {
                seat,
                coord,
                token_kind,
            }
        ),
        5 => (
            arb_seat(),
            arb_coord(),
            arb_coord(),
            arb_expected_rev(),
            arb_token_kind()
        )
            .prop_map(|(seat, from, to, expected_rev, token_kind)| {
                FuzzAction::SubmitMove {
                    seat,
                    from,
                    to,
                    expected_rev,
                    token_kind,
                }
            }),
        4 => (
            arb_seat(),
            prop_oneof![
                (0usize..32, 0usize..32).prop_map(|(piece_idx, target_idx)| {
                    MoveOrGarbage::IndexedMove {
                        piece_idx,
                        target_idx,
                    }
                }),
                (arb_coord(), arb_coord())
                    .prop_map(|(from, to)| MoveOrGarbage::RawMove { from, to }),
                "[0-9a-z]{8,20}".prop_map(MoveOrGarbage::GarbageHash),
            ],
            "[a-z0-9]{4,10}",
            arb_expected_rev(),
            arb_token_kind()
        )
            .prop_map(
                |(seat, move_or_garbage, salt, expected_rev, token_kind)| FuzzAction::CommitHash {
                    seat,
                    move_or_garbage,
                    salt,
                    expected_rev,
                    token_kind,
                }
            ),
        4 => (
            arb_seat(),
            arb_coord(),
            arb_coord(),
            "[a-z0-9]{4,10}",
            any::<bool>(),
            arb_token_kind()
        )
            .prop_map(
                |(seat, from, to, salt, use_matching_committed, token_kind)| {
                    FuzzAction::RevealMove {
                        seat,
                        from,
                        to,
                        salt,
                        use_matching_committed,
                        token_kind,
                    }
                }
            ),
        3 => arb_token_kind().prop_map(|token_kind| FuzzAction::TriggerResolve { token_kind }),
        1 => (arb_seat(), arb_token_kind())
            .prop_map(|(seat, token_kind)| FuzzAction::Resign { seat, token_kind }),
        2 => arb_viewer_kind().prop_map(|viewer_kind| FuzzAction::PollView { viewer_kind }),
        2 => Just(FuzzAction::EvictAndLazyReloadFromDisk),
        2 => Just(FuzzAction::VerifyReplay),
    ]
}

/// Pick a legal `(from, to)` move for `session` using `seed_coords` as deterministic hints when
/// raw coordinates are not directly legal, so the fuzzer frequently advances multi-turn games
/// while still exercising illegal coordinates when `use_raw` is chosen.
fn pick_legal_move_or_raw(session: &GameSession, from: Coord, to: Coord) -> (Coord, Coord) {
    let n = session.board.n as i32;
    // If `from` is out of bounds or negative, keep raw coordinates to test error rejection
    if from.0 < 0 || from.1 < 0 || to.0 < 0 || to.1 < 0 {
        return (from, to);
    }
    let mut pieces = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let c = (x, y);
            let p = session.board.cell_at(c);
            if (p == REP || p == ATT)
                && c != session.board.auto
                && !session.marks.contains(&c)
                && !session.is_on_cooldown(c)
            {
                pieces.push(c);
            }
        }
    }
    if pieces.is_empty() {
        return (from, to);
    }
    let src = pieces[(from.0 as usize + from.1 as usize * 7) % pieces.len()];
    let targets = oracle::executable_targets(&session.board, &session.marks, &session.locked, 0, src)
        .unwrap_or_default();
    if targets.is_empty() {
        let legals = oracle::legal_targets(&session.board, &session.marks, 0, src).unwrap_or_default();
        if legals.is_empty() {
            return (from, to);
        }
        let dst = legals[(to.0 as usize + to.1 as usize * 5) % legals.len()];
        return (src, dst);
    }
    let dst = targets[(to.0 as usize + to.1 as usize * 5) % targets.len()];
    (src, dst)
}

/// Assert all 4 universal invariants on the active game in `state`!
async fn assert_universal_service_invariants(
    state: &Arc<AppState>,
    app: &axum::Router,
    game_id: &str,
    token_a: &str,
    token_b: &str,
    rotated_prejoin_token: &str,
) {
    let session_arc = state
        .get_or_load_session(game_id)
        .expect("session must exist in RAM or disk");
    let snap = session_arc.lock().unwrap().clone();
    drop(session_arc);

    // ------------------------------------------------------------------------
    // Invariant 1: Strict Fog-of-War Isolation
    // ------------------------------------------------------------------------
    let spec_view = snap.view_for(None);
    let view_a = snap.view_for(Some(Seat::A));
    let view_b = snap.view_for(Some(Seat::B));

    assert_eq!(spec_view.viewer_seat, None);
    assert_eq!(view_a.viewer_seat, Some(Seat::A));
    assert_eq!(view_b.viewer_seat, Some(Seat::B));

    for idx in 0..2 {
        let seat_spec = &spec_view.seats[idx];
        // Spectator NEVER sees own_selection or own_move_forecast
        assert_eq!(
            seat_spec.own_selection, None,
            "Spectator view must never leak own_selection for seat {idx}"
        );
        assert_eq!(
            seat_spec.own_move_forecast, None,
            "Spectator view must never leak own_move_forecast for seat {idx}"
        );
        // Spectator NEVER sees uncommitted/unrevealed move unless it is locked from a prior sub-round or post-reveal
        if !seat_spec.is_locked && !seat_spec.has_revealed {
            assert_eq!(
                seat_spec.visible_move, None,
                "Spectator view leaked unrevealed move for seat {idx}"
            );
        }
        assert!(
            seat_spec.legal_targets.is_empty(),
            "Spectator view leaked legal_targets for seat {idx}"
        );
    }
    // Also verify no cell in spectator view is marked selectable, proposable_target, executable_target, or sealed
    for cell in &spec_view.cells {
        assert!(!cell.is_selectable, "Spectator cell leaked is_selectable");
        assert!(
            !cell.is_proposable_target,
            "Spectator cell leaked is_proposable_target"
        );
        assert!(
            !cell.is_executable_target,
            "Spectator cell leaked is_executable_target"
        );
        assert_ne!(cell.tag.as_ref(), "sealed", "Spectator cell leaked sealed tag");
    }

    // Seat A's GameView NEVER exposes Seat B's unrevealed move, selection, or forecast
    assert_eq!(
        view_a.seats[1].own_selection, None,
        "Seat A view leaked Seat B own_selection"
    );
    assert_eq!(
        view_a.seats[1].own_move_forecast, None,
        "Seat A view leaked Seat B own_move_forecast"
    );
    assert!(
        view_a.seats[1].legal_targets.is_empty(),
        "Seat A view leaked Seat B legal_targets"
    );
    if !view_a.seats[1].is_locked && !view_a.seats[1].has_revealed {
        assert_eq!(
            view_a.seats[1].visible_move, None,
            "Seat A view leaked Seat B unrevealed move"
        );
    }

    // Seat B's GameView NEVER exposes Seat A's unrevealed move, selection, or forecast
    assert_eq!(
        view_b.seats[0].own_selection, None,
        "Seat B view leaked Seat A own_selection"
    );
    assert_eq!(
        view_b.seats[0].own_move_forecast, None,
        "Seat B view leaked Seat A own_move_forecast"
    );
    assert!(
        view_b.seats[0].legal_targets.is_empty(),
        "Seat B view leaked Seat A legal_targets"
    );
    if !view_b.seats[0].is_locked && !view_b.seats[0].has_revealed {
        assert_eq!(
            view_b.seats[0].visible_move, None,
            "Seat B view leaked Seat A unrevealed move"
        );
    }

    // Verify rotated pre-join token & bogus token authenticate to None (Spectator)
    assert_eq!(snap.authenticate(Some(rotated_prejoin_token)), None);
    assert_eq!(snap.authenticate(Some("forged-token-99999")), None);

    // ------------------------------------------------------------------------
    // Invariant 2: Stale Revision Protection
    // ------------------------------------------------------------------------
    if snap.revision > 0 {
        let stale_rev = snap.revision - 1;
        let mut clone_s = snap.clone();
        let err = clone_s
            .submit_move_with_revision(Seat::A, (0, 0), (0, 1), Some(stale_rev))
            .unwrap_err();
        assert!(
            err.contains("Stale revision"),
            "expected stale revision rejection, got: {err}"
        );
        assert_eq!(clone_s.revision, snap.revision);
        assert_eq!(clone_s.board, snap.board);
    }

    // ------------------------------------------------------------------------
    // Invariant 3: Replay Completeness & Oracle Verification
    // ------------------------------------------------------------------------
    snap.verify_replay()
        .expect("session.verify_replay() must succeed at every step");

    // Step-by-step independent oracle::round_with_config reconstruction from start_board:
    let mut cur_board = snap.start_board.clone();
    let mut cur_marks: Vec<Coord> = Vec::new();
    let mut cur_locked: Vec<Move> = Vec::new();
    let mut cur_waiting: Vec<u32> = vec![0, 1];
    let mut cur_last_step: Option<Coord> = None;
    let mut resolved_turns = 0u64;
    let mut oracle_winner: Option<Seat> = None;

    for t in &snap.turns_played {
        for clash_subs in &t.conflict_subs {
            let (outcome, _) = oracle::round_with_config(
                &snap.config,
                cur_last_step,
                &cur_board,
                &snap.goals,
                &cur_marks,
                &cur_locked,
                &cur_waiting,
                clash_subs,
            )
            .expect("oracle::round_with_config must succeed on recorded clash sub-round");
            match outcome {
                RoundOutcome::Again {
                    marks,
                    locked,
                    waiting,
                } => {
                    cur_marks = marks;
                    cur_locked = locked;
                    cur_waiting = waiting;
                }
                RoundOutcome::Resolved { .. } => {
                    panic!("conflict_subs recorded a round that resolved cleanly");
                }
            }
        }

        let (outcome, _) = oracle::round_with_config(
            &snap.config,
            cur_last_step,
            &cur_board,
            &snap.goals,
            &cur_marks,
            &cur_locked,
            &cur_waiting,
            &t.clean_subs,
        )
        .expect("oracle::round_with_config must succeed on recorded clean turn");
        match outcome {
            RoundOutcome::Again { .. } => {
                panic!("clean_subs recorded a round that clashed");
            }
            RoundOutcome::Resolved { board, win } => {
                let from = t.auto_step.from;
                let to = t.auto_step.to;
                let dx = (to.0 - from.0).signum();
                let dy = (to.1 - from.1).signum();
                if dx != 0 || dy != 0 {
                    cur_last_step = Some((dx, dy));
                }
                cur_board = board;
                cur_marks.clear();
                cur_locked.clear();
                cur_waiting = vec![0, 1];
                resolved_turns += 1;
                if let Some(w) = win {
                    oracle_winner = Some(Seat::from_idx(w));
                }
            }
        }
    }
    assert_eq!(
        cur_board.to_wire(),
        snap.board.to_wire(),
        "replayed board must match session.board"
    );
    assert_eq!(
        resolved_turns, snap.turn_no,
        "replayed turn_no must match session.turn_no"
    );
    if !matches!(
        snap.finish_reason,
        Some(FinishReason::Resignation { .. })
            | Some(FinishReason::TurnCapAdjudicated { .. })
            | Some(FinishReason::DrawByTurnCap)
    ) {
        assert_eq!(oracle_winner, snap.winner);
    }

    // ------------------------------------------------------------------------
    // Invariant 4: Disk Persistence & RAM Eviction Equivalence
    // ------------------------------------------------------------------------
    // Create a dummy overflow game or call evict_excess_games to evict `game_id`, then lazy-load
    let before_a = serde_json::to_value(snap.view_for(Some(Seat::A))).unwrap();
    let before_b = serde_json::to_value(snap.view_for(Some(Seat::B))).unwrap();
    let before_spec = serde_json::to_value(snap.view_for(None)).unwrap();

    // Force `game_id` to have oldest `last_active_ms = 0` and insert a fresh dummy session
    // so `evict_excess_games(Some("evict-protector"))` evicts `game_id` from RAM.
    {
        let arc = state.get_or_load_session(game_id).unwrap();
        arc.lock().unwrap().last_active_ms = 0;
    }
    let dummy_id = "evict-helper";
    if !state.is_in_memory(dummy_id) {
        let dummy = GameSession::new(
            dummy_id.to_string(),
            "Evict Helper".to_string(),
            false,
            GameConfig::default(),
            42,
            u64::MAX,
        )
        .unwrap();
        state.insert_session(dummy).unwrap();
    } else {
        state.evict_excess_games(Some(dummy_id));
    }
    assert!(
        !state.is_in_memory(game_id),
        "game {game_id} must be evicted from RAM when dummy game is protected (max_in_memory=1)"
    );

    // Lazy-load via HTTP GET for Seat A, Seat B, and Spectator:
    let (st_a, after_a) = send_req(app, "GET", &format!("/api/games/{game_id}"), Some(token_a), None).await;
    let (st_b, after_b) = send_req(app, "GET", &format!("/api/games/{game_id}"), Some(token_b), None).await;
    let (st_s, after_spec) = send_req(app, "GET", &format!("/api/games/{game_id}"), None, None).await;
    assert_eq!(st_a, StatusCode::OK);
    assert_eq!(st_b, StatusCode::OK);
    assert_eq!(st_s, StatusCode::OK);

    assert_eq!(
        before_a, after_a,
        "Seat A GameView after disk eviction & lazy reload must be 100% identical"
    );
    assert_eq!(
        before_b, after_b,
        "Seat B GameView after disk eviction & lazy reload must be 100% identical"
    );
    assert_eq!(
        before_spec, after_spec,
        "Spectator GameView after disk eviction & lazy reload must be 100% identical"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 24,
        max_shrink_iters: 0,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_stateful_session_and_api_state_machine_fuzzing(
        config in arb_game_config(),
        actions in proptest::collection::vec(arb_fuzz_action(), 15..50),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async move {
            let dir = tempfile::tempdir().unwrap();
            let store = GameStore::disk(dir.path()).unwrap();
            // Cap max_in_memory_games at 1 so evict_excess_games always evicts non-protected games!
            let state = AppState::with_max_in_memory_games(store, 1).unwrap();
            let app = build_router(Arc::clone(&state));

            // 1. Create the game via POST /api/games (Seat A claims creator_seat)
            let (st_create, create_val) = send_req(
                &app,
                "POST",
                "/api/games",
                None,
                Some(serde_json::json!({
                    "config": config,
                    "creator_seat": "A",
                    "player_name": "Alice"
                })),
            )
            .await;
            assert_eq!(st_create, StatusCode::OK);
            let created: CreateGameResponse = serde_json::from_value(create_val).unwrap();
            let game_id = created.game_id;
            let token_a = created.creator_token.unwrap();
            // Record the pre-join open_invite_token for Seat B BEFORE Seat B joins (which rotates it)
            let pre_join_token = created.open_invite_token.unwrap();

            // 2. Seat B joins via POST /api/games/{id}/join, rotating Seat B's token
            let (st_join, join_val) = send_req(
                &app,
                "POST",
                &format!("/api/games/{game_id}/join"),
                None,
                Some(serde_json::json!({
                    "preferred_seat": "B",
                    "player_name": "Bob"
                })),
            )
            .await;
            assert_eq!(st_join, StatusCode::OK);
            let joined: JoinGameResponse = serde_json::from_value(join_val).unwrap();
            let token_b = joined.seat_token;
            assert_ne!(token_b, pre_join_token, "joining Seat B must rotate pre-join token");

            // Track blind-hash commitments per seat so RevealMove can test both valid and invalid reveals
            let mut last_committed: [Option<(Coord, Coord, String)>; 2] = [None, None];

            // Verify invariants on initial state before stepping
            assert_universal_service_invariants(
                &state,
                &app,
                &game_id,
                &token_a,
                &token_b,
                &pre_join_token,
            )
            .await;

            for action in actions {
                let resolve_token = |tk: TokenKind| -> Option<String> {
                    match tk {
                        TokenKind::ValidSeatA => Some(token_a.clone()),
                        TokenKind::ValidSeatB => Some(token_b.clone()),
                        TokenKind::SpectatorNone => None,
                        TokenKind::InvalidToken => Some("invalid-adversarial-token".to_string()),
                        TokenKind::PreJoinCreatorToken => Some(pre_join_token.clone()),
                    }
                };
                let is_authorized_for = |seat: Seat, tk: TokenKind| -> bool {
                    matches!(
                        (seat, tk),
                        (Seat::A, TokenKind::ValidSeatA) | (Seat::B, TokenKind::ValidSeatB)
                    )
                };

                let pre_snap = state
                    .get_or_load_session(&game_id)
                    .unwrap()
                    .lock()
                    .unwrap()
                    .clone();

                let compute_rev = |kind: &ExpectedRevKind, cur: u64| -> (Option<u64>, bool) {
                    match kind {
                        ExpectedRevKind::None => (None, false),
                        ExpectedRevKind::Current => (Some(cur), false),
                        ExpectedRevKind::StaleOffset(off) => {
                            if cur >= *off {
                                (Some(cur - *off), true)
                            } else {
                                (Some(cur + *off), true)
                            }
                        }
                        ExpectedRevKind::FutureOffset(off) => (Some(cur + *off), true),
                    }
                };

                match action {
                    FuzzAction::SelectPiece {
                        seat,
                        coord,
                        token_kind,
                    } => {
                        let tok = resolve_token(token_kind);
                        let target_coord = coord.map(|c| {
                            let (src, _) = pick_legal_move_or_raw(&pre_snap, c, (0, 0));
                            src
                        });
                        let (st, _) = send_req(
                            &app,
                            "POST",
                            &format!("/api/games/{game_id}/select"),
                            tok.as_deref(),
                            Some(serde_json::json!({
                                "coord": target_coord,
                                "seat": seat,
                            })),
                        )
                        .await;
                        if !is_authorized_for(seat, token_kind) {
                            assert_eq!(
                                st,
                                StatusCode::UNAUTHORIZED,
                                "unauthorized SelectPiece ({token_kind:?} for {seat:?}) must return 401"
                            );
                            let post_snap = state
                                .get_or_load_session(&game_id)
                                .unwrap()
                                .lock()
                                .unwrap()
                                .clone();
                            assert_eq!(post_snap.revision, pre_snap.revision);
                            assert_eq!(post_snap.sel, pre_snap.sel);
                        }
                    }
                    FuzzAction::SubmitMove {
                        seat,
                        from,
                        to,
                        expected_rev,
                        token_kind,
                    } => {
                        let tok = resolve_token(token_kind);
                        let (src, dst) = pick_legal_move_or_raw(&pre_snap, from, to);
                        let (exp_rev, is_stale) = compute_rev(&expected_rev, pre_snap.revision);
                        let (st, _) = send_req(
                            &app,
                            "POST",
                            &format!("/api/games/{game_id}/move"),
                            tok.as_deref(),
                            Some(serde_json::json!({
                                "from": src,
                                "to": dst,
                                "expected_revision": exp_rev,
                                "seat": seat,
                            })),
                        )
                        .await;
                        if !is_authorized_for(seat, token_kind) {
                            assert_eq!(
                                st,
                                StatusCode::UNAUTHORIZED,
                                "unauthorized SubmitMove ({token_kind:?} for {seat:?}) must return 401"
                            );
                            let post_snap = state
                                .get_or_load_session(&game_id)
                                .unwrap()
                                .lock()
                                .unwrap()
                                .clone();
                            assert_eq!(post_snap.revision, pre_snap.revision);
                            assert_eq!(post_snap.seal, pre_snap.seal);
                        } else if is_stale {
                            assert_eq!(
                                st,
                                StatusCode::BAD_REQUEST,
                                "stale expected_revision on SubmitMove must be rejected with 400"
                            );
                            let post_snap = state
                                .get_or_load_session(&game_id)
                                .unwrap()
                                .lock()
                                .unwrap()
                                .clone();
                            assert_eq!(post_snap.revision, pre_snap.revision);
                            assert_eq!(post_snap.seal, pre_snap.seal);
                        }
                    }
                    FuzzAction::CommitHash {
                        seat,
                        move_or_garbage,
                        salt,
                        expected_rev,
                        token_kind,
                    } => {
                        let tok = resolve_token(token_kind);
                        let (exp_rev, is_stale) = compute_rev(&expected_rev, pre_snap.revision);
                        let (hash, committed_tuple) = match move_or_garbage {
                            MoveOrGarbage::IndexedMove {
                                piece_idx,
                                target_idx,
                            } => {
                                let (src, dst) = pick_legal_move_or_raw(
                                    &pre_snap,
                                    (piece_idx as i32, 0),
                                    (target_idx as i32, 0),
                                );
                                let h = GameSession::compute_commitment(
                                    &pre_snap.id,
                                    pre_snap.turn_no,
                                    pre_snap.conflict_subs.len(),
                                    seat,
                                    src,
                                    dst,
                                    &salt,
                                );
                                (h, Some((src, dst, salt)))
                            }
                            MoveOrGarbage::RawMove { from, to } => {
                                let h = GameSession::compute_commitment(
                                    &pre_snap.id,
                                    pre_snap.turn_no,
                                    pre_snap.conflict_subs.len(),
                                    seat,
                                    from,
                                    to,
                                    &salt,
                                );
                                (h, Some((from, to, salt)))
                            }
                            MoveOrGarbage::GarbageHash(g) => (g, None),
                        };

                        let (st, _) = send_req(
                            &app,
                            "POST",
                            &format!("/api/games/{game_id}/commit"),
                            tok.as_deref(),
                            Some(serde_json::json!({
                                "commitment_hash": hash,
                                "expected_revision": exp_rev,
                                "seat": seat,
                            })),
                        )
                        .await;
                        if !is_authorized_for(seat, token_kind) {
                            assert_eq!(st, StatusCode::UNAUTHORIZED);
                            let post_snap = state
                                .get_or_load_session(&game_id)
                                .unwrap()
                                .lock()
                                .unwrap()
                                .clone();
                            assert_eq!(post_snap.revision, pre_snap.revision);
                        } else if is_stale {
                            assert_eq!(st, StatusCode::BAD_REQUEST);
                            let post_snap = state
                                .get_or_load_session(&game_id)
                                .unwrap()
                                .lock()
                                .unwrap()
                                .clone();
                            assert_eq!(post_snap.revision, pre_snap.revision);
                        } else if st == StatusCode::OK {
                            last_committed[seat.idx()] = committed_tuple;
                        }
                    }
                    FuzzAction::RevealMove {
                        seat,
                        from,
                        to,
                        salt,
                        use_matching_committed,
                        token_kind,
                    } => {
                        let tok = resolve_token(token_kind);
                        let body = if use_matching_committed {
                            if let Some((c_from, c_to, ref c_salt)) = last_committed[seat.idx()] {
                                serde_json::json!({
                                    "from": c_from,
                                    "to": c_to,
                                    "nonce": c_salt,
                                    "seat": seat,
                                })
                            } else {
                                serde_json::json!({ "seat": seat })
                            }
                        } else {
                            serde_json::json!({
                                "from": from,
                                "to": to,
                                "nonce": salt,
                                "seat": seat,
                            })
                        };
                        let (st, _) = send_req(
                            &app,
                            "POST",
                            &format!("/api/games/{game_id}/reveal"),
                            tok.as_deref(),
                            Some(body),
                        )
                        .await;
                        if !is_authorized_for(seat, token_kind) {
                            assert_eq!(st, StatusCode::UNAUTHORIZED);
                            let post_snap = state
                                .get_or_load_session(&game_id)
                                .unwrap()
                                .lock()
                                .unwrap()
                                .clone();
                            assert_eq!(post_snap.revision, pre_snap.revision);
                        }
                    }
                    FuzzAction::TriggerResolve { token_kind } => {
                        let tok = resolve_token(token_kind);
                        let (st, _) = send_req(
                            &app,
                            "POST",
                            &format!("/api/games/{game_id}/resolve"),
                            tok.as_deref(),
                            Some(serde_json::json!({})),
                        )
                        .await;
                        if !matches!(token_kind, TokenKind::ValidSeatA | TokenKind::ValidSeatB) {
                            assert_eq!(st, StatusCode::UNAUTHORIZED);
                            let post_snap = state
                                .get_or_load_session(&game_id)
                                .unwrap()
                                .lock()
                                .unwrap()
                                .clone();
                            assert_eq!(post_snap.revision, pre_snap.revision);
                        }
                    }
                    FuzzAction::Resign { seat, token_kind } => {
                        let tok = resolve_token(token_kind);
                        // Only execute valid resignation when game is past turn 2 or unauthorized
                        // so random sequences don't always end on step 1!
                        if !is_authorized_for(seat, token_kind) || pre_snap.turn_no >= 2 {
                            let (st, _) = send_req(
                                &app,
                                "POST",
                                &format!("/api/games/{game_id}/resign"),
                                tok.as_deref(),
                                Some(serde_json::json!({ "seat": seat })),
                            )
                            .await;
                            if !is_authorized_for(seat, token_kind) {
                                assert_eq!(st, StatusCode::UNAUTHORIZED);
                                let post_snap = state
                                    .get_or_load_session(&game_id)
                                    .unwrap()
                                    .lock()
                                    .unwrap()
                                    .clone();
                                assert_eq!(post_snap.revision, pre_snap.revision);
                                assert_eq!(post_snap.ended, pre_snap.ended);
                            }
                        }
                    }
                    FuzzAction::PollView { viewer_kind } => {
                        let tok = match viewer_kind {
                            ViewerKind::SeatA => Some(token_a.as_str()),
                            ViewerKind::SeatB => Some(token_b.as_str()),
                            ViewerKind::Spectator => None,
                            ViewerKind::InvalidToken => Some("bogus-viewer-token"),
                            ViewerKind::PreJoinToken => Some(pre_join_token.as_str()),
                        };
                        let (st, val) =
                            send_req(&app, "GET", &format!("/api/games/{game_id}"), tok, None)
                                .await;
                        assert_eq!(st, StatusCode::OK);
                        let view: GameView = serde_json::from_value(val).unwrap();
                        let expected_viewer = match viewer_kind {
                            ViewerKind::SeatA => Some(Seat::A),
                            ViewerKind::SeatB => Some(Seat::B),
                            ViewerKind::Spectator
                            | ViewerKind::InvalidToken
                            | ViewerKind::PreJoinToken => None,
                        };
                        assert_eq!(view.viewer_seat, expected_viewer);
                    }
                    FuzzAction::EvictAndLazyReloadFromDisk => {
                        // Tested both here and in assert_universal_service_invariants
                        state.evict_excess_games(None);
                    }
                    FuzzAction::VerifyReplay => {
                        let (st, val) = send_req(
                            &app,
                            "GET",
                            &format!("/api/games/{game_id}/replay"),
                            None,
                            None,
                        )
                        .await;
                        assert_eq!(st, StatusCode::OK);
                        let rep: ReplayExport = serde_json::from_value(val).unwrap();
                        assert!(rep.replay_verified);
                    }
                }

                // After EVERY SINGLE STEP in the generated sequence, assert all 4 universal invariants!
                assert_universal_service_invariants(
                    &state,
                    &app,
                    &game_id,
                    &token_a,
                    &token_b,
                    &pre_join_token,
                )
                .await;
            }
        });
    }
}

// ============================================================================
// Requirement 3: Randomized Property Differential Testing:
// `automatafl-server::oracle` vs Embedded Browser JS Engine (`web_ui.rs`)
// ============================================================================

#[derive(Serialize, Deserialize)]
struct JsDiffCase {
    case_id: usize,
    board: Board,
    config: GameConfig,
    marks: Vec<Coord>,
    cooldown_squares: Vec<Coord>,
    locked_moves: Vec<Move>,
    submitted_moves: Vec<Move>,
    goals: Vec<(Coord, u32)>,
    last_step: Option<Coord>,
    sample_sources: Vec<Coord>,
    forecast_move: Option<Move>,
    forecast_seat: u32,
    // Expected outputs from `automatafl_server::oracle`:
    expected_sightlines: Vec<Coord>,
    expected_targets: Vec<(Coord, Vec<Coord>, Vec<Coord>)>,
    expected_mid_cells: Vec<u8>,
    expected_stepped_cells: Vec<u8>,
    expected_stepped_auto: Coord,
    expected_stepped_winner: Option<String>,
    expected_stepped_next_dir: Option<Coord>,
    expected_landed: Vec<Coord>,
    expected_forecast: Option<String>,
    expected_round_tag: String,
    expected_round_board_cells: Vec<u8>,
    expected_round_auto: Coord,
    expected_round_marks: Vec<Coord>,
}

fn build_random_board(seed: u64, preset: BoardPreset, col_rule: bool, kind: usize) -> Board {
    let n = preset.size();
    let mut b = match kind % 3 {
        0 => {
            let mut cfg = GameConfig::default();
            cfg.preset = preset;
            cfg.tie_break = if col_rule {
                TieBreakMode::Column
            } else {
                TieBreakMode::Row
            };
            oracle::stock_board_for_config(&cfg).unwrap()
        }
        _ => {
            let mut cells = vec![VAC; n * n];
            let mut s = seed ^ 0x9E37_79B9_7F4A_7C15;
            let mut next_u64 = || {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                s
            };
            let ax = (1 + (next_u64() as usize % (n - 2))) as i32;
            let ay = (1 + (next_u64() as usize % (n - 2))) as i32;
            let auto = (ax, ay);
            cells[(ay as usize) * n + (ax as usize)] = AUTO;
            for y in 0..n {
                for x in 0..n {
                    if (x as i32, y as i32) == auto {
                        continue;
                    }
                    let r = next_u64() % 100;
                    if r < 18 {
                        cells[y * n + x] = REP;
                    } else if r < 34 {
                        cells[y * n + x] = ATT;
                    }
                }
            }
            Board {
                n,
                cells,
                auto,
                col_rule,
            }
        }
    };
    b.col_rule = col_rule;
    b
}

#[test]
fn differential_property_oracle_vs_embedded_web_ui_js_engine_across_64_scenarios() {
    let presets = [
        BoardPreset::Mini5x5,
        BoardPreset::Blitz7x7,
        BoardPreset::Tactical9x9,
        BoardPreset::Classic11x11,
    ];
    let tie_breaks = [
        TieBreakMode::Column,
        TieBreakMode::Row,
        TieBreakMode::Freeze,
        TieBreakMode::Momentum {
            prefer_same_axis: true,
        },
        TieBreakMode::Momentum {
            prefer_same_axis: false,
        },
    ];
    let confluences = [
        ConfluenceMode::Conflict,
        ConfluenceMode::BunchBefore,
        ConfluenceMode::Annihilate,
    ];

    let mut cases: Vec<JsDiffCase> = Vec::with_capacity(64);

    for idx in 0..64usize {
        let seed = 0xC0FF_EE00_0000_0000u64 + (idx as u64).wrapping_mul(0x517C_C1B7_2722_0A95);
        let preset = presets[idx % presets.len()];
        let tie_break = tie_breaks[idx % tie_breaks.len()];
        let confluence = confluences[idx % confluences.len()];
        let two_cycle_swaps = (idx / 2) % 2 == 1;
        let cooldown_turns = (idx % 4) as u32;
        let railgun_fling = (idx / 3) % 2 == 1;
        let automata_count = ((idx % 3) + 1) as u32;
        let col_rule = matches!(tie_break, TieBreakMode::Column);

        let config = GameConfig {
            preset,
            tie_break,
            confluence,
            two_cycle_swaps,
            cooldown_turns,
            max_turns: 60,
            railgun_fling,
            automata_count,
            commit_mode: CommitMode::AutoRevealOnBothSealed,
        };

        let mut board = build_random_board(seed, preset, col_rule, idx);
        let n = board.n as i32;
        let goals = oracle::stock_goals2(board.n).unwrap();

        // Specialize certain indices to guarantee coverage of multi-piece chains, 2-cycles,
        // 3-cycles, caterpillar bunch_before collisions, and conflict re-entry:
        let (marks, cooldown_squares, locked_moves, submitted_moves, last_step) = match idx % 8 {
            // Case pattern 0: 2-cycle swap between two adjacent pieces
            0 => {
                board.cells.fill(VAC);
                board.auto = (n - 1, n - 1);
                board.cells[(n as usize - 1) * board.n + (n as usize - 1)] = AUTO;
                board.cells[0] = REP; // (0,0)
                board.cells[1] = ATT; // (1,0)
                board.cells[board.n] = REP; // (0,1)
                let subs = vec![
                    Move {
                        who: 0,
                        frm: (0, 0),
                        to: (1, 0),
                    },
                    Move {
                        who: 1,
                        frm: (1, 0),
                        to: (0, 0),
                    },
                ];
                (vec![], vec![], vec![], subs, Some((1, 0)))
            }
            // Case pattern 1: 3-cycle rotation via locked move + 2 submitted moves
            1 => {
                board.cells.fill(VAC);
                board.auto = (n - 1, n - 1);
                board.cells[(n as usize - 1) * board.n + (n as usize - 1)] = AUTO;
                board.cells[0] = REP; // (0,0)
                board.cells[1] = ATT; // (1,0)
                board.cells[board.n + 1] = REP; // (1,1)
                board.cells[board.n] = ATT; // (0,1)
                let locked = vec![
                    Move {
                        who: 2,
                        frm: (0, 0),
                        to: (1, 0),
                    },
                    Move {
                        who: 3,
                        frm: (1, 0),
                        to: (1, 1),
                    },
                ];
                let subs = vec![
                    Move {
                        who: 0,
                        frm: (1, 1),
                        to: (0, 1),
                    },
                    Move {
                        who: 1,
                        frm: (0, 1),
                        to: (0, 0),
                    },
                ];
                (vec![], vec![], locked, subs, Some((0, -1)))
            }
            // Case pattern 2: Caterpillar 3-move collision (`bunch_before` / `conflict` / `annihilate`)
            2 => {
                board.cells.fill(VAC);
                board.auto = (n - 1, n - 1);
                board.cells[(n as usize - 1) * board.n + (n as usize - 1)] = AUTO;
                board.cells[0] = REP; // (0,0)
                board.cells[2] = ATT; // (2,0)
                board.cells[board.n] = ATT; // (0,1)
                let locked = vec![Move {
                    who: 2,
                    frm: (0, 1),
                    to: (0, 0),
                }];
                let subs = vec![
                    Move {
                        who: 0,
                        frm: (0, 0),
                        to: (1, 0),
                    },
                    Move {
                        who: 1,
                        frm: (2, 0),
                        to: (1, 0),
                    },
                ];
                (vec![(3, 0)], vec![(2, 0)], locked, subs, Some((-1, 0)))
            }
            // Case pattern 3: Head-on collision into same vacant square (conflict re-entry)
            3 => {
                board.cells.fill(VAC);
                board.auto = (n / 2, n / 2);
                board.cells[(n as usize / 2) * board.n + (n as usize / 2)] = AUTO;
                board.cells[0] = REP; // (0,0)
                board.cells[2 * board.n + 2] = ATT; // (2,2)
                let subs = vec![
                    Move {
                        who: 0,
                        frm: (0, 0),
                        to: (2, 0),
                    },
                    Move {
                        who: 1,
                        frm: (2, 2),
                        to: (2, 0),
                    },
                ];
                (vec![(0, 2)], vec![], vec![], subs, Some((0, 1)))
            }
            // Case pattern 4..7: Multi-piece random legal/colliding moves on random/preset boards
            _ => {
                let mut pieces = Vec::new();
                for y in 0..n {
                    for x in 0..n {
                        let c = (x, y);
                        let p = board.cell_at(c);
                        if p == REP || p == ATT {
                            pieces.push(c);
                        }
                    }
                }
                let marks = if pieces.len() > 3 && idx % 2 == 0 {
                    vec![pieces[0]]
                } else {
                    vec![]
                };
                let cd_sq = if pieces.len() > 4 && cooldown_turns > 0 {
                    vec![pieces[1]]
                } else {
                    vec![]
                };
                let mut subs = Vec::new();
                if pieces.len() >= 3 {
                    let p0 = pieces[(idx + 1) % pieces.len()];
                    let t0 = oracle::legal_targets(&board, &marks, 0, p0).unwrap_or_default();
                    if !t0.is_empty() {
                        subs.push(Move {
                            who: 0,
                            frm: p0,
                            to: t0[idx % t0.len()],
                        });
                    }
                    let p1 = pieces[(idx + 2) % pieces.len()];
                    let t1 = oracle::legal_targets(&board, &marks, 1, p1).unwrap_or_default();
                    if !t1.is_empty() {
                        subs.push(Move {
                            who: 1,
                            frm: p1,
                            to: t1[(idx * 3) % t1.len()],
                        });
                    }
                }
                let lstep = match idx % 5 {
                    0 => None,
                    1 => Some((1, 0)),
                    2 => Some((-1, 0)),
                    3 => Some((0, 1)),
                    _ => Some((0, -1)),
                };
                (marks, cd_sq, vec![], subs, lstep)
            }
        };

        // Compute expected oracle outputs in Rust:
        let expected_sightlines = oracle::sightlines(&board);

        let mut sample_sources = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let c = (x, y);
                let p = board.cell_at(c);
                if (p == REP || p == ATT) && sample_sources.len() < 6 {
                    sample_sources.push(c);
                }
            }
        }
        // Also include auto and a marked square in sample_sources to verify empty target parity
        sample_sources.push(board.auto);
        if let Some(&m) = marks.first() {
            sample_sources.push(m);
        }

        let mut expected_targets = Vec::new();
        for &src in &sample_sources {
            let p = board.cell_at(src);
            let is_valid_piece = (p == REP || p == ATT)
                && src != board.auto
                && !marks.contains(&src)
                && !cooldown_squares.contains(&src);
            if is_valid_piece {
                let leg = oracle::legal_targets(&board, &marks, 0, src).unwrap();
                let exe =
                    oracle::executable_targets(&board, &marks, &locked_moves, 0, src).unwrap();
                expected_targets.push((src, leg, exe));
            } else {
                expected_targets.push((src, vec![], vec![]));
            }
        }

        // Legal filter matching resolveMidClient / landedCoordsCfgClient
        let mut all_moves = locked_moves.clone();
        all_moves.extend(submitted_moves.iter().copied());
        let legal_all: Vec<Move> = all_moves
            .iter()
            .copied()
            .filter(|m| oracle::move_legal(&board, &marks, m).unwrap_or(false))
            .collect();

        let mid = oracle::resolve_moves_cfg(&config, &board, &legal_all);
        let (stepped_board, stepped_win, stepped_next_dir) =
            oracle::step_automata_cfg(&config, &goals, &mid, last_step);
        let expected_landed = oracle::landed_coords_cfg(&config, &board, &legal_all);

        let forecast_move = submitted_moves.first().copied();
        let forecast_seat = (idx % 2) as u32;
        let expected_forecast = forecast_move.and_then(|mv| {
            oracle::own_move_forecast_with_config(
                &config,
                last_step,
                &board,
                &marks,
                &locked_moves,
                &goals,
                forecast_seat,
                &mv,
            )
        });

        let waiting = vec![0, 1];
        let (round_outcome, _) = oracle::round_with_config(
            &config,
            last_step,
            &board,
            &goals,
            &marks,
            &locked_moves,
            &waiting,
            &submitted_moves,
        )
        .unwrap();

        let (expected_round_tag, expected_round_board_cells, expected_round_auto, expected_round_marks) =
            match round_outcome {
                RoundOutcome::Again {
                    marks: m_out,
                    ..
                } => ("A".to_string(), board.cells.clone(), board.auto, m_out),
                RoundOutcome::Resolved { board: b_out, .. } => {
                    ("R".to_string(), b_out.cells.clone(), b_out.auto, vec![])
                }
            };

        cases.push(JsDiffCase {
            case_id: idx,
            board,
            config,
            marks,
            cooldown_squares,
            locked_moves,
            submitted_moves,
            goals,
            last_step,
            sample_sources,
            forecast_move,
            forecast_seat,
            expected_sightlines,
            expected_targets,
            expected_mid_cells: mid.cells,
            expected_stepped_cells: stepped_board.cells,
            expected_stepped_auto: stepped_board.auto,
            expected_stepped_winner: stepped_win
                .map(|w| if w == 0 { "A".to_string() } else { "B".to_string() }),
            expected_stepped_next_dir: stepped_next_dir,
            expected_landed,
            expected_forecast,
            expected_round_tag,
            expected_round_board_cells,
            expected_round_auto,
            expected_round_marks,
        });
    }

    assert!(cases.len() >= 50);

    // Extract `<script>...</script>` from `web_ui::INDEX_HTML` and run all 64 cases in one batch!
    let script_start = INDEX_HTML.find("<script>").expect("<script>") + "<script>".len();
    let script_end = INDEX_HTML.rfind("</script>").expect("</script>");
    let script_body = &INDEX_HTML[script_start..script_end];
    let cases_json = serde_json::to_string(&cases).unwrap();

    let harness = format!(
        r#"
function assertEq(a, b, msg) {{
  if (JSON.stringify(a) !== JSON.stringify(b)) {{
    throw new Error(msg + '\n  JS got:      ' + JSON.stringify(a) + '\n  Rust oracle: ' + JSON.stringify(b));
  }}
}}
const elements = {{}};
function getEl(id) {{
  if (!elements[id]) {{
    elements[id] = {{ id, innerHTML: '', textContent: '', value: '', checked: false, style: {{}} }};
  }}
  return elements[id];
}}
globalThis.window = {{
  location: {{ pathname: '/', search: '', origin: 'http://localhost:3000', href: '' }}
}};
globalThis.document = {{ getElementById: getEl }};
globalThis.localStorage = {{
  store: {{}},
  getItem(k) {{ return this.store[k] || null; }},
  setItem(k, v) {{ this.store[k] = String(v); }}
}};
globalThis.fetch = async () => ({{ ok: true, json: async () => [] }});
globalThis.EventSource = function() {{ this.close = () => {{}}; }};
globalThis.setInterval = () => 0;
globalThis.clearInterval = () => {{}};

{script_body}

const cases = {cases_json};
for (const c of cases) {{
  const ctx = `Case #${{c.case_id}} (preset=${{c.config.preset}}, conf=${{c.config.confluence}}, tb=${{JSON.stringify(c.config.tie_break)}}, swaps=${{c.config.two_cycle_swaps}}, fling=${{c.config.railgun_fling}}, auto_k=${{c.config.automata_count}})`;

  // 1. Sightlines parity
  const jsSight = computeSightlinesClient(c.board);
  assertEq(jsSight, c.expected_sightlines, `${{ctx}}: computeSightlinesClient mismatch`);

  // 2. Legal & Executable targets parity
  for (const [src, expLegal, expExec] of c.expected_targets) {{
    const jsT = computeTargetsClient(c.board, c.marks, c.cooldown_squares, c.locked_moves, src);
    assertEq(jsT.legal, expLegal, `${{ctx}}: computeTargetsClient.legal at (${{src}})`);
    assertEq(jsT.executable, expExec, `${{ctx}}: computeTargetsClient.executable at (${{src}})`);
  }}

  // 3. Mid-board move resolution parity (resolveMidClient vs oracle::resolve_moves_cfg)
  const allMoves = [...c.locked_moves, ...c.submitted_moves];
  const jsMid = resolveMidClient(c.board, c.marks, allMoves, c.config);
  assertEq(jsMid.cells, c.expected_mid_cells, `${{ctx}}: resolveMidClient.cells mismatch`);

  // 4. Automaton multi-step & variant sensing parity (stepAutomataClient vs oracle::step_automata_cfg)
  const jsStepped = stepAutomataClient(jsMid, c.config, c.goals, c.last_step);
  assertEq(jsStepped.board.cells, c.expected_stepped_cells, `${{ctx}}: stepAutomataClient.board.cells mismatch`);
  assertEq(jsStepped.board.auto, c.expected_stepped_auto, `${{ctx}}: stepAutomataClient.board.auto mismatch`);
  assertEq(jsStepped.winner, c.expected_stepped_winner, `${{ctx}}: stepAutomataClient.winner mismatch`);
  assertEq(jsStepped.nextDir, c.expected_stepped_next_dir, `${{ctx}}: stepAutomataClient.nextDir mismatch`);

  // 5. Landed coordinates parity (landedCoordsCfgClient vs oracle::landed_coords_cfg)
  const jsLanded = landedCoordsCfgClient(c.config, c.board, c.marks, allMoves);
  assertEq(jsLanded, c.expected_landed, `${{ctx}}: landedCoordsCfgClient mismatch`);

  // 6. Counterfactual single-move forecast parity (ownMoveForecastClient vs oracle::own_move_forecast_with_config)
  if (c.forecast_move) {{
    const jsForecast = ownMoveForecastClient(
      c.board,
      c.marks,
      c.locked_moves,
      c.forecast_move,
      c.config,
      c.goals,
      c.forecast_seat,
      c.last_step
    );
    assertEq(jsForecast, c.expected_forecast, `${{ctx}}: ownMoveForecastClient mismatch`);
  }}

  // 7. Full round clash/unresolved vs resolved parity (round_with_config)
  const legalSub = c.submitted_moves.filter(m =>
    (m.who === 0 || m.who === 1) && moveLegalClient(c.board, c.marks, null, m.frm, m.to)
  );
  const roundMoves = [...c.locked_moves, ...legalSub];
  const clash = clashCoordsCfgClient(c.config, c.board, roundMoves);
  const cs = clash.length > 0 ? clash : unresolvedCfgClient(c.config, c.board, roundMoves);
  if (cs.length > 0) {{
    assertEq('A', c.expected_round_tag, `${{ctx}}: expected RoundOutcome::Again`);
    const newMarks = dedupCoords([...c.marks, ...cs]);
    assertEq(newMarks, c.expected_round_marks, `${{ctx}}: RoundOutcome::Again marks mismatch`);
  }} else {{
    assertEq('R', c.expected_round_tag, `${{ctx}}: expected RoundOutcome::Resolved`);
    const rMid = resolveMidClient(c.board, c.marks, roundMoves, c.config);
    const rStep = stepAutomataClient(rMid, c.config, c.goals, c.last_step);
    assertEq(rStep.board.cells, c.expected_round_board_cells, `${{ctx}}: RoundOutcome::Resolved cells mismatch`);
    assertEq(rStep.board.auto, c.expected_round_auto, `${{ctx}}: RoundOutcome::Resolved auto mismatch`);
  }}
}}
if (typeof process !== 'undefined' && process.exit) process.exit(0);
"#,
        script_body = script_body,
        cases_json = cases_json
    );

    let dir = tempfile::tempdir().unwrap();
    let script_path = dir.path().join("diff_batch.js");
    std::fs::write(&script_path, harness.as_bytes()).unwrap();

    let js_runtime = ["bun", "deno", "node"]
        .into_iter()
        .find(|bin| {
            std::process::Command::new(bin)
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
        .expect("at least one JS runtime (bun, deno, or node) must be available");

    let mut cmd = std::process::Command::new(js_runtime);
    if js_runtime == "deno" {
        cmd.arg("run");
    }
    let output = cmd
        .arg(&script_path)
        .output()
        .expect("failed to execute JS runtime");
    assert!(
        output.status.success(),
        "{js_runtime} differential test of 64 cases failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ============================================================================
// Requirement 4: Composed End-to-End Service Fitness & High-Concurrency Load Gate
// ============================================================================

fn pick_non_colliding_pair(view: &GameView) -> ((Coord, Coord), (Coord, Coord)) {
    let n = view.board.n as i32;
    let mut legal_moves = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let c = (x, y);
            let p = view.board.cell_at(c);
            if (p == REP || p == ATT)
                && c != view.board.auto
                && !view.marks.contains(&c)
                && !view.cooldown_squares.contains(&c)
            {
                let targets = oracle::executable_targets(
                    &view.board,
                    &view.marks,
                    &view.locked_moves,
                    0,
                    c,
                )
                .unwrap_or_default();
                for dst in targets {
                    legal_moves.push((c, dst));
                }
            }
        }
    }
    for (i, &m_a) in legal_moves.iter().enumerate() {
        for &m_b in &legal_moves[i + 1..] {
            if m_a.0 != m_b.0 && m_a.1 != m_b.1 && m_a.1 != m_b.0 && m_b.1 != m_a.0 {
                return (m_a, m_b);
            }
        }
    }
    // Fallback: include proposable legal targets (including vacuum pass moves when all pieces are marked/cooldown)
    for y in 0..n {
        for x in 0..n {
            let c = (x, y);
            if c != view.board.auto
                && view.board.cell_at(c) != AUTO
                && !view.marks.contains(&c)
                && !view.cooldown_squares.contains(&c)
            {
                let targets = oracle::legal_targets(&view.board, &view.marks, 0, c)
                    .unwrap_or_default();
                for dst in targets {
                    if !legal_moves.contains(&(c, dst)) {
                        legal_moves.push((c, dst));
                    }
                }
            }
        }
    }
    for (i, &m_a) in legal_moves.iter().enumerate() {
        for &m_b in &legal_moves[i + 1..] {
            if m_a.0 != m_b.0 && m_a.1 != m_b.1 && m_a.1 != m_b.0 && m_b.1 != m_a.0 {
                return (m_a, m_b);
            }
        }
    }
    let first = legal_moves.first().copied().unwrap_or(((0, 1), (0, 2)));
    (first, first)
}

fn pick_colliding_pair(view: &GameView) -> Option<((Coord, Coord), (Coord, Coord))> {
    let n = view.board.n as i32;
    let mut by_dst: std::collections::HashMap<Coord, Vec<Coord>> = std::collections::HashMap::new();
    for y in 0..n {
        for x in 0..n {
            let c = (x, y);
            let p = view.board.cell_at(c);
            if (p == REP || p == ATT)
                && c != view.board.auto
                && !view.marks.contains(&c)
                && !view.cooldown_squares.contains(&c)
            {
                let targets = oracle::executable_targets(
                    &view.board,
                    &view.marks,
                    &view.locked_moves,
                    0,
                    c,
                )
                .unwrap_or_default();
                for dst in targets {
                    by_dst.entry(dst).or_default().push(c);
                }
            }
        }
    }
    for (dst, srcs) in by_dst {
        if srcs.len() >= 2 {
            return Some(((srcs[0], dst), (srcs[1], dst)));
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn composed_end_to_end_service_fitness_and_24_game_high_concurrency_load_gate() {
    let dir = tempfile::tempdir().unwrap();
    let store = GameStore::disk(dir.path()).unwrap();
    // Tight RAM bound: max 8 games in memory while running 24 concurrent games (3x over-subscription)
    let state = AppState::with_max_in_memory_games(store, 8).unwrap();
    let app = build_router(Arc::clone(&state));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = RemoteClient::new(&base_url).unwrap();
    assert!(client.health_check().await.unwrap());

    // ------------------------------------------------------------------------
    // Pre-pair 8 Human-vs-Human games via POST /api/matchmaking/seek
    // ------------------------------------------------------------------------
    let mut human_matches = Vec::with_capacity(8);
    let presets = [
        BoardPreset::Mini5x5,
        BoardPreset::Blitz7x7,
        BoardPreset::Tactical9x9,
        BoardPreset::Classic11x11,
    ];
    for i in 0..8usize {
        let cfg = GameConfig {
            preset: presets[i % presets.len()],
            tie_break: if i % 2 == 0 {
                TieBreakMode::Column
            } else {
                TieBreakMode::Row
            },
            confluence: ConfluenceMode::Conflict,
            two_cycle_swaps: i % 3 == 0,
            cooldown_turns: if i % 2 == 0 { 1 } else { 0 },
            max_turns: 40,
            railgun_fling: i % 4 == 0,
            automata_count: 1,
            commit_mode: CommitMode::AutoRevealOnBothSealed,
        };
        let seek_a = client
            .seek_matchmaking(&SeekRequest {
                player_name: Some(format!("HumanA-{i}")),
                config: cfg.clone(),
                any_variant_with_same_board: false,
            })
            .await
            .unwrap();
        let ticket_a = match seek_a {
            SeekStatus::Queued { ticket_id, .. } => ticket_id,
            other => panic!("expected Queued, got {other:?}"),
        };
        let seek_b = client
            .seek_matchmaking(&SeekRequest {
                player_name: Some(format!("HumanB-{i}")),
                config: cfg.clone(),
                any_variant_with_same_board: false,
            })
            .await
            .unwrap();
        let tok_b = match seek_b {
            SeekStatus::Matched { seat_token, .. } => seat_token,
            other => panic!("expected Matched for B, got {other:?}"),
        };
        let status_a = client.matchmaking_status(&ticket_a).await.unwrap();
        let (game_id, tok_a) = match status_a {
            SeekStatus::Matched {
                game_id,
                seat_token,
                ..
            } => (game_id, seat_token),
            other => panic!("expected Matched for A, got {other:?}"),
        };
        human_matches.push((i, game_id, tok_a, tok_b, cfg));
    }

    // ------------------------------------------------------------------------
    // Launch all 24 concurrent game tasks + concurrent health/lobby latency probe
    // ------------------------------------------------------------------------
    let mut join_set: tokio::task::JoinSet<Result<Vec<String>, String>> =
        tokio::task::JoinSet::new();

    // Group 1: 8 Human-vs-Human Matchmaking + TUI ClientApp + RemoteClient + live SSE games
    for (idx, game_id, tok_a, tok_b, cfg) in human_matches {
        let c = client.clone();
        join_set.spawn(async move {
            let mut recorded_ids = vec![game_id.clone()];
            // Subscribe Seat A and Spectator to live SSE event streams over TCP:
            let mut sse_a = c.subscribe_events(&game_id, Some(&tok_a)).await?;
            let mut sse_spec = c.subscribe_events(&game_id, None).await?;

            let init_a = tokio::time::timeout(Duration::from_secs(5), sse_a.next_view())
                .await
                .map_err(|_| "SSE A initial timeout".to_string())??;
            assert_eq!(init_a.viewer_seat, Some(Seat::A));
            let init_spec = tokio::time::timeout(Duration::from_secs(5), sse_spec.next_view())
                .await
                .map_err(|_| "SSE Spec initial timeout".to_string())??;
            assert_eq!(init_spec.viewer_seat, None);

            // Also drive local TUI ClientApp and client-side selection offloading:
            let mut tui_app = ClientApp::new(ClientMode::LocalPassAndPlay, cfg.clone())?;
            let _ = tui_app.handle_key(KeyCommand::MoveCursor(1, 0));
            let _ = tui_app.render_plain();

            // Turn 0: If idx % 2 == 0, trigger a deliberate conflict re-entry sub-round first!
            let mut view_a = c.get_view(&game_id, Some(&tok_a)).await?;
            if idx % 2 == 0 {
                if let Some((mv_a, mv_b)) = pick_colliding_pair(&view_a) {
                    ClientApp::apply_client_selection(&mut view_a, Some(mv_a.0))?;
                    assert!(view_a.seats[0].legal_targets.contains(&mv_a.1));
                    let after_a = c.submit_move(&game_id, &tok_a, mv_a.0, mv_a.1).await?;
                    assert!(after_a.seats[0].has_committed);

                    let after_b = c.submit_move(&game_id, &tok_b, mv_b.0, mv_b.1).await?;
                    // Conflict re-entry sub-round: conflict_round_no incremented and marks added!
                    assert!(
                        after_b.conflict_round_no >= 1 || !after_b.marks.is_empty(),
                        "colliding moves in Conflict mode must trigger conflict re-entry"
                    );
                }
            }

            // Play 2 full non-colliding turns
            for _turn in 0..2 {
                let v = c.get_view(&game_id, Some(&tok_a)).await?;
                if v.ended {
                    break;
                }
                let (mv_a, mv_b) = pick_non_colliding_pair(&v);
                let mut local_v = v.clone();
                ClientApp::apply_client_selection(&mut local_v, Some(mv_a.0))?;
                assert!(local_v.seats[0].executable_targets.contains(&mv_a.1));

                c.submit_move(&game_id, &tok_a, mv_a.0, mv_a.1).await?;
                c.submit_move(&game_id, &tok_b, mv_b.0, mv_b.1).await?;

                // Verify live SSE stream delivers updated GameView even across RAM evictions
                let sse_update = tokio::time::timeout(Duration::from_secs(5), sse_a.next_view())
                    .await
                    .map_err(|_| "SSE A update timeout".to_string())??;
                assert!(sse_update.revision > init_a.revision);

                let spec_update =
                    tokio::time::timeout(Duration::from_secs(5), sse_spec.next_view())
                        .await
                        .map_err(|_| "SSE Spec update timeout".to_string())??;
                assert_eq!(spec_update.viewer_seat, None);
                for s in &spec_update.seats {
                    if !s.is_locked && !s.has_revealed {
                        assert_eq!(s.visible_move, None);
                    }
                }
            }

            // For idx == 0, test resign + rematch flow!
            if idx == 0 {
                let resigned = c.resign(&game_id, &tok_a).await?;
                assert!(resigned.ended);
                assert_eq!(resigned.winner, Some(Seat::B));

                // Rematch: create a fresh game with the same config and play a turn
                let rematch = c
                    .create_game(cfg, None, Seat::A, Some("HumanA-0-Rematch".to_string()))
                    .await?;
                let rm_id = rematch.game_id;
                let rm_tok_a = rematch.creator_token.unwrap();
                let rm_join = c
                    .join_game(&rm_id, Some(Seat::B), Some("HumanB-0-Rematch".to_string()))
                    .await?;
                let rm_tok_b = rm_join.seat_token;
                let (mv_a, mv_b) = pick_non_colliding_pair(&rm_join.view);
                c.submit_move(&rm_id, &rm_tok_a, mv_a.0, mv_a.1).await?;
                let rm_after = c.submit_move(&rm_id, &rm_tok_b, mv_b.0, mv_b.1).await?;
                assert!(rm_after.turn_no >= 1 || rm_after.conflict_round_no >= 1);
                recorded_ids.push(rm_id);
            }

            Ok(recorded_ids)
        });
    }

    // Group 2: 8 Cryptographic BlindHash Commit-Reveal Games
    for idx in 0..8usize {
        let c = client.clone();
        let preset = presets[idx % presets.len()];
        join_set.spawn(async move {
            let cfg = GameConfig {
                preset,
                tie_break: TieBreakMode::Column,
                confluence: if idx % 2 == 0 {
                    ConfluenceMode::BunchBefore
                } else {
                    ConfluenceMode::Annihilate
                },
                two_cycle_swaps: true,
                cooldown_turns: 1,
                max_turns: 40,
                railgun_fling: false,
                automata_count: 1,
                commit_mode: CommitMode::ExplicitCommitReveal,
            };
            let created = c
                .create_game(cfg, None, Seat::A, Some(format!("CryptoA-{idx}")))
                .await?;
            let game_id = created.game_id;
            let tok_a = created.creator_token.unwrap();
            let joined = c
                .join_game(&game_id, Some(Seat::B), Some(format!("CryptoB-{idx}")))
                .await?;
            let tok_b = joined.seat_token;

            for turn in 0..2u32 {
                let v = c.get_view(&game_id, Some(&tok_a)).await?;
                if v.ended {
                    break;
                }
                let (mv_a, mv_b) = pick_non_colliding_pair(&v);
                let nonce_a = format!("salt-a-{idx}-{turn}");
                let nonce_b = format!("salt-b-{idx}-{turn}");
                let hash_a = RemoteClient::compute_commitment(
                    &game_id,
                    Seat::A,
                    v.turn_no as u32,
                    v.conflict_round_no as u32,
                    mv_a.0,
                    mv_a.1,
                    &nonce_a,
                );
                let hash_b = RemoteClient::compute_commitment(
                    &game_id,
                    Seat::B,
                    v.turn_no as u32,
                    v.conflict_round_no as u32,
                    mv_b.0,
                    mv_b.1,
                    &nonce_b,
                );

                let after_ca = c.commit_blind_hash(&game_id, &tok_a, &hash_a).await?;
                assert!(after_ca.seats[0].has_committed);
                assert_eq!(after_ca.seats[0].visible_move, None);

                let after_cb = c.commit_blind_hash(&game_id, &tok_b, &hash_b).await?;
                assert_eq!(after_cb.phase, Phase::Reveal);

                c.reveal_move(&game_id, &tok_a, Some((mv_a.0, mv_a.1, nonce_a)))
                    .await?;
                let after_rb = c
                    .reveal_move(&game_id, &tok_b, Some((mv_b.0, mv_b.1, nonce_b)))
                    .await?;
                assert_eq!(after_rb.phase, Phase::Resolve);

                let resolved = c.resolve_round(&game_id, Some(&tok_a)).await?;
                assert_eq!(resolved.turn_no, (turn + 1) as u64);
            }

            Ok(vec![game_id])
        });
    }

    // Group 3: 8 Server-Hosted AI Bot Games across all BotStrategy variants and rulesets
    let bot_strategies = [
        BotStrategy::Novice,
        BotStrategy::Tactical,
        BotStrategy::Minimax,
        BotStrategy::DecoupledUct,
        BotStrategy::RegretMatching,
        BotStrategy::Novice,
        BotStrategy::Tactical,
        BotStrategy::RegretMatching,
    ];
    for (idx, strategy) in bot_strategies.into_iter().enumerate() {
        let c = client.clone();
        join_set.spawn(async move {
            let preset = match strategy {
                BotStrategy::Minimax | BotStrategy::DecoupledUct => BoardPreset::Mini5x5,
                _ => presets[idx % presets.len()],
            };
            let cfg = GameConfig {
                preset,
                tie_break: match idx % 4 {
                    0 => TieBreakMode::Column,
                    1 => TieBreakMode::Row,
                    2 => TieBreakMode::Freeze,
                    _ => TieBreakMode::Momentum {
                        prefer_same_axis: true,
                    },
                },
                confluence: ConfluenceMode::BunchBefore,
                two_cycle_swaps: idx % 2 == 0,
                cooldown_turns: (idx % 2) as u32,
                max_turns: 40,
                railgun_fling: idx % 3 == 0,
                automata_count: ((idx % 2) + 1) as u32,
                commit_mode: CommitMode::AutoRevealOnBothSealed,
            };
            let created = c
                .create_game(cfg, Some(strategy), Seat::A, Some(format!("BotPlay-{idx}")))
                .await?;
            let game_id = created.game_id;
            let tok_a = created.creator_token.unwrap();

            for _step in 0..6 {
                let v = c.get_view(&game_id, Some(&tok_a)).await?;
                if v.ended || v.turn_no >= 2 {
                    break;
                }
                let (mv_a, _) = pick_non_colliding_pair(&v);
                let after = c.submit_move(&game_id, &tok_a, mv_a.0, mv_a.1).await?;
                assert!(
                    after.turn_no > v.turn_no
                        || after.conflict_round_no > v.conflict_round_no
                        || after.ended
                );
            }

            Ok(vec![game_id])
        });
    }

    // ------------------------------------------------------------------------
    // Measure health check (`GET /api/health`) and lobby (`GET /api/games`)
    // latency WHILE the 24 concurrent games are actively running!
    // ------------------------------------------------------------------------
    for _ in 0..6 {
        let t_health = Instant::now();
        let ok = client.health_check().await.unwrap();
        let health_elapsed = t_health.elapsed();
        assert!(ok);
        assert!(
            health_elapsed < Duration::from_millis(50),
            "GET /api/health took {health_elapsed:?} (> 50ms) during concurrent load"
        );

        let t_lobby = Instant::now();
        let _lobby = client.list_games().await.unwrap();
        let lobby_elapsed = t_lobby.elapsed();
        assert!(
            lobby_elapsed < Duration::from_millis(50),
            "GET /api/games took {lobby_elapsed:?} (> 50ms) during concurrent load"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Wait for all 24 concurrent game tasks to complete with zero errors or panics:
    let mut all_game_ids = Vec::new();
    while let Some(res) = join_set.join_next().await {
        let ids = res
            .expect("task must not panic")
            .expect("game flow must not return HTTP 5xx or client error");
        all_game_ids.extend(ids);
    }
    assert!(all_game_ids.len() >= 24);

    // Assert bounded RAM: `state.in_memory_game_count() <= 8` holds at the end of the burst!
    state.evict_excess_games(None);
    assert!(
        state.in_memory_game_count() <= 8,
        "in_memory_game_count must be <= 8 after 24-game burst, got {}",
        state.in_memory_game_count()
    );

    // Assert every single one of the 24+ persisted games on disk passes GET /api/games/{id}/replay
    // (`replay_verified: true`) AND `store.load_all_and_verify()`!
    for gid in &all_game_ids {
        let rep = client.export_replay(gid).await.unwrap();
        assert!(
            rep.replay_verified,
            "persisted game {gid} failed GET /api/games/{gid}/replay verification"
        );
    }

    let disk_verified = state
        .store()
        .load_all_and_verify()
        .expect("store.load_all_and_verify() must succeed on all persisted games");
    assert_eq!(disk_verified.len(), all_game_ids.len());

    server_handle.abort();
}
