use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use automatafl_server::api::{
    build_router, ActionResponse, AppState, CreateGameResponse, JoinGameResponse, ReplayExport,
};
use automatafl_server::bot::{choose_bot_move, BotStrategy};
use automatafl_server::matchmaking::SeekStatus;
use automatafl_server::oracle;
use automatafl_server::persistence::GameStore;
use automatafl_server::protocol::{
    Board, BoardPreset, CommitMode, ConfluenceMode, GameConfig, Move, RoundOutcome, Seat,
    TieBreakMode, ATT, AUTO, REP, VAC,
};
use automatafl_server::session::{GameSession, GameView, Phase};

async fn send_json(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(tok) = token {
        builder = builder.header("authorization", format!("Bearer {tok}"));
    }
    let req = builder
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn get_json(
    app: &axum::Router,
    uri: &str,
    token: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(tok) = token {
        builder = builder.header("authorization", format!("Bearer {tok}"));
    }
    let req = builder.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn fog_of_war_never_leaks_secret_moves_to_opponent_or_spectator() {
    let state = AppState::new_in_memory();
    let app = build_router(Arc::clone(&state));

    // Create a human-vs-human game on the 11x11 Classic board.
    let (status, create_val) = send_json(
        &app,
        "POST",
        "/api/games",
        None,
        serde_json::json!({
            "creator_seat": "A",
            "player_name": "Alice"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let created: CreateGameResponse = serde_json::from_value(create_val).unwrap();
    let game_id = created.game_id;
    let token_a = created.creator_token.unwrap();

    // Seat B joins via /join.
    let (status, join_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/join"),
        None,
        serde_json::json!({ "player_name": "Bob" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let joined: JoinGameResponse = serde_json::from_value(join_val).unwrap();
    assert_eq!(joined.seat, Seat::B);
    let token_b = joined.seat_token;
    assert_ne!(token_a, token_b);

    // Seat A selects piece at (3, 1) (Attractor on stock 11x11 board).
    let (status, sel_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/select"),
        Some(&token_a),
        serde_json::json!({ "coord": [3, 1] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let sel_resp: ActionResponse = serde_json::from_value(sel_val).unwrap();
    assert_eq!(sel_resp.view.seats[0].own_selection, Some((3, 1)));
    assert!(!sel_resp.view.seats[0].legal_targets.is_empty());

    // Seat A submits a secret move: (3, 1) -> (3, 2).
    let secret_from = (3, 1);
    let secret_to = (3, 2);
    let (status, mv_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_a),
        serde_json::json!({
            "from": secret_from,
            "to": secret_to
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let view_a: ActionResponse = serde_json::from_value(mv_val).unwrap();
    assert_eq!(
        view_a.view.seats[0].visible_move,
        Some(Move {
            who: 0,
            frm: secret_from,
            to: secret_to
        })
    );
    assert!(view_a.view.seats[0].has_committed);
    assert!(!view_a.view.seats[1].has_committed);

    // Inspect Seat B's view BEFORE Seat B submits:
    // Seat B may know Seat A has sealed (`has_committed == true` and `commitment_seal`),
    // but MUST NOT see `visible_move`, `own_selection`, `own_move_forecast`, or `"sealed"` cell tags.
    let (status, raw_b) = get_json(&app, &format!("/api/games/{game_id}"), Some(&token_b)).await;
    assert_eq!(status, StatusCode::OK);
    let view_b: GameView = serde_json::from_value(raw_b.clone()).unwrap();
    assert_eq!(view_b.viewer_seat, Some(Seat::B));
    assert!(view_b.seats[0].has_committed);
    assert!(view_b.seats[0].commitment_seal.is_some());
    assert_eq!(view_b.seats[0].visible_move, None);
    assert_eq!(view_b.seats[0].own_selection, None);
    assert_eq!(view_b.seats[0].own_move_forecast, None);
    assert!(view_b.seats[0].legal_targets.is_empty());
    assert!(view_b.cells.iter().all(|c| c.tag != "sealed"));

    // Inspect Spectator's view (no token):
    let (status, raw_spec) = get_json(&app, &format!("/api/games/{game_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    let view_spec: GameView = serde_json::from_value(raw_spec).unwrap();
    assert_eq!(view_spec.viewer_seat, None);
    assert!(view_spec.seats[0].has_committed);
    assert_eq!(view_spec.seats[0].visible_move, None);
    assert_eq!(view_spec.seats[0].own_selection, None);

    // Spectator cannot submit a move or select a piece.
    let (status, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        None,
        serde_json::json!({
            "from": [7, 9],
            "to": [7, 8]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Now Seat B submits a non-clashing move: (7, 9) -> (7, 8).
    // In AutoRevealOnBothSealed mode, this automatically reveals and resolves the round!
    let (status, mv_b_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_b),
        serde_json::json!({
            "from": [7, 9],
            "to": [7, 8]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let after_b: ActionResponse = serde_json::from_value(mv_b_val).unwrap();
    assert_eq!(after_b.view.turn_no, 1);
    assert_eq!(after_b.view.turns_played_count, 1);
    assert_eq!(after_b.view.board.cell_at((3, 2)), ATT);
    assert_eq!(after_b.view.board.cell_at((7, 8)), ATT);
}

#[tokio::test]
async fn conflict_reentry_marks_contested_squares_and_locks_non_clashing_moves() {
    let state = AppState::new_in_memory();
    let app = build_router(Arc::clone(&state));

    let (_, create_val) = send_json(
        &app,
        "POST",
        "/api/games",
        None,
        serde_json::json!({ "creator_seat": "A" }),
    )
    .await;
    let created: CreateGameResponse = serde_json::from_value(create_val).unwrap();
    let game_id = created.game_id;
    let token_a = created.creator_token.unwrap();
    let stale_invite_token = created.open_invite_token.unwrap();

    // Claim seat B via join: this rotates Seat B's token so stale_invite_token is invalidated!
    let (st_join, join_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/join"),
        None,
        serde_json::json!({ "player_name": "Bob" }),
    )
    .await;
    assert_eq!(st_join, StatusCode::OK);
    let joined: JoinGameResponse = serde_json::from_value(join_val).unwrap();
    let token_b = joined.seat_token;
    assert_ne!(token_b, stale_invite_token);

    // Attempting to use stale_invite_token after /join must fail with 401 UNAUTHORIZED!
    let (st_stale, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&stale_invite_token),
        serde_json::json!({ "from": [3, 1], "to": [3, 3] }),
    )
    .await;
    assert_eq!(st_stale, StatusCode::UNAUTHORIZED);

    // Fork clash: Both Seat A and Seat B try to move the same piece at (3, 1) to different squares!
    // Seat A: (3, 1) -> (3, 2)
    // Seat B: (3, 1) -> (3, 3)
    let (st_a, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_a),
        serde_json::json!({ "from": [3, 1], "to": [3, 2] }),
    )
    .await;
    assert_eq!(st_a, StatusCode::OK);

    let (st_b, clash_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_b),
        serde_json::json!({ "from": [3, 1], "to": [3, 3] }),
    )
    .await;
    assert_eq!(st_b, StatusCode::OK);
    let clash_resp: ActionResponse = serde_json::from_value(clash_val).unwrap();

    // Round must enter Resubmit with (3, 1) marked!
    assert_eq!(clash_resp.view.phase, Phase::Resubmit);
    assert_eq!(clash_resp.view.conflict_round_no, 1);
    assert!(clash_resp.view.marks.contains(&(3, 1)));
    assert_eq!(clash_resp.view.waiting_seats, vec![0, 1]);

    // Attempting to move from the marked square (3, 1) again must fail!
    let (st_illegal, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_a),
        serde_json::json!({ "from": [3, 1], "to": [3, 4] }),
    )
    .await;
    assert_eq!(st_illegal, StatusCode::BAD_REQUEST);

    // Second clash on the resubmit (destination collision at (5, 4)):
    // Seat A moves Attractor (1, 4) -> (5, 4)
    // Seat B moves Attractor (9, 4) -> (5, 4)
    let (st_a2, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_a),
        serde_json::json!({ "from": [1, 4], "to": [5, 4] }),
    )
    .await;
    assert_eq!(st_a2, StatusCode::OK);

    let (st_b2, collide_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_b),
        serde_json::json!({ "from": [9, 4], "to": [5, 4] }),
    )
    .await;
    assert_eq!(st_b2, StatusCode::OK);
    let collide_resp: ActionResponse = serde_json::from_value(collide_val).unwrap();

    // Marks strictly grew to include both (3, 1) and (5, 4)!
    assert_eq!(collide_resp.view.phase, Phase::Resubmit);
    assert_eq!(collide_resp.view.conflict_round_no, 2);
    assert!(collide_resp.view.marks.contains(&(3, 1)));
    assert!(collide_resp.view.marks.contains(&(5, 4)));
    assert_eq!(collide_resp.view.waiting_seats, vec![0, 1]);

    // Moving to the newly marked square (5, 4) is now rejected!
    let (st_marked_dst, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_a),
        serde_json::json!({ "from": [1, 4], "to": [5, 4] }),
    )
    .await;
    assert_eq!(st_marked_dst, StatusCode::BAD_REQUEST);

    // Round 3: Clean moves from both seats resolve the turn!
    let (st_a3, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_a),
        serde_json::json!({ "from": [1, 5], "to": [2, 5] }),
    )
    .await;
    assert_eq!(st_a3, StatusCode::OK);

    let (st_b3, resolved_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_b),
        serde_json::json!({ "from": [9, 5], "to": [8, 5] }),
    )
    .await;
    assert_eq!(st_b3, StatusCode::OK);
    let resolved_resp: ActionResponse = serde_json::from_value(resolved_val).unwrap();
    assert_eq!(resolved_resp.view.turn_no, 1);
    assert_eq!(resolved_resp.view.conflict_round_no, 0);
    assert!(resolved_resp.view.marks.is_empty());
    assert_eq!(resolved_resp.view.turns_played_count, 1);

    // Also verify partial clash locking at the oracle level when a non-clashing move coexists
    // with two clashing moves:
    let stock = oracle::stock_two_player().unwrap();
    let goals = oracle::stock_goals2(11).unwrap();
    let partial_out = oracle::round(
        &stock,
        &goals,
        &[],
        &[],
        &[0, 1, 2],
        &[
            Move {
                who: 0,
                frm: (1, 5),
                to: (2, 5),
            },
            Move {
                who: 1,
                frm: (3, 1),
                to: (3, 2),
            },
            Move {
                who: 2,
                frm: (3, 1),
                to: (3, 3),
            },
        ],
    )
    .unwrap();
    match partial_out {
        RoundOutcome::Again {
            marks,
            locked,
            waiting,
        } => {
            assert_eq!(marks, vec![(3, 1)]);
            assert_eq!(
                locked,
                vec![Move {
                    who: 0,
                    frm: (1, 5),
                    to: (2, 5)
                }]
            );
            assert_eq!(waiting, vec![1, 2]);
        }
        other => panic!("expected RoundOutcome::Again, got {other:?}"),
    }
}

#[tokio::test]
async fn blind_cryptographic_commit_and_reveal_workflow() {
    let mut cfg = GameConfig::default();
    cfg.commit_mode = CommitMode::ExplicitCommitReveal;
    let mut session = GameSession::new(
        "af-blind".to_string(),
        "Blind Table".to_string(),
        true,
        cfg,
        42,
        1000,
    )
    .unwrap();

    let frm_a = (3, 1);
    let to_a = (3, 2);
    let frm_b = (7, 9);
    let to_b = (7, 8);
    let nonce_a = "secret-nonce-alice-999";
    let nonce_b = "secret-nonce-bob-777";

    let hash_a =
        GameSession::compute_client_commitment("af-blind", 0, 0, Seat::A, frm_a, to_a, nonce_a);
    let hash_b =
        GameSession::compute_client_commitment("af-blind", 0, 0, Seat::B, frm_b, to_b, nonce_b);

    session.commit_blind_hash(Seat::A, hash_a).unwrap();
    assert_eq!(session.phase(), Phase::Commit);
    session.commit_blind_hash(Seat::B, hash_b).unwrap();
    // Both committed -> transitions to Reveal phase!
    assert_eq!(session.phase(), Phase::Reveal);

    // Attempting to reveal with a wrong nonce or altered move fails!
    assert!(session
        .reveal_move(Seat::A, Some((frm_a, to_a, "wrong-nonce".to_string())))
        .is_err());
    assert!(session
        .reveal_move(Seat::A, Some(((3, 1), (3, 3), nonce_a.to_string())))
        .is_err());

    // Valid reveals succeed and transition to Resolve, then resolve_round executes the turn.
    session
        .reveal_move(Seat::A, Some((frm_a, to_a, nonce_a.to_string())))
        .unwrap();
    assert_eq!(session.phase(), Phase::Reveal);
    session
        .reveal_move(Seat::B, Some((frm_b, to_b, nonce_b.to_string())))
        .unwrap();
    assert_eq!(session.phase(), Phase::Resolve);

    session.resolve_round().unwrap();
    assert_eq!(session.turn_no, 1);
    assert_eq!(session.phase(), Phase::Commit);
    assert_eq!(session.board.cell_at((3, 2)), ATT);
    assert_eq!(session.board.cell_at((7, 8)), ATT);
}

#[tokio::test]
async fn matchmaking_pairs_compatible_seekers_and_isolates_seat_tokens() {
    let state = AppState::new_in_memory();
    let app = build_router(Arc::clone(&state));

    // Seeker 1 queues for Blitz7x7.
    let (st1, val1) = send_json(
        &app,
        "POST",
        "/api/matchmaking/seek",
        None,
        serde_json::json!({
            "player_name": "BlitzPlayer1",
            "config": { "preset": "blitz7x7" }
        }),
    )
    .await;
    assert_eq!(st1, StatusCode::OK);
    let s1: SeekStatus = serde_json::from_value(val1).unwrap();
    let ticket_1 = match s1 {
        SeekStatus::Queued {
            ticket_id,
            preset,
            position_in_queue,
        } => {
            assert_eq!(preset, BoardPreset::Blitz7x7);
            assert_eq!(position_in_queue, 1);
            ticket_id
        }
        other => panic!("expected Queued, got {other:?}"),
    };

    // Seeker 2 queues for Classic11x11 (should NOT match Blitz7x7).
    let (st2, val2) = send_json(
        &app,
        "POST",
        "/api/matchmaking/seek",
        None,
        serde_json::json!({
            "player_name": "ClassicPlayer",
            "config": { "preset": "classic11x11" }
        }),
    )
    .await;
    assert_eq!(st2, StatusCode::OK);
    let s2: SeekStatus = serde_json::from_value(val2).unwrap();
    let ticket_2 = match s2 {
        SeekStatus::Queued {
            ticket_id,
            preset,
            position_in_queue,
        } => {
            assert_eq!(preset, BoardPreset::Classic11x11);
            assert_eq!(position_in_queue, 2);
            ticket_id
        }
        other => panic!("expected Queued, got {other:?}"),
    };

    // Seeker 3 queues for Blitz7x7 -> immediately matches Seeker 1!
    let (st3, val3) = send_json(
        &app,
        "POST",
        "/api/matchmaking/seek",
        None,
        serde_json::json!({
            "player_name": "BlitzPlayer2",
            "config": { "preset": "blitz7x7" }
        }),
    )
    .await;
    assert_eq!(st3, StatusCode::OK);
    let s3: SeekStatus = serde_json::from_value(val3).unwrap();
    let (game_id_3, seat_3, token_3) = match s3 {
        SeekStatus::Matched {
            game_id,
            seat,
            seat_token,
            ..
        } => (game_id, seat, seat_token),
        other => panic!("expected Matched for seeker 3, got {other:?}"),
    };
    assert_eq!(seat_3, Seat::B);

    // Poll Seeker 1's ticket -> now Matched with Seat A and a distinct token!
    let (st_poll, val_poll) =
        get_json(&app, &format!("/api/matchmaking/status/{ticket_1}"), None).await;
    assert_eq!(st_poll, StatusCode::OK);
    let s1_after: SeekStatus = serde_json::from_value(val_poll).unwrap();
    let (game_id_1, seat_1, token_1) = match s1_after {
        SeekStatus::Matched {
            game_id,
            seat,
            seat_token,
            ..
        } => (game_id, seat, seat_token),
        other => panic!("expected Matched for seeker 1, got {other:?}"),
    };
    assert_eq!(game_id_1, game_id_3);
    assert_eq!(seat_1, Seat::A);
    assert_ne!(token_1, token_3);

    // Verify the created game has a 7x7 board.
    let (_, game_val) = get_json(&app, &format!("/api/games/{game_id_1}"), Some(&token_1)).await;
    let view: GameView = serde_json::from_value(game_val).unwrap();
    assert_eq!(view.board.n, 7);

    // Cancel Seeker 2's ticket.
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/matchmaking/seek/{ticket_2}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn play_against_all_simultaneous_ai_bot_strategies() {
    let strategies = [
        BotStrategy::Novice,
        BotStrategy::Tactical,
        BotStrategy::Minimax,
        BotStrategy::DecoupledUct,
        BotStrategy::RegretMatching,
    ];

    for strategy in strategies {
        let mut cfg = GameConfig::default();
        cfg.preset = BoardPreset::Blitz7x7;
        let mut session = GameSession::new(
            format!("bot-{strategy:?}"),
            "Bot Test".to_string(),
            true,
            cfg,
            12345,
            1000,
        )
        .unwrap();
        session.set_bot(Seat::B, strategy);

        // Submit moves for Seat A (including any conflict re-entry rounds if Seat A and Bot clashed)
        // until Turn 1 completes.
        let mut attempt = 0u64;
        while session.turn_no == 0 && !session.ended {
            attempt += 1;
            let mv_a = choose_bot_move(
                BotStrategy::Novice,
                &session.board,
                &session.goals,
                &session.marks,
                &session.locked,
                Seat::A,
                999 + attempt,
            )
            .expect("at least one legal move");
            session.submit_move(Seat::A, mv_a.frm, mv_a.to).unwrap();
        }
        assert_eq!(session.turn_no, 1);
        assert!(session.verify_replay().is_ok());
    }
}

#[test]
fn r2_variant_modes_bunch_before_annihilate_swaps_cooldown_and_tiebreak() {
    // 1. ConfluenceMode::BunchBefore:
    // Construct a 5x5 board with two Repulsors moving toward (2, 2) from (0, 2) and (4, 2),
    // and Automaton at (2, 0).
    let mut cells = vec![VAC; 25];
    cells[0 * 5 + 2] = AUTO;
    cells[2 * 5 + 0] = REP;
    cells[2 * 5 + 4] = REP;
    let b = Board {
        n: 5,
        cells,
        auto: (2, 0),
        col_rule: true,
    };

    let mut cfg_bunch = GameConfig::default();
    cfg_bunch.preset = BoardPreset::Mini5x5;
    cfg_bunch.confluence = ConfluenceMode::BunchBefore;
    let goals = oracle::stock_goals2(5).unwrap();
    let moves = vec![
        Move {
            who: 0,
            frm: (0, 2),
            to: (2, 2),
        },
        Move {
            who: 1,
            frm: (4, 2),
            to: (2, 2),
        },
    ];
    let (step_bunch, _) =
        oracle::round_with_config(&cfg_bunch, None, &b, &goals, &[], &[], &[0, 1], &moves).unwrap();
    match step_bunch {
        RoundOutcome::Resolved { board: after, .. } => {
            // Both pieces stop 1 cell short of (2, 2): at (1, 2) and (3, 2)!
            assert_eq!(after.cell_at((1, 2)), REP);
            assert_eq!(after.cell_at((3, 2)), REP);
            assert_eq!(after.cell_at((2, 2)), VAC);
        }
        RoundOutcome::Again { .. } => {
            panic!("BunchBefore must not re-enter conflict resolution")
        }
    }

    // 2. ConfluenceMode::Annihilate on the same collision:
    let mut cfg_ann = cfg_bunch.clone();
    cfg_ann.confluence = ConfluenceMode::Annihilate;
    let (step_ann, _) =
        oracle::round_with_config(&cfg_ann, None, &b, &goals, &[], &[], &[0, 1], &moves).unwrap();
    match step_ann {
        RoundOutcome::Resolved { board: after, .. } => {
            // Both colliding movers annihilate each other!
            assert_eq!(after.cell_at((0, 2)), VAC);
            assert_eq!(after.cell_at((2, 2)), VAC);
            assert_eq!(after.cell_at((4, 2)), VAC);
        }
        RoundOutcome::Again { .. } => {
            panic!("Annihilate must not re-enter conflict resolution")
        }
    }

    // 3. Two-cycle swaps: canonical (false) vs variant (true)
    let mut swap_cells = vec![VAC; 25];
    swap_cells[0 * 5 + 2] = AUTO;
    swap_cells[2 * 5 + 1] = REP;
    swap_cells[2 * 5 + 2] = ATT;
    let b_swap = Board {
        n: 5,
        cells: swap_cells,
        auto: (2, 0),
        col_rule: true,
    };
    let swap_moves = vec![
        Move {
            who: 0,
            frm: (1, 2),
            to: (2, 2),
        },
        Move {
            who: 1,
            frm: (2, 2),
            to: (1, 2),
        },
    ];
    let mut cfg_no_swap = GameConfig::default();
    cfg_no_swap.preset = BoardPreset::Mini5x5;
    cfg_no_swap.two_cycle_swaps = false;
    let (_, mid_no_swap) = oracle::round_with_config(
        &cfg_no_swap,
        None,
        &b_swap,
        &goals,
        &[],
        &[],
        &[0, 1],
        &swap_moves,
    )
    .unwrap();
    assert_eq!(mid_no_swap.cell_at((1, 2)), REP);
    assert_eq!(mid_no_swap.cell_at((2, 2)), ATT);

    let mut cfg_yes_swap = cfg_no_swap.clone();
    cfg_yes_swap.two_cycle_swaps = true;
    let (_, mid_yes_swap) = oracle::round_with_config(
        &cfg_yes_swap,
        None,
        &b_swap,
        &goals,
        &[],
        &[],
        &[0, 1],
        &swap_moves,
    )
    .unwrap();
    assert_eq!(mid_yes_swap.cell_at((1, 2)), ATT);
    assert_eq!(mid_yes_swap.cell_at((2, 2)), REP);

    // 4. TieBreakMode: Column vs Row vs Freeze on symmetric cross
    // Place Automaton at (2, 2), Attractors at (2, 0) [North/Y, dist 2] and (4, 2) [East/X, dist 2].
    let mut tie_cells = vec![VAC; 25];
    tie_cells[2 * 5 + 2] = AUTO;
    tie_cells[0 * 5 + 2] = ATT;
    tie_cells[2 * 5 + 4] = ATT;
    let b_tie = Board {
        n: 5,
        cells: tie_cells,
        auto: (2, 2),
        col_rule: true,
    };

    let mut cfg_col = GameConfig::default();
    cfg_col.tie_break = TieBreakMode::Column;
    let (out_col, _) =
        oracle::round_with_config(&cfg_col, None, &b_tie, &goals, &[], &[], &[], &[]).unwrap();
    match out_col {
        RoundOutcome::Resolved { board: after, .. } => assert_eq!(after.auto, (2, 1)), // Y axis wins under Column tie-break
        _ => panic!(),
    }

    let mut cfg_row = GameConfig::default();
    cfg_row.tie_break = TieBreakMode::Row;
    let (out_row, _) =
        oracle::round_with_config(&cfg_row, None, &b_tie, &goals, &[], &[], &[], &[]).unwrap();
    match out_row {
        RoundOutcome::Resolved { board: after, .. } => assert_eq!(after.auto, (3, 2)), // X axis wins under Row tie-break
        _ => panic!(),
    }

    let mut cfg_freeze = GameConfig::default();
    cfg_freeze.tie_break = TieBreakMode::Freeze;
    let (out_freeze, _) =
        oracle::round_with_config(&cfg_freeze, None, &b_tie, &goals, &[], &[], &[], &[]).unwrap();
    match out_freeze {
        RoundOutcome::Resolved { board: after, .. } => assert_eq!(after.auto, (2, 2)), // Automaton freezes on tie
        _ => panic!(),
    }

    // 5. Cooldown turns enforcement in GameSession:
    let mut cfg_cd = GameConfig::default();
    cfg_cd.preset = BoardPreset::Classic11x11;
    cfg_cd.cooldown_turns = 1;
    let mut session_cd =
        GameSession::new("cd-test".to_string(), "CD".to_string(), true, cfg_cd, 1, 0).unwrap();
    session_cd.submit_move(Seat::A, (3, 1), (3, 2)).unwrap();
    session_cd.submit_move(Seat::B, (7, 9), (7, 8)).unwrap();
    assert_eq!(session_cd.turn_no, 1);
    // Now (3, 2) and (7, 8) are on cooldown for 1 turn! Moving (3, 2) immediately on turn 1 must fail.
    let err = session_cd.submit_move(Seat::A, (3, 2), (3, 1));
    assert!(err.is_err());
}

#[tokio::test]
async fn persistence_saves_loads_and_verifies_replay_on_startup() {
    let dir = tempfile::tempdir().unwrap();
    let store = GameStore::disk(dir.path()).unwrap();
    let state = AppState::new(store.clone()).unwrap();
    let app = build_router(Arc::clone(&state));

    // Create a game vs Tactical AI bot on 7x7 Blitz.
    let (st, create_val) = send_json(
        &app,
        "POST",
        "/api/games",
        None,
        serde_json::json!({
            "config": { "preset": "blitz7x7" },
            "vs_bot": "tactical",
            "creator_seat": "A"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let created: CreateGameResponse = serde_json::from_value(create_val).unwrap();
    let game_id = created.game_id;
    let token_a = created.creator_token.unwrap();

    // Play at least 2 full turns.
    let mut step = 0u64;
    while step < 10 {
        step += 1;
        let (_, v_json) = get_json(&app, &format!("/api/games/{game_id}"), Some(&token_a)).await;
        let v: GameView = serde_json::from_value(v_json).unwrap();
        if v.ended || v.turn_no >= 2 {
            break;
        }
        let mv = choose_bot_move(
            BotStrategy::Novice,
            &v.board,
            &v.goals,
            &v.marks,
            &v.locked_moves,
            Seat::A,
            100 + step,
        )
        .expect("legal move");
        let (st_mv, _) = send_json(
            &app,
            "POST",
            &format!("/api/games/{game_id}/move"),
            Some(&token_a),
            serde_json::json!({ "from": mv.frm, "to": mv.to }),
        )
        .await;
        assert_eq!(st_mv, StatusCode::OK);
    }

    // Export replay and check replay_verified == true.
    let (st_rep, rep_val) = get_json(&app, &format!("/api/games/{game_id}/replay"), None).await;
    assert_eq!(st_rep, StatusCode::OK);
    let export: ReplayExport = serde_json::from_value(rep_val).unwrap();
    assert!(export.replay_verified);
    assert!(!export.turns_played.is_empty());

    // Now simulate server restart by constructing a brand-new AppState from the same disk directory!
    let reloaded_state = AppState::new(GameStore::disk(dir.path()).unwrap()).unwrap();
    let reloaded_app = build_router(reloaded_state);
    let (st_loaded, loaded_val) =
        get_json(&reloaded_app, &format!("/api/games/{game_id}"), Some(&token_a)).await;
    assert_eq!(st_loaded, StatusCode::OK);
    let loaded_view: GameView = serde_json::from_value(loaded_val).unwrap();
    assert_eq!(loaded_view.viewer_seat, Some(Seat::A));
    assert_eq!(loaded_view.turns_played_count, export.turns_played.len());
}

#[tokio::test]
async fn edge_cases_move_legality_automaton_dst_win_on_entry_turn_cap_and_resign() {
    let stock = oracle::stock_two_player().unwrap();

    // 1. Out-of-bounds, diagonal, stationary, and moving-automaton moves are rejected:
    assert!(!oracle::move_legal(&stock, &[], &Move { who: 0, frm: (-1, 1), to: (2, 1) }).unwrap());
    assert!(!oracle::move_legal(&stock, &[], &Move { who: 0, frm: (3, 1), to: (11, 1) }).unwrap());
    assert!(!oracle::move_legal(&stock, &[], &Move { who: 0, frm: (3, 1), to: (4, 2) }).unwrap());
    assert!(!oracle::move_legal(&stock, &[], &Move { who: 0, frm: (3, 1), to: (3, 1) }).unwrap());
    assert!(!oracle::move_legal(&stock, &[], &Move { who: 0, frm: (5, 5), to: (5, 4) }).unwrap());

    // 2. Naming the Automaton square (5, 5) as a DESTINATION is legal to propose (`move_legal == true`),
    //    and fails to execute at resolution (stays put):
    let to_auto = Move { who: 0, frm: (5, 1), to: (5, 5) };
    assert!(oracle::move_legal(&stock, &[], &to_auto).unwrap());
    let mid = oracle::resolve_mid(&stock, &[], &[to_auto]).unwrap();
    assert_eq!(mid.cell_at((5, 1)), REP);
    assert_eq!(mid.cell_at((5, 5)), AUTO);

    // 3. win_on_entry: if the automaton is ALREADY on a goal corner (0, 0) and does not move,
    //    it must NOT trigger a win (`win == None`).
    let mut b_on_goal = Board {
        n: 5,
        cells: vec![VAC; 25],
        auto: (0, 0),
        col_rule: true,
    };
    b_on_goal.cells[0] = AUTO;
    let goals5 = oracle::stock_goals2(5).unwrap();
    let (_, win_already) = oracle::turn(&b_on_goal, &[], &[], &goals5).unwrap();
    assert_eq!(win_already, None);

    // Conversely, when the automaton STEPS onto goal corner (0, 0) from (0, 1) (pushed by REP at (0, 2)),
    // it DOES trigger a win for Seat 0 (`win == Some(0)`):
    let mut b_step_goal = Board {
        n: 5,
        cells: vec![VAC; 25],
        auto: (0, 1),
        col_rule: true,
    };
    b_step_goal.cells[1 * 5 + 0] = AUTO;
    b_step_goal.cells[2 * 5 + 0] = REP; // pushes automaton from (0, 1) to (0, 0)
    let (after_entry, win_entry) = oracle::turn(&b_step_goal, &[], &[], &goals5).unwrap();
    assert_eq!(after_entry.auto, (0, 0));
    assert_eq!(win_entry, Some(0));

    // 4. Turn cap adjudication (`max_turns = 1`) and Resignation via HTTP API:
    let state = AppState::new_in_memory();
    let app = build_router(Arc::clone(&state));
    let (_, create_val) = send_json(
        &app,
        "POST",
        "/api/games",
        None,
        serde_json::json!({
            "config": { "preset": "classic11x11", "max_turns": 1 },
            "creator_seat": "A"
        }),
    )
    .await;
    let created: CreateGameResponse = serde_json::from_value(create_val).unwrap();
    let game_id = created.game_id;
    let token_a = created.creator_token.unwrap();
    let token_b = created.open_invite_token.unwrap();

    // Resign test on a second table:
    let (_, create_resign_val) = send_json(
        &app,
        "POST",
        "/api/games",
        None,
        serde_json::json!({ "creator_seat": "A" }),
    )
    .await;
    let created_resign: CreateGameResponse = serde_json::from_value(create_resign_val).unwrap();
    let (st_resign, resign_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{}/resign", created_resign.game_id),
        Some(created_resign.creator_token.as_deref().unwrap()),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(st_resign, StatusCode::OK);
    let resign_resp: ActionResponse = serde_json::from_value(resign_val).unwrap();
    assert!(resign_resp.view.ended);
    assert_eq!(resign_resp.view.winner, Some(Seat::B));

    // Play 1 turn on the `max_turns = 1` game where Seat A shoves the automaton North toward Seat A's goals:
    let _ = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_a),
        serde_json::json!({ "from": [5, 9], "to": [5, 6] }),
    )
    .await;
    let (st_cap, cap_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_b),
        serde_json::json!({ "from": [7, 9], "to": [7, 8] }),
    )
    .await;
    assert_eq!(st_cap, StatusCode::OK);
    let cap_resp: ActionResponse = serde_json::from_value(cap_val).unwrap();
    assert!(cap_resp.view.ended);
    assert_eq!(cap_resp.view.winner, Some(Seat::A));
}

#[test]
fn variants_lean_parity_blitz_balance_railgun_automata_count_bunch_chain_and_wire_verbs() {
    // 1. Blitz7x7 opening X/Y symmetry: Repulsors at (2, 1), (4, 1), (2, 5), (4, 5), vacuum at (3, 1) and (3, 5).
    let mut blitz_cfg = GameConfig::default();
    blitz_cfg.preset = BoardPreset::Blitz7x7;
    let blitz = oracle::stock_board_for_config(&blitz_cfg).unwrap();
    assert_eq!(blitz.cell_at((2, 1)), REP);
    assert_eq!(blitz.cell_at((4, 1)), REP);
    assert_eq!(blitz.cell_at((2, 5)), REP);
    assert_eq!(blitz.cell_at((4, 5)), REP);
    assert_eq!(blitz.cell_at((3, 1)), VAC);
    assert_eq!(blitz.cell_at((3, 5)), VAC);
    let sense_blitz = oracle::sense(&blitz).unwrap();
    assert_eq!(sense_blitz.offset, (0, 0));

    // 2. railgun_fling slides the automaton across contiguous vacuum and does NOT fling regular pieces:
    let mut cells = vec![VAC; 25];
    cells[2 * 5 + 1] = AUTO; // Automaton at (1, 2)
    cells[2 * 5 + 0] = REP;  // Repulsor at (0, 2) pushes automaton East (+1, 0)
    cells[0 * 5 + 0] = ATT;  // Regular piece at (0, 0) moving to (0, 1)
    let b_rail = Board {
        n: 5,
        cells,
        auto: (1, 2),
        col_rule: true,
    };
    let mut cfg_rail = GameConfig::default();
    cfg_rail.preset = BoardPreset::Mini5x5;
    cfg_rail.railgun_fling = true;
    let goals5 = oracle::stock_goals2(5).unwrap();
    let mv_piece = Move {
        who: 0,
        frm: (0, 0),
        to: (0, 1),
    };
    let (out_rail, mid_rail) =
        oracle::round_with_config(&cfg_rail, None, &b_rail, &goals5, &[], &[], &[0], &[mv_piece])
            .unwrap();
    // Regular piece stops at its target (0, 1), NOT flung to (0, 4):
    assert_eq!(mid_rail.cell_at((0, 1)), ATT);
    assert_eq!(mid_rail.cell_at((0, 4)), VAC);
    // Automaton flings across vacuum from (1, 2) all the way to (4, 2):
    match out_rail {
        RoundOutcome::Resolved { board: after, .. } => assert_eq!(after.auto, (4, 2)),
        other => panic!("expected Resolved, got {other:?}"),
    }

    // 3. automata_count = 2 steps the automaton twice in a single turn:
    let mut cfg_multi = GameConfig::default();
    cfg_multi.preset = BoardPreset::Mini5x5;
    cfg_multi.automata_count = 2;
    let (out_multi, _) =
        oracle::round_with_config(&cfg_multi, None, &b_rail, &goals5, &[], &[], &[], &[]).unwrap();
    match out_multi {
        RoundOutcome::Resolved { board: after, .. } => assert_eq!(after.auto, (3, 2)),
        other => panic!("expected Resolved, got {other:?}"),
    }

    // 4. ConfluenceMode::BunchBefore chain preservation:
    // Mover 0 at (0, 2) -> (1, 2), Mover 1 at (1, 2) -> (3, 2) (chaining to (3, 2)),
    // Mover 2 at (4, 2) -> (3, 2) (1-step colliding on (3, 2)).
    // Mover 0 claims (1, 2); Mover 1's chain fallback must NOT be overwritten by Mover 2!
    let mut bunch_cells = vec![VAC; 25];
    bunch_cells[0 * 5 + 2] = AUTO;
    bunch_cells[2 * 5 + 0] = ATT; // (0, 2)
    bunch_cells[2 * 5 + 1] = REP; // (1, 2)
    bunch_cells[2 * 5 + 4] = ATT; // (4, 2)
    let b_bunch_chain = Board {
        n: 5,
        cells: bunch_cells,
        auto: (2, 0),
        col_rule: true,
    };
    let mut cfg_bunch = GameConfig::default();
    cfg_bunch.preset = BoardPreset::Mini5x5;
    cfg_bunch.confluence = ConfluenceMode::BunchBefore;
    let chain_moves = vec![
        Move {
            who: 0,
            frm: (0, 2),
            to: (1, 2),
        },
        Move {
            who: 1,
            frm: (1, 2),
            to: (3, 2),
        },
        Move {
            who: 2,
            frm: (4, 2),
            to: (3, 2),
        },
    ];
    let (out_bunch_chain, _) = oracle::round_with_config(
        &cfg_bunch,
        None,
        &b_bunch_chain,
        &goals5,
        &[],
        &[],
        &[0, 1, 2],
        &chain_moves,
    )
    .unwrap();
    match out_bunch_chain {
        RoundOutcome::Resolved { board: after, .. } => {
            // Total piece count (2 ATT + 1 REP) is conserved!
            let att_count = after.cells.iter().filter(|&&c| c == ATT).count();
            let rep_count = after.cells.iter().filter(|&&c| c == REP).count();
            assert_eq!(att_count, 2);
            assert_eq!(rep_count, 1);
            // Contested destination (3, 2) stays empty!
            assert_eq!(after.cell_at((3, 2)), VAC);
        }
        other => panic!("expected Resolved, got {other:?}"),
    }

    // 5. ConfluenceMode::Annihilate stayer protection & wire verbs (`preset`, `stepcfg`, `midcfg`, `roundcfg`):
    let mut cfg_ann = GameConfig::default();
    cfg_ann.preset = BoardPreset::Mini5x5;
    cfg_ann.confluence = ConfluenceMode::Annihilate;
    let wire_preset = oracle::call_wire("preset 0").unwrap();
    assert!(wire_preset.starts_with("1 7 "));
    let wire_stepcfg = oracle::call_wire(&format!(
        "stepcfg {} 0 {}",
        cfg_rail.to_wire(),
        b_rail.to_wire()
    ))
    .unwrap();
    assert!(wire_stepcfg.starts_with("1 5 "));
    let wire_midcfg = oracle::call_wire(&format!(
        "midcfg {} {} 0 0",
        cfg_ann.to_wire(),
        b_rail.to_wire()
    ))
    .unwrap();
    assert!(wire_midcfg.starts_with("1 5 "));
}

#[test]
fn security_and_cooldown_regressions_blind_hash_chain_cooldown_and_selection_revision() {
    // 1. Short (< 32 chars) or non-hex blind hashes are rejected:
    let mut cfg = GameConfig::default();
    cfg.commit_mode = CommitMode::ExplicitCommitReveal;
    cfg.cooldown_turns = 1;
    let mut session = GameSession::new(
        "af-sec".to_string(),
        "Sec".to_string(),
        true,
        cfg,
        77,
        100,
    )
    .unwrap();
    assert!(session.commit_blind_hash(Seat::A, "deadbeef".to_string()).is_err());
    assert!(session
        .commit_blind_hash(Seat::A, "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz".to_string())
        .is_err());

    // 2. Private selection does NOT increment public `session.revision`:
    let rev_before = session.revision;
    session.select_source(Seat::A, (3, 1)).unwrap();
    assert_eq!(session.revision, rev_before);
    session.clear_selection(Seat::A);
    assert_eq!(session.revision, rev_before);

    // 3. Chain move cooldowns & cooldown enforcement on blind `reveal_move`:
    // Seat A moves (4, 0) -> (4, 1) and Seat B chains (4, 1) -> (4, 2) on turn 0:
    let h_a0 = GameSession::compute_client_commitment("af-sec", 0, 0, Seat::A, (4, 0), (4, 1), "n1");
    let h_b0 = GameSession::compute_client_commitment("af-sec", 0, 0, Seat::B, (4, 1), (4, 2), "n2");
    session.commit_blind_hash(Seat::A, h_a0).unwrap();
    session.commit_blind_hash(Seat::B, h_b0).unwrap();
    session
        .reveal_move(Seat::A, Some(((4, 0), (4, 1), "n1".to_string())))
        .unwrap();
    session
        .reveal_move(Seat::B, Some(((4, 1), (4, 2), "n2".to_string())))
        .unwrap();
    session.resolve_round().unwrap();
    assert_eq!(session.turn_no, 1);

    // Both (4, 1) AND (4, 2) landed in the chain, so BOTH must be on cooldown during turn 1!
    assert!(session.is_on_cooldown((4, 1)));
    assert!(session.is_on_cooldown((4, 2)));

    // Attempting to commit+reveal a move from cooldown square (4, 1) on turn 1 must be rejected at reveal_move!
    let h_a1 = GameSession::compute_client_commitment("af-sec", 1, 0, Seat::A, (4, 1), (4, 3), "n3");
    let h_b1 = GameSession::compute_client_commitment("af-sec", 1, 0, Seat::B, (7, 9), (7, 8), "n4");
    session.commit_blind_hash(Seat::A, h_a1).unwrap();
    session.commit_blind_hash(Seat::B, h_b1).unwrap();
    assert!(session
        .reveal_move(Seat::A, Some(((4, 1), (4, 3), "n3".to_string())))
        .is_err());
}

#[test]
fn lean_ffi_zero_twin_wire_and_bunch_before_caterpillar_permutation_invariance() {
    // 1. Verify `oracle::call_wire` dispatches directly to `automatafl_lean::rules_wire`
    //    across canonical and R2 variant wire verbs:
    let stock_wire = oracle::call_wire("stock").unwrap();
    assert_eq!(stock_wire, automatafl_lean::rules_wire("stock"));
    assert_eq!(stock_wire, oracle::eval_wire_canonical("stock"));

    for q in ["preset 0", "preset 1", "preset 2", "goals 11", "goals 7"] {
        let via_oracle = oracle::call_wire(q).unwrap();
        let via_lean = automatafl_lean::rules_wire(q);
        assert_eq!(via_oracle, via_lean, "mismatch on {q}");
        assert_eq!(via_oracle, oracle::eval_wire_canonical(q), "canonical mismatch on {q}");
    }

    // 2. 3-move caterpillar `BunchBefore` collision:
    //    m1: (0, 1) -> (0, 0), m2: (0, 0) -> (1, 0), m3: (2, 0) -> (1, 0).
    //    Here m2 and m3 collide on (1, 0), so m2 backs up to (0, 0), which in turn forces m1
    //    to back up to (0, 1). All 3 pieces must be conserved and the result must be 100%
    //    permutation-invariant across all 6 orderings of [m1, m2, m3].
    let mut cells = vec![VAC; 25];
    cells[4 * 5 + 4] = AUTO; // Automaton at (4, 4)
    cells[1 * 5 + 0] = ATT;  // m1 piece at (0, 1)
    cells[0 * 5 + 0] = REP;  // m2 piece at (0, 0)
    cells[0 * 5 + 2] = ATT;  // m3 piece at (2, 0)
    let b_cat = Board {
        n: 5,
        cells,
        auto: (4, 4),
        col_rule: true,
    };
    let goals5 = oracle::stock_goals2(5).unwrap();
    let mut cfg_bunch = GameConfig::default();
    cfg_bunch.preset = BoardPreset::Mini5x5;
    cfg_bunch.confluence = ConfluenceMode::BunchBefore;
    cfg_bunch.cooldown_turns = 1;

    let m1 = Move { who: 0, frm: (0, 1), to: (0, 0) };
    let m2 = Move { who: 1, frm: (0, 0), to: (1, 0) };
    let m3 = Move { who: 2, frm: (2, 0), to: (1, 0) };
    let perms = [
        [m1, m2, m3],
        [m1, m3, m2],
        [m2, m1, m3],
        [m2, m3, m1],
        [m3, m1, m2],
        [m3, m2, m1],
    ];

    for perm in &perms {
        let (outcome, mid) = oracle::round_with_config(
            &cfg_bunch,
            None,
            &b_cat,
            &goals5,
            &[],
            &[],
            &[0, 1, 2],
            perm,
        )
        .unwrap();
        // Verify all 3 pieces are conserved on `mid` and after the round:
        assert_eq!(mid.cell_at((0, 1)), ATT, "m1 piece at (0,1) must be conserved for perm {perm:?}");
        assert_eq!(mid.cell_at((0, 0)), REP, "m2 piece at (0,0) must be conserved for perm {perm:?}");
        assert_eq!(mid.cell_at((2, 0)), ATT, "m3 piece at (2,0) must be conserved for perm {perm:?}");
        assert_eq!(mid.cell_at((1, 0)), VAC, "contested merge (1,0) must stay VAC for perm {perm:?}");

        let att_count = mid.cells.iter().filter(|&&c| c == ATT).count();
        let rep_count = mid.cells.iter().filter(|&&c| c == REP).count();
        assert_eq!(att_count, 2);
        assert_eq!(rep_count, 1);

        match outcome {
            RoundOutcome::Resolved { board: after, .. } => {
                assert_eq!(after.cell_at((0, 1)), ATT);
                assert_eq!(after.cell_at((0, 0)), REP);
                assert_eq!(after.cell_at((2, 0)), ATT);
                assert_eq!(after.cell_at((1, 0)), VAC);
            }
            other => panic!("expected Resolved, got {other:?}"),
        }

        // Since all 3 pieces backed up to their starting squares, no piece changed square,
        // so `compute_landed_squares` must be empty `[]`:
        let landed = oracle::compute_landed_squares(&cfg_bunch, &b_cat, &[], perm);
        assert!(landed.is_empty(), "expected empty landed_squares when all 3 back up, got {landed:?}");

        // Also verify `eval_wire_canonical` matches `automatafl_lean::rules_wire` on `roundcfg` & `midcfg`:
        let midcfg_q = format!(
            "midcfg {} {} 0 {}",
            cfg_bunch.to_wire(),
            b_cat.to_wire(),
            automatafl_server::protocol::moves_wire(perm),
        );
        assert_eq!(
            oracle::call_wire(&midcfg_q).unwrap(),
            oracle::eval_wire_canonical(&midcfg_q)
        );
        let roundcfg_q = format!(
            "roundcfg {} 0 {} {} 0 0 3 0 1 2 0 {}",
            cfg_bunch.to_wire(),
            b_cat.to_wire(),
            automatafl_server::protocol::goals_wire(&goals5),
            automatafl_server::protocol::moves_wire(perm),
        );
        assert_eq!(
            oracle::call_wire(&roundcfg_q).unwrap(),
            oracle::eval_wire_canonical(&roundcfg_q)
        );
    }

    // 3. Multi-cell slide backing up along `reverseEdgeInterior`:
    //    Piece 0 at (0, 0) slides to (3, 0), Piece 1 at (3, 2) slides to (3, 0) (colliding at (3, 0)),
    //    and Piece 2 at (2, 2) moves to (2, 0) (colliding with Piece 0's 1-short fallback at (2, 0)).
    //    Synchronous relaxation backs Piece 0 up past (2, 0) to (1, 0), backs Piece 2 up 1 cell to (2, 1),
    //    and stops Piece 1 at (3, 1)!
    let mut slide_cells = vec![VAC; 25];
    slide_cells[4 * 5 + 4] = AUTO;
    slide_cells[0 * 5 + 0] = ATT; // (0, 0)
    slide_cells[2 * 5 + 3] = REP; // (3, 2)
    slide_cells[2 * 5 + 2] = ATT; // (2, 2)
    let b_slide = Board {
        n: 5,
        cells: slide_cells,
        auto: (4, 4),
        col_rule: true,
    };
    let s1 = Move { who: 0, frm: (0, 0), to: (3, 0) };
    let s2 = Move { who: 1, frm: (3, 2), to: (3, 0) };
    let s3 = Move { who: 2, frm: (2, 2), to: (2, 0) };
    let slide_perms = [
        [s1, s2, s3],
        [s1, s3, s2],
        [s2, s1, s3],
        [s2, s3, s1],
        [s3, s1, s2],
        [s3, s2, s1],
    ];
    for perm in &slide_perms {
        let (_, mid) = oracle::round_with_config(
            &cfg_bunch,
            None,
            &b_slide,
            &goals5,
            &[],
            &[],
            &[0, 1, 2],
            perm,
        )
        .unwrap();
        assert_eq!(mid.cell_at((1, 0)), ATT, "slider from (0,0) backs up 2 cells to (1,0)");
        assert_eq!(mid.cell_at((2, 1)), ATT, "mover from (2,2) backs up 1 cell to (2,1)");
        assert_eq!(mid.cell_at((3, 1)), REP, "slider from (3,2) stops 1 short at (3,1)");
        assert_eq!(mid.cell_at((3, 0)), VAC, "contested (3,0) remains empty");
        assert_eq!(mid.cell_at((2, 0)), VAC, "secondary contested (2,0) remains empty");

        let mut landed = oracle::compute_landed_squares(&cfg_bunch, &b_slide, &[], perm);
        landed.sort();
        assert_eq!(landed, vec![(1, 0), (2, 1), (3, 1)]);
    }
}

#[test]
fn bunch_before_two_cycle_swaps_multi_turn_cooldowns_and_wire_dedup_parity() {
    // 1. Multi-chain `BunchBefore` caterpillar collision combined with 2-cycle swaps (`two_cycle_swaps = true`):
    //    - Caterpillar on row 0/1: m1 (0,1)->(0,0), m2 (0,0)->(1,0), m3 (2,0)->(1,0) -> all 3 back up!
    //    - 2-cycle swap on row 3:  s1 (3,3)->(4,3), s2 (4,3)->(3,3) -> swap places!
    let mut cells = vec![VAC; 25];
    cells[4 * 5 + 4] = AUTO; // Automaton at (4, 4)
    cells[1 * 5 + 0] = ATT;  // (0, 1)
    cells[0 * 5 + 0] = REP;  // (0, 0)
    cells[0 * 5 + 2] = ATT;  // (2, 0)
    cells[3 * 5 + 3] = ATT;  // (3, 3)
    cells[3 * 5 + 4] = REP;  // (4, 3)
    let b_combo = Board {
        n: 5,
        cells,
        auto: (4, 4),
        col_rule: true,
    };
    let goals5 = oracle::stock_goals2(5).unwrap();
    let mut cfg_combo = GameConfig::default();
    cfg_combo.preset = BoardPreset::Mini5x5;
    cfg_combo.confluence = ConfluenceMode::BunchBefore;
    cfg_combo.two_cycle_swaps = true;
    cfg_combo.cooldown_turns = 2;

    let moves_combo = [
        Move { who: 0, frm: (0, 1), to: (0, 0) },
        Move { who: 1, frm: (0, 0), to: (1, 0) },
        Move { who: 2, frm: (2, 0), to: (1, 0) },
        Move { who: 3, frm: (3, 3), to: (4, 3) },
        Move { who: 4, frm: (4, 3), to: (3, 3) },
    ];
    let (_, mid_combo) = oracle::round_with_config(
        &cfg_combo,
        None,
        &b_combo,
        &goals5,
        &[],
        &[],
        &[0, 1, 2, 3, 4],
        &moves_combo,
    )
    .unwrap();
    // Caterpillar pieces stay at their origins; (1,0) stays VAC:
    assert_eq!(mid_combo.cell_at((0, 1)), ATT);
    assert_eq!(mid_combo.cell_at((0, 0)), REP);
    assert_eq!(mid_combo.cell_at((2, 0)), ATT);
    assert_eq!(mid_combo.cell_at((1, 0)), VAC);
    // 2-cycle pieces swapped places:
    assert_eq!(mid_combo.cell_at((3, 3)), REP);
    assert_eq!(mid_combo.cell_at((4, 3)), ATT);
    // Only the 2 swapped squares (3,3) and (4,3) are recorded in `landed_squares`:
    let mut landed_combo = oracle::compute_landed_squares(&cfg_combo, &b_combo, &[], &moves_combo);
    landed_combo.sort();
    assert_eq!(landed_combo, vec![(3, 3), (4, 3)]);

    // 2. GameSession multi-turn cooldowns (`cooldown_turns = 2`) when a turn resolves with 0 landed squares:
    //    On Turn 0, two pieces collide at (1, 0) from adjacent squares in `BunchBefore` mode:
    //    Seat A: (0, 0) -> (1, 0), Seat B: (2, 0) -> (1, 0).
    //    Both back up to their origins (0, 0) and (2, 0), so `landed_squares` is EMPTY `[]`!
    //    Verify `session.active_cooldown_squares()` is EMPTY (and does NOT falsely mark (1, 0) or (0, 0) on cooldown).
    let mut sess_cd = GameSession::new(
        "af-bunch-cd".to_string(),
        "Bunch CD".to_string(),
        true,
        cfg_combo.clone(),
        123,
        0,
    )
    .unwrap();
    sess_cd.board = b_combo.clone();
    sess_cd.start_board = b_combo.clone();
    // Turn 0: Seat A submits (0, 1) -> (0, 0) (blocked by stationary REP at (0, 0)),
    //         Seat B submits (2, 0) -> (0, 0) (blocked by stationary REP at (0, 0)).
    // Neither piece moves (`landed_squares == []`), and (0, 0) still holds REP on `post_board`.
    sess_cd.submit_move(Seat::A, (0, 1), (0, 0)).unwrap();
    sess_cd.submit_move(Seat::B, (2, 0), (0, 0)).unwrap();
    assert_eq!(sess_cd.turn_no, 1);
    assert!(sess_cd.turns_played[0].landed_squares.is_empty());
    assert!(
        sess_cd.active_cooldown_squares().is_empty(),
        "0-mover turn must NOT falsely put destination (0, 0) on cooldown"
    );
    assert!(!sess_cd.is_on_cooldown((0, 0)));

    // Turn 1: Seat A and Seat B execute a 2-cycle swap (3, 3) <-> (4, 3) with `cooldown_turns = 2`:
    sess_cd.submit_move(Seat::A, (3, 3), (4, 3)).unwrap();
    sess_cd.submit_move(Seat::B, (4, 3), (3, 3)).unwrap();
    assert_eq!(sess_cd.turn_no, 2);
    assert!(sess_cd.is_on_cooldown((3, 3)));
    assert!(sess_cd.is_on_cooldown((4, 3)));

    // Turn 2: Another 0-mover turn ((0, 1) -> (0, 0) and (2, 0) -> (0, 0)).
    // Because `cooldown_turns == 2`, (3, 3) and (4, 3) from Turn 1 must STILL be on cooldown in Turn 3,
    // while (0, 0) must still NOT be on cooldown!
    sess_cd.submit_move(Seat::A, (0, 1), (0, 0)).unwrap();
    sess_cd.submit_move(Seat::B, (2, 0), (0, 0)).unwrap();
    assert_eq!(sess_cd.turn_no, 3);
    assert!(sess_cd.is_on_cooldown((3, 3)));
    assert!(sess_cd.is_on_cooldown((4, 3)));
    assert!(!sess_cd.is_on_cooldown((0, 0)));

    // 3. `TieBreakMode::Momentum` preserves `last_auto_delta` across stationary turns and passes `verify_replay`:
    let mut cfg_mom = GameConfig::default();
    cfg_mom.preset = BoardPreset::Mini5x5;
    cfg_mom.tie_break = TieBreakMode::Momentum {
        prefer_same_axis: true,
    };
    let mut sess_mom = GameSession::new(
        "af-mom".to_string(),
        "Momentum".to_string(),
        true,
        cfg_mom,
        456,
        0,
    )
    .unwrap();
    // Turn 0 on Mini5x5: automaton at (2, 2) steps left to (1, 2) toward lone attractor at (0, 2):
    sess_mom.submit_move(Seat::A, (0, 1), (0, 2)).unwrap();
    sess_mom.submit_move(Seat::B, (4, 3), (4, 1)).unwrap();
    let delta_after_t0 = sess_mom.last_auto_delta();
    assert_eq!(delta_after_t0, Some((-1, 0)), "expected automaton to step left on turn 0");
    // Force a stationary automaton turn on Turn 1 (clear both row 2 and column 1) and verify `last_auto_delta` is preserved:
    sess_mom.submit_move(Seat::A, (0, 2), (0, 1)).unwrap();
    sess_mom.submit_move(Seat::B, (1, 0), (2, 0)).unwrap();
    assert_eq!(sess_mom.board.auto, (1, 2));
    assert_eq!(sess_mom.last_auto_delta(), Some((-1, 0)));
    assert_eq!(sess_mom.verify_replay().unwrap(), 2);

    // 4. Last-occurrence `List.dedup` and edge-case wire parity (`clash`, `round`, `goals 0`, malformed wire):
    //    Fork at (1, 0) and fork at (0, 0) where (1, 0) also appears as a destination after (0, 0):
    //    m1: (1,0)->(2,0), m2: (1,0)->(1,1), m3: (0,0)->(1,0), m4: (0,0)->(0,1).
    let clash_dedup_q = format!(
        "clash {} 0 4 0 1 0 2 0 1 1 0 1 1 0 0 0 1 0 1 0 0 0 1",
        b_combo.to_wire()
    );
    let clash_via_lean = automatafl_lean::rules_wire(&clash_dedup_q);
    let clash_via_canonical = oracle::eval_wire_canonical(&clash_dedup_q);
    assert_eq!(clash_via_lean, "1 2 0 0 1 0");
    assert_eq!(clash_via_canonical, clash_via_lean);

    // Waiting PIDs last-occurrence dedup (`[0, 1, 0].dedup == [1, 0]`):
    let round_dedup_q = format!(
        "round 0 {} {} 0 0 2 0 1 3 0 0 0 1 0 1 0 0 0 1 0 0 0 2 0",
        b_combo.to_wire(),
        automatafl_server::protocol::goals_wire(&goals5)
    );
    assert_eq!(
        oracle::eval_wire_canonical(&round_dedup_q),
        automatafl_lean::rules_wire(&round_dedup_q)
    );

    // `goals 0` and negative/non-numeric board tokens parity:
    for edge_q in [
        "goals 0",
        "step 0 3 000030000 -1 1 1",
        "step 0 3 000030000 1 1 abc",
    ] {
        assert_eq!(
            oracle::eval_wire_canonical(edge_q),
            automatafl_lean::rules_wire(edge_q),
            "edge wire parity mismatch on {edge_q:?}"
        );
    }
}

#[tokio::test]
async fn bounded_ram_lru_eviction_and_transparent_disk_lazy_loading() {
    let dir = tempfile::tempdir().unwrap();
    let store = GameStore::disk(dir.path()).unwrap();
    // Cap in-memory games at 2!
    let state = AppState::with_max_in_memory_games(store, 2).unwrap();
    let app = build_router(Arc::clone(&state));

    // Create Game 1 and play a move against Tactical AI bot so it has 1 completed turn on disk.
    let (_, c1_val) = send_json(
        &app,
        "POST",
        "/api/games",
        None,
        serde_json::json!({
            "config": { "preset": "mini5x5" },
            "vs_bot": "tactical",
            "creator_seat": "A"
        }),
    )
    .await;
    let c1: CreateGameResponse = serde_json::from_value(c1_val).unwrap();
    let g1_id = c1.game_id;
    let g1_tok = c1.creator_token.unwrap();

    let (st_mv, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{g1_id}/move"),
        Some(&g1_tok),
        serde_json::json!({ "from": [0, 1], "to": [0, 2] }),
    )
    .await;
    assert_eq!(st_mv, StatusCode::OK);
    assert_eq!(state.in_memory_game_count(), 1);

    // Create Game 2 and resign it so `ended == true` (preferred eviction candidate).
    let (_, c2_val) = send_json(
        &app,
        "POST",
        "/api/games",
        None,
        serde_json::json!({ "creator_seat": "A" }),
    )
    .await;
    let c2: CreateGameResponse = serde_json::from_value(c2_val).unwrap();
    let g2_id = c2.game_id;
    let g2_tok = c2.creator_token.unwrap();
    let (st_resign, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{g2_id}/resign"),
        Some(&g2_tok),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(st_resign, StatusCode::OK);
    assert_eq!(state.in_memory_game_count(), 2);

    // Create Game 3 and Game 4: RAM cap stays strictly <= 2, evicting Game 2 and Game 1 from RAM!
    for _ in 0..2 {
        let (st, _) = send_json(
            &app,
            "POST",
            "/api/games",
            None,
            serde_json::json!({ "creator_seat": "A" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert!(state.in_memory_game_count() <= 2);
    }
    assert_eq!(state.in_memory_game_count(), 2);

    // Now request evicted Game 1 via `GET /api/games/{g1_id}` and `/replay`:
    // Server must transparently lazy-load Game 1 from disk while keeping RAM bounded at 2!
    let (st_get, g1_raw) = get_json(&app, &format!("/api/games/{g1_id}"), Some(&g1_tok)).await;
    assert_eq!(st_get, StatusCode::OK);
    let g1_view: GameView = serde_json::from_value(g1_raw).unwrap();
    assert_eq!(g1_view.id, g1_id);
    assert_eq!(g1_view.turn_no, 1);
    assert_eq!(g1_view.turn_history.len(), 1);
    assert_eq!(state.in_memory_game_count(), 2);

    let (st_rep, rep_raw) = get_json(&app, &format!("/api/games/{g1_id}/replay"), None).await;
    assert_eq!(st_rep, StatusCode::OK);
    let rep: ReplayExport = serde_json::from_value(rep_raw).unwrap();
    assert!(rep.replay_verified);
    assert_eq!(rep.turns_played.len(), 1);
}

#[tokio::test]
async fn compact_wire_payload_omits_cells_and_exposes_cooldown_squares_and_turn_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = GameStore::disk(dir.path()).unwrap();
    let state = AppState::new(store).unwrap();
    let app = build_router(Arc::clone(&state));

    let (_, create_val) = send_json(
        &app,
        "POST",
        "/api/games",
        None,
        serde_json::json!({
            "config": {
                "preset": "classic11x11",
                "cooldown_turns": 2
            },
            "creator_seat": "A"
        }),
    )
    .await;
    let created: CreateGameResponse = serde_json::from_value(create_val).unwrap();
    let game_id = created.game_id;
    let token_a = created.creator_token.unwrap();
    let (_, join_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/join"),
        None,
        serde_json::json!({ "player_name": "Bob" }),
    )
    .await;
    let joined: JoinGameResponse = serde_json::from_value(join_val).unwrap();
    let token_b = joined.seat_token;

    // Verify `POST /select` does NOT write `sel` to disk snapshot!
    let (st_sel, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/select"),
        Some(&token_a),
        serde_json::json!({ "coord": [3, 1] }),
    )
    .await;
    assert_eq!(st_sel, StatusCode::OK);
    let disk_snapshot_bytes = std::fs::read_to_string(dir.path().join(format!("{game_id}.json"))).unwrap();
    let disk_session: GameSession = serde_json::from_str(&disk_snapshot_bytes).unwrap();
    assert_eq!(
        disk_session.sel,
        [None, None],
        "select_piece must not write ephemeral selection to disk"
    );

    // Play Turn 0: Seat A moves (3,1)->(3,2), Seat B moves (7,9)->(7,8)
    send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_a),
        serde_json::json!({ "from": [3, 1], "to": [3, 2] }),
    )
    .await;
    send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_b),
        serde_json::json!({ "from": [7, 9], "to": [7, 8] }),
    )
    .await;

    // Fetch raw JSON over HTTP:
    let (st_raw, raw_val) = get_json(&app, &format!("/api/games/{game_id}"), Some(&token_a)).await;
    assert_eq!(st_raw, StatusCode::OK);

    // 1. `cells` MUST be omitted on the wire (`skip_serializing`) to save ~14 KB per payload!
    assert!(
        raw_val.get("cells").is_none(),
        "wire JSON must omit redundant `cells` array"
    );

    // 2. `cooldown_squares` and `turn_history` MUST be present on the wire:
    let cd_arr = raw_val.get("cooldown_squares").and_then(|v| v.as_array()).unwrap();
    assert_eq!(cd_arr.len(), 2);
    let hist_arr = raw_val.get("turn_history").and_then(|v| v.as_array()).unwrap();
    assert_eq!(hist_arr.len(), 1);

    // 3. Deserializing `GameView` in Rust automatically populates `view.cells` via `derive_cells()`:
    let view: GameView = serde_json::from_value(raw_val).unwrap();
    assert_eq!(view.cells.len(), 121);
    assert!(view.cooldown_squares.contains(&(3, 2)));
    assert!(view.cooldown_squares.contains(&(7, 8)));
    let cell_3_2 = &view.cells[2 * 11 + 3];
    assert!(cell_3_2.is_on_cooldown);
    assert!(!cell_3_2.is_selectable);
}

#[tokio::test]
async fn matchmaking_prunes_completed_tickets_and_web_ui_contains_offloading_features() {
    use automatafl_server::matchmaking::{MatchmakingQueue, SeekRequest};

    let mut mm = MatchmakingQueue::with_max_completed_tickets(3);
    for i in 0..8 {
        let (t_id, _) = mm
            .enqueue(SeekRequest {
                player_name: Some(format!("P{i}")),
                config: GameConfig::default(),
                any_variant_with_same_board: true,
            })
            .unwrap();
        mm.cancel(&t_id);
        assert!(
            mm.completed_count() <= 3,
            "completed tickets must be pruned to cap 3, got {}",
            mm.completed_count()
        );
    }

    // Verify `GET /` serves the upgraded single-page UI with client-side target/sightline/forecast
    // calculation, Web Worker AI offloading, cooldown badging, automata_count configurator,
    // turn history log, and Rematch button.
    let state = AppState::new_in_memory();
    let app = build_router(state);
    let resp = app
        .oneshot(Request::builder().method("GET").uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(html_bytes.to_vec()).unwrap();

    for expected_marker in [
        "computeTargetsClient",
        "computeSightlinesClient",
        "ownMoveForecastClient",
        "chooseBotMoveClientSync",
        "chooseBotMoveClientAsync",
        "client_local",
        "cfg-automata",
        "history-log",
        "btn-rematch",
        "cooldown-badge",
    ] {
        assert!(
            html.contains(expected_marker),
            "web_ui.rs INDEX_HTML missing expected feature marker: {expected_marker}"
        );
    }
}

#[tokio::test]
async fn web_ui_embedded_javascript_runtime_renders_server_view_and_matches_oracle_rules() {
    // 1. Create a real server-hosted game with `cooldown_turns = 2` and resolve Turn 0
    //    so we have an authentic wire `GameView` JSON payload with `turn_history`, `last_step`,
    //    `cooldown_squares`, and `sightlines`.
    let state = AppState::new_in_memory();
    let app = build_router(Arc::clone(&state));

    let (_, create_val) = send_json(
        &app,
        "POST",
        "/api/games",
        None,
        serde_json::json!({
            "config": {
                "preset": "classic11x11",
                "cooldown_turns": 2
            },
            "creator_seat": "A"
        }),
    )
    .await;
    let created: CreateGameResponse = serde_json::from_value(create_val).unwrap();
    let game_id = created.game_id;
    let token_a = created.creator_token.unwrap();
    let (_, join_val) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/join"),
        None,
        serde_json::json!({ "player_name": "Bob" }),
    )
    .await;
    let joined: JoinGameResponse = serde_json::from_value(join_val).unwrap();
    let token_b = joined.seat_token;

    send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_a),
        serde_json::json!({ "from": [3, 1], "to": [3, 2] }),
    )
    .await;
    send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_b),
        serde_json::json!({ "from": [7, 9], "to": [7, 8] }),
    )
    .await;

    let (st_view, server_view_json) =
        get_json(&app, &format!("/api/games/{game_id}"), Some(&token_a)).await;
    assert_eq!(st_view, StatusCode::OK);

    // 2. Fetch `GET /` HTML and extract the embedded `<script>...</script>` code:
    let resp = app
        .oneshot(Request::builder().method("GET").uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(html_bytes.to_vec()).unwrap();
    let script_start = html.find("<script>").expect("<script> tag") + "<script>".len();
    let script_end = html.rfind("</script>").expect("</script> tag");
    let script_body = &html[script_start..script_end];

    // 3. Execute the embedded JS in Node.js with a lightweight DOM harness and verify:
    //    - `renderGameView` & `renderBoard` on real server `GameView` JSON after Turn 1
    //    - `.sightline`, `.cooldown`, `.last-move-src`, `.last-move-dst`, `.last-auto-from`, `.history-entry`
    //    - 0ms zero-network piece selection, cooldown rejection, target calculation, and hover forecast
    //    - ClientEngine parity with `oracle.rs` on adjacent Attractor (`dist == 1`), decision priority
    //      (`variant 3 > 2 > 1`), `railgun_fling`, `automata_count = 2`, and `bunch_before` caterpillar chains
    //    - Full local AI turn resolution (`createLocalBotMatch` + `submitLocalMove`)
    let harness = format!(
        r#"
function assert(cond, msg) {{
  if (!cond) throw new Error(msg || 'Assertion failed');
}}
function assertEq(a, b, msg) {{
  if (JSON.stringify(a) !== JSON.stringify(b)) {{
    throw new Error((msg || 'Mismatch') + ': got ' + JSON.stringify(a) + ', expected ' + JSON.stringify(b));
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
globalThis.document = {{
  getElementById: getEl
}};
globalThis.localStorage = {{
  store: {{}},
  getItem(k) {{ return this.store[k] || null; }},
  setItem(k, v) {{ this.store[k] = String(v); }}
}};
let fetchCalls = 0;
globalThis.fetch = async () => {{ fetchCalls++; return {{ ok: true, json: async () => [] }}; }};
globalThis.EventSource = function() {{ this.close = () => {{}}; }};
globalThis.setInterval = () => 0;
globalThis.clearInterval = () => {{}};

{script_body}

(async () => {{
  const serverView = {server_view};
  // 1. Render real server GameView after Turn 1 (must not throw TypeError):
  renderGameView(serverView);
  const histHtml = getEl('history-log').innerHTML;
  assert(histHtml.includes('history-entry'), 'history-log must contain .history-entry: ' + histHtml);
  assert(histHtml.includes('T1:'), 'history-log must display T1: ' + histHtml);
  assert(histHtml.includes('(3,1)→(3,2)'), 'history-log must show Seat A move: ' + histHtml);
  assert(histHtml.includes('(7,9)→(7,8)'), 'history-log must show Seat B move: ' + histHtml);

  const gridHtml = getEl('board-grid').innerHTML;
  assert(gridHtml.includes('sightline'), 'board-grid must highlight .sightline on server GameView');
  assert(gridHtml.includes('cooldown'), 'board-grid must highlight .cooldown squares');
  assert(gridHtml.includes('❄️'), 'board-grid must render ❄️ cooldown badge');
  assert(gridHtml.includes('last-move-src'), 'board-grid must highlight .last-move-src');
  assert(gridHtml.includes('last-move-dst'), 'board-grid must highlight .last-move-dst');
  assert(gridHtml.includes('last-auto-from'), 'board-grid must highlight .last-auto-from');

  // 2. Clicking cooled-down square (3, 2) must be rejected in 0ms:
  const fetchBefore = fetchCalls;
  await onSquareClick(3, 2);
  assertEq(localSelectedCoord, null, 'cooled-down square (3,2) must not be selectable');
  assertEq(fetchCalls, fetchBefore, 'clicking square must not trigger network fetch');

  // 3. Clicking active piece (4, 0) selects it in 0ms and highlights targets + hover forecast:
  await onSquareClick(4, 0);
  assertEq(localSelectedCoord, [4, 0], 'selecting (4,0)');
  assertEq(fetchCalls, fetchBefore, 'selecting piece must be 0ms zero-network');
  const gridSelected = getEl('board-grid').innerHTML;
  assert(gridSelected.includes('selected'), 'selected piece must have .selected class');
  assert(gridSelected.includes('target-exec'), 'executable targets must have .target-exec class');
  onSquareHover(4, 2);
  assert(getEl('own-forecast').innerHTML.includes('Client Forecast (4,0)→(4,2)'), 'hover must render live forecast');

  // 4. Rule parity — Adjacent Attractor (dist == 1) must NOT pull @ onto itself:
  const bAdjAtt = {{
    n: 5,
    cells: [
      0,0,0,0,0,
      0,0,2,0,0, // ATT at (2, 1), dist 1 North of @ at (2, 2)
      0,0,3,0,0, // AUTO at (2, 2)
      0,0,0,0,0,
      0,0,0,0,0
    ],
    auto: [2, 2],
    col_rule: true
  }};
  const adjSense = senseAtOriginClient(bAdjAtt, [2, 2], {{ tie_break: {{ mode: 'column' }} }}, null);
  assertEq(adjSense.offset, [0, 0], 'adjacent Attractor at dist 1 must not pull @');
  const adjStepped = stepAutomataClient(bAdjAtt, {{ tie_break: {{ mode: 'column' }} }}, stockGoalsClient(5), null);
  assertEq(adjStepped.board.auto, [2, 2], '@ must remain at (2,2)');
  assertEq(cellAt(adjStepped.board, 2, 1), ATT, 'Attractor at (2,1) must not be overwritten');

  // 5. Rule parity — Decision variant priority (variant 2 fromRepulsor beats variant 1 towardAttractor even if ATT is closer):
  const bPrio = {{
    n: 7,
    cells: new Array(49).fill(0),
    auto: [3, 3],
    col_rule: true
  }};
  bPrio.cells[3 * 7 + 3] = AUTO;
  bPrio.cells[1 * 7 + 3] = ATT; // North at (3, 1), dist 2 -> variant 1 (towardAttractor)
  bPrio.cells[3 * 7 + 0] = REP; // West at (0, 3), dist 3 -> variant 2 (fromRepulsor East)
  const prioSense = senseAtOriginClient(bPrio, [3, 3], {{ tie_break: {{ mode: 'column' }} }}, null);
  assertEq(prioSense.offset, [1, 0], 'variant 2 (fromRepulsor dist 3) must beat variant 1 (towardAttractor dist 2)');

  // 6. Rule parity — railgun_fling slides @ across vacuum and does not fling regular pieces:
  const bRail = {{
    n: 5,
    cells: [
      2,0,0,0,0, // ATT at (0, 0)
      0,0,0,0,0,
      1,3,0,0,0, // REP at (0, 2), AUTO at (1, 2)
      0,0,0,0,0,
      0,0,0,0,0
    ],
    auto: [1, 2],
    col_rule: true
  }};
  const cfgRail = {{ preset: 'mini5x5', tie_break: {{ mode: 'column' }}, confluence: 'conflict', railgun_fling: true, automata_count: 1 }};
  const midRail = resolveMidClient(bRail, [], [{{ who: 0, frm: [0, 0], to: [0, 1] }}], cfgRail);
  assertEq(cellAt(midRail, 0, 1), ATT, 'regular piece stops at (0,1)');
  assertEq(cellAt(midRail, 0, 4), VAC, 'regular piece is not flung to (0,4)');
  const stepRail = stepAutomataClient(midRail, cfgRail, stockGoalsClient(5), null);
  assertEq(stepRail.board.auto, [4, 2], 'railgun_fling slides @ from (1,2) to (4,2)');

  // 7. Rule parity — automata_count = 2 steps @ twice in one round:
  const cfgMulti = {{ preset: 'mini5x5', tie_break: {{ mode: 'column' }}, confluence: 'conflict', railgun_fling: false, automata_count: 2 }};
  const stepMulti = stepAutomataClient(bRail, cfgMulti, stockGoalsClient(5), null);
  assertEq(stepMulti.board.auto, [3, 2], 'automata_count=2 steps @ twice from (1,2) to (3,2)');

  // 8. Rule parity — bunch_before 3-move caterpillar collision conserves all 3 pieces:
  const bCat = {{
    n: 5,
    cells: [
      1,0,2,0,0, // REP at (0,0), ATT at (2,0)
      2,0,0,0,0, // ATT at (0,1)
      0,0,0,0,0,
      0,0,0,0,0,
      0,0,0,0,3  // AUTO at (4,4)
    ],
    auto: [4, 4],
    col_rule: true
  }};
  const cfgBunch = {{ preset: 'mini5x5', tie_break: {{ mode: 'column' }}, confluence: 'bunch_before', two_cycle_swaps: false }};
  const catMoves = [
    {{ who: 0, frm: [0, 1], to: [0, 0] }},
    {{ who: 1, frm: [0, 0], to: [1, 0] }},
    {{ who: 2, frm: [2, 0], to: [1, 0] }}
  ];
  const midCat = resolveMidClient(bCat, [], catMoves, cfgBunch);
  assertEq(cellAt(midCat, 0, 1), ATT, 'm1 piece at (0,1) conserved');
  assertEq(cellAt(midCat, 0, 0), REP, 'm2 piece at (0,0) conserved');
  assertEq(cellAt(midCat, 2, 0), ATT, 'm3 piece at (2,0) conserved');
  assertEq(cellAt(midCat, 1, 0), VAC, 'contested (1,0) stays VAC');
  assertEq(landedCoordsCfgClient(cfgBunch, bCat, [], catMoves), [], 'landed squares empty when all 3 back up');

  // 9. Local AI Match full turn resolution in browser (zero network calls):
  currentGameId = 'local-ai';
  createLocalBotMatch(
    {{ preset: 'mini5x5', tie_break: {{ mode: 'column' }}, confluence: 'bunch_before', two_cycle_swaps: true, cooldown_turns: 1, railgun_fling: false, automata_count: 1 }},
    'tactical',
    'A'
  );
  await submitLocalMove([0, 1], [0, 2]);
  assertEq(localMatchState.turn_no, 1, 'local match must complete Turn 1');
  assertEq(localMatchState.turn_history.length, 1, 'local match must record TurnSummary');
  assert(getEl('history-log').innerHTML.includes('T1:'), 'history-log must render local Turn 1');
  assertEq(fetchCalls, fetchBefore, 'local AI match must make zero HTTP requests');
  if (typeof process !== 'undefined' && process.exit) process.exit(0);
}})();
"#,
        script_body = script_body,
        server_view = serde_json::to_string(&server_view_json).unwrap()
    );

    let dir = tempfile::tempdir().unwrap();
    let script_path = dir.path().join("harness.js");
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
        "{js_runtime} runtime test of web_ui.rs failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn telemetry_status_dashboard_metrics_and_fog_of_war_redaction() {
    let app = build_router(AppState::new_in_memory());

    // 1. Verify `/status` and `/lag` serve the Lichess-style dashboard view:
    for route in ["/status", "/lag"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(route)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("id=\"status-view\""));
        assert!(html.contains("System Load, Network Lag"));
    }

    // 2. Create a 2-player game and seal Seat A's move `(3,1) -> (3,4)` under fog of war:
    let (st_create, created) = send_json(
        &app,
        "POST",
        "/api/games",
        None,
        serde_json::json!({
            "name": "Telemetry Fog Check",
            "creator_seat": "A",
            "player_name": "Alice"
        }),
    )
    .await;
    assert_eq!(st_create, StatusCode::OK);
    let game_id = created["game_id"].as_str().unwrap().to_string();
    let token_a = created["creator_token"].as_str().unwrap().to_string();
    let token_b = created["open_invite_token"].as_str().unwrap().to_string();

    let (st_mv_a, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_a),
        serde_json::json!({ "from": [3, 1], "to": [3, 4] }),
    )
    .await;
    assert_eq!(st_mv_a, StatusCode::OK);

    // 3. Query `/api/status` while Seat A's move is sealed & unrevealed:
    let (st_status1, status_json1) = get_json(&app, "/api/status", None).await;
    assert_eq!(st_status1, StatusCode::OK);
    assert!(status_json1["lean_ffi_calls_total"].as_u64().unwrap() > 0);
    assert_eq!(status_json1["lean_ffi_errors_total"].as_u64().unwrap(), 0);
    assert!(status_json1["games"]["active_games"].as_u64().unwrap() >= 1);

    // Verify fog-of-war redaction in `recent_events` — must NOT contain `(3,1)` or `(3,4)`:
    let traces1 = status_json1["recent_events"].as_array().unwrap();
    let game_traces: Vec<&serde_json::Value> = traces1
        .iter()
        .filter(|t| t["game_id"].as_str() == Some(game_id.as_str()))
        .collect();
    assert!(
        game_traces.iter().any(|t| t["kind"] == "ROUND_SEAL"),
        "expected ROUND_SEAL trace for {game_id}"
    );
    for t in &game_traces {
        let summary = t["summary"].as_str().unwrap_or("");
        assert!(
            !summary.contains("(3,1)") && !summary.contains("(3,4)") && !summary.contains("[3,1]"),
            "Fog-of-war violation: sealed trace leaked secret coordinates: {summary}"
        );
    }

    // 4. Seal Seat B's move `(7,9) -> (7,8)` to resolve Turn 1:
    let (st_mv_b, _) = send_json(
        &app,
        "POST",
        &format!("/api/games/{game_id}/move"),
        Some(&token_b),
        serde_json::json!({ "from": [7, 9], "to": [7, 8] }),
    )
    .await;
    assert_eq!(st_mv_b, StatusCode::OK);

    let (st_status2, status_json2) = get_json(&app, "/api/status", None).await;
    assert_eq!(st_status2, StatusCode::OK);
    assert!(status_json2["games"]["turns_resolved_total"].as_u64().unwrap() >= 1);
    let traces2 = status_json2["recent_events"].as_array().unwrap();
    assert!(
        traces2
            .iter()
            .any(|t| t["game_id"].as_str() == Some(game_id.as_str()) && t["kind"] == "TURN_RESOLVE"),
        "expected TURN_RESOLVE trace after both seats sealed"
    );

    // 5. Verify `/metrics` Prometheus exposition endpoint:
    let resp_metrics = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_metrics.status(), StatusCode::OK);
    let metrics_bytes = to_bytes(resp_metrics.into_body(), usize::MAX).await.unwrap();
    let metrics_text = String::from_utf8(metrics_bytes.to_vec()).unwrap();
    assert!(metrics_text.contains("automatafl_http_requests_total"));
    assert!(metrics_text.contains("automatafl_lean_ffi_calls_total "));
    assert!(metrics_text.contains("automatafl_turns_resolved_total "));
    assert!(metrics_text.contains("automatafl_memory_current_bytes "));
    assert!(metrics_text.contains("automatafl_disk_used_bytes "));

    // Also verify `/metrics` does not leak sealed move coordinates:
    assert!(!metrics_text.contains("(3,4)"));
}


