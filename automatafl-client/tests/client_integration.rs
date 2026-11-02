use automatafl_client::app::{ClientApp, ClientMode, KeyCommand};
use automatafl_client::remote::RemoteClient;
use automatafl_client::tui::{render_frame_ansi, render_frame_plain};
use automatafl_server::api::{build_router, AppState};
use automatafl_server::bot::BotStrategy;
use automatafl_server::matchmaking::{SeekRequest, SeekStatus};
use automatafl_server::protocol::{BoardPreset, GameConfig, Seat};

#[test]
fn tui_renders_boards_sightlines_targets_variants_and_fog_of_war() {
    let mut app = ClientApp::new(ClientMode::LocalPassAndPlay, GameConfig::default()).unwrap();

    // Move cursor to (3, 1) [Attractor on 11x11 Classic] and select it.
    app.cursor = (3, 1);
    app.handle_key(KeyCommand::ConfirmAtCursor).unwrap();

    let frame_select = app.render_plain();
    assert!(frame_select.contains("11×11 Classic"));
    assert!(frame_select.contains("[A]")); // selected Attractor at (3, 1)
    assert!(frame_select.contains(" + ")); // executable rook-line targets
    assert!(frame_select.contains(" ? ")); // occluded proposable targets across other pieces
    assert!(frame_select.contains("SELECTING from (3,1)"));

    // Move cursor to (3, 2) and seal the move for Seat A.
    app.cursor = (3, 2);
    app.handle_key(KeyCommand::ConfirmAtCursor).unwrap();

    // In LocalPassAndPlay mode, sealing Seat A's move automatically rotates viewer to Seat B!
    assert_eq!(app.viewer, Some(Seat::B));
    let frame_seat_b = app.render_plain();

    // Seat B's rendered TUI MUST show Seat A is sealed under fog of war and MUST NOT leak (3,1) -> (3,2)!
    assert!(frame_seat_b.contains("Hidden by Fog of War"));
    assert!(!frame_seat_b.contains("(3,1) -> (3,2)"));

    // Now Seat B intentionally causes a fork clash at (3, 1) by selecting (3, 1) and moving to (3, 3)!
    app.cursor = (3, 1);
    app.handle_key(KeyCommand::ConfirmAtCursor).unwrap();
    app.cursor = (3, 3);
    app.handle_key(KeyCommand::ConfirmAtCursor).unwrap();

    // Both seats clashed on (3, 1) -> conflict marker × is rendered on the board and in the HUD!
    let frame_clash = app.render_plain();
    assert!(frame_clash.contains("!×!"));
    assert!(frame_clash.contains("Marked Clash Squares (×): (3,1)"));

    // Also verify ANSI rendering works and includes ANSI color sequences.
    let ansi_frame = app.render_ansi();
    assert!(ansi_frame.contains("\x1b["));

    // Cycle variant settings via keyboard commands and verify the variant bar updates.
    app.handle_key(KeyCommand::CycleConfluence).unwrap();
    app.handle_key(KeyCommand::CycleTieBreak).unwrap();
    app.handle_key(KeyCommand::ToggleSwaps).unwrap();
    app.handle_key(KeyCommand::ToggleRailgun).unwrap();
    let frame_variants = app.render_plain();
    assert!(frame_variants.contains("Bunch Before Merge"));
    assert!(frame_variants.contains("Row (prefer X)"));
    assert!(frame_variants.contains("Swaps: ON"));
    assert!(frame_variants.contains("Railgun: ON"));
}

#[test]
fn tui_interactive_play_against_ai_bots_and_preset_cycling() {
    let mut cfg = GameConfig::default();
    cfg.preset = BoardPreset::Blitz7x7;
    let mut app = ClientApp::new(ClientMode::VsBot(BotStrategy::Tactical), cfg).unwrap();

    // On 7x7 Blitz board, (1, 1) is an Attractor (`ATT`), and (1, 2) is Vacuum (`VAC`).
    app.cursor = (1, 1);
    app.handle_key(KeyCommand::ConfirmAtCursor).unwrap();
    app.cursor = (1, 2);
    app.handle_key(KeyCommand::ConfirmAtCursor).unwrap();

    // Cycle bot strategy and preset.
    app.handle_key(KeyCommand::CycleBotStrategy).unwrap();
    assert_eq!(app.mode, ClientMode::VsBot(BotStrategy::Minimax));

    app.handle_key(KeyCommand::CyclePreset).unwrap();
    assert_eq!(app.config.preset, BoardPreset::Mini5x5);
    assert_eq!(app.session.board.n, 5);
}

#[tokio::test]
async fn remote_client_end_to_end_matchmaking_fog_of_war_and_play_over_tcp() {
    let state = AppState::new_in_memory();
    let router = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let base_url = format!("http://{addr}");
    let client_a = RemoteClient::new(&base_url).unwrap();
    let client_b = RemoteClient::new(&base_url).unwrap();

    // Both clients enter matchmaking for Classic11x11.
    let seek_a = client_a
        .seek_matchmaking(&SeekRequest {
            player_name: Some("RemoteAlice".to_string()),
            config: GameConfig::default(),
            any_variant_with_same_board: true,
        })
        .await
        .unwrap();
    let ticket_a = match seek_a {
        SeekStatus::Queued { ticket_id, .. } => ticket_id,
        other => panic!("expected Queued, got {other:?}"),
    };

    let seek_b = client_b
        .seek_matchmaking(&SeekRequest {
            player_name: Some("RemoteBob".to_string()),
            config: GameConfig::default(),
            any_variant_with_same_board: true,
        })
        .await
        .unwrap();
    let (game_id, token_b) = match seek_b {
        SeekStatus::Matched {
            game_id,
            seat,
            seat_token,
            ..
        } => {
            assert_eq!(seat, Seat::B);
            (game_id, seat_token)
        }
        other => panic!("expected Matched, got {other:?}"),
    };

    let matched_a = client_a.matchmaking_status(&ticket_a).await.unwrap();
    let token_a = match matched_a {
        SeekStatus::Matched {
            game_id: gid,
            seat,
            seat_token,
            ..
        } => {
            assert_eq!(gid, game_id);
            assert_eq!(seat, Seat::A);
            seat_token
        }
        other => panic!("expected Matched, got {other:?}"),
    };

    // Alice selects (3, 1) and submits secret move (3, 1) -> (3, 2).
    let view_a_sel = client_a
        .select_piece(&game_id, &token_a, Some((3, 1)))
        .await
        .unwrap();
    assert_eq!(view_a_sel.seats[0].own_selection, Some((3, 1)));

    let view_a_sealed = client_a
        .submit_move(&game_id, &token_a, (3, 1), (3, 2))
        .await
        .unwrap();
    assert!(view_a_sealed.seats[0].has_committed);
    assert!(view_a_sealed.seats[0].visible_move.is_some());

    // Bob fetches his view before submitting his own move:
    let view_b_before = client_b.get_view(&game_id, Some(&token_b)).await.unwrap();
    assert!(view_b_before.seats[0].has_committed);
    assert_eq!(view_b_before.seats[0].visible_move, None);

    let rendered_b = render_frame_plain(&view_b_before, Some((5, 5)), "Remote");
    assert!(rendered_b.contains("Hidden by Fog of War"));
    assert!(!rendered_b.contains("(3,1) -> (3,2)"));

    let rendered_a = render_frame_ansi(&view_a_sealed, Some((3, 2)), "Remote");
    assert!(rendered_a.contains("(3,1) -> (3,2)"));

    // Bob submits (7, 9) -> (7, 8) -> round resolves!
    let view_after = client_b
        .submit_move(&game_id, &token_b, (7, 9), (7, 8))
        .await
        .unwrap();
    assert_eq!(view_after.turn_no, 1);

    // Export replay and verify integrity.
    let replay = client_a.export_replay(&game_id).await.unwrap();
    assert!(replay.replay_verified);
    assert_eq!(replay.turns_played.len(), 1);
}

#[test]
fn tui_cooldown_badging_raw_key_parsing_and_zero_round_trip_client_selection() {
    use automatafl_client::app::{apply_client_selection, parse_raw_key_bytes, RawKeyAction};

    // 1. Verify raw-mode single-keystroke and ANSI escape sequence parser:
    assert_eq!(
        parse_raw_key_bytes(b"\x1b[A"),
        Some((RawKeyAction::Command(KeyCommand::MoveCursor(0, -1)), 3))
    );
    assert_eq!(
        parse_raw_key_bytes(b"\x1b[B"),
        Some((RawKeyAction::Command(KeyCommand::MoveCursor(0, 1)), 3))
    );
    assert_eq!(
        parse_raw_key_bytes(b"\x1b[C"),
        Some((RawKeyAction::Command(KeyCommand::MoveCursor(1, 0)), 3))
    );
    assert_eq!(
        parse_raw_key_bytes(b"\x1b[D"),
        Some((RawKeyAction::Command(KeyCommand::MoveCursor(-1, 0)), 3))
    );
    assert_eq!(
        parse_raw_key_bytes(b"\x1b"),
        Some((RawKeyAction::Command(KeyCommand::ClearSelection), 1))
    );
    assert_eq!(
        parse_raw_key_bytes(b"h"),
        Some((RawKeyAction::Command(KeyCommand::MoveCursor(-1, 0)), 1))
    );
    assert_eq!(
        parse_raw_key_bytes(b"\r"),
        Some((RawKeyAction::Command(KeyCommand::ConfirmAtCursor), 1))
    );
    assert_eq!(
        parse_raw_key_bytes(b" "),
        Some((RawKeyAction::Command(KeyCommand::ConfirmAtCursor), 1))
    );
    assert_eq!(
        parse_raw_key_bytes(b"\t"),
        Some((RawKeyAction::Command(KeyCommand::SwitchViewer), 1))
    );
    assert_eq!(
        parse_raw_key_bytes(b"f"),
        Some((RawKeyAction::Refresh, 1))
    );
    assert_eq!(parse_raw_key_bytes(b"q"), Some((RawKeyAction::Quit, 1)));
    assert_eq!(parse_raw_key_bytes(b"\x03"), Some((RawKeyAction::Quit, 1)));
    assert_eq!(
        parse_raw_key_bytes(b"z"),
        Some((RawKeyAction::Command(KeyCommand::ToggleSwaps), 1))
    );
    assert_eq!(
        parse_raw_key_bytes(b"g"),
        Some((RawKeyAction::Command(KeyCommand::ToggleRailgun), 1))
    );
    assert_eq!(
        parse_raw_key_bytes(b"u"),
        Some((RawKeyAction::Command(KeyCommand::Resign), 1))
    );

    // 2. Verify cooldown badging (`*A*` / `*R*` + `Cooldown Squares (❄️):` HUD line)
    //    and zero-round-trip `apply_client_selection`:
    let mut cfg = GameConfig::default();
    cfg.cooldown_turns = 2;
    let mut app = ClientApp::new(ClientMode::LocalPassAndPlay, cfg).unwrap();

    // Seat A seals (3,1) -> (3,2); Seat B seals (7,9) -> (7,8)
    app.cursor = (3, 1);
    app.handle_key(KeyCommand::ConfirmAtCursor).unwrap();
    app.cursor = (3, 2);
    app.handle_key(KeyCommand::ConfirmAtCursor).unwrap();

    app.cursor = (7, 9);
    app.handle_key(KeyCommand::ConfirmAtCursor).unwrap();
    app.cursor = (7, 8);
    app.handle_key(KeyCommand::ConfirmAtCursor).unwrap();

    // Turn 1: (3,2) and (7,8) are now on cooldown!
    let plain = app.render_plain();
    assert!(plain.contains("*A*"));
    assert!(plain.contains("Cooldown Squares (❄️): (3,2), (7,8)"));
    let ansi = app.render_ansi();
    assert!(ansi.contains("\x1b[1;94m")); // frost blue ANSI color for cooldown squares

    // Test zero-round-trip client selection (`apply_client_selection`):
    let mut view = app.current_view();
    // Selecting a cooled-down square (3, 2) locally must be rejected!
    assert!(apply_client_selection(&mut view, Some((3, 2))).is_err());
    // Selecting a movable piece (4, 1) locally populates targets in 0ms!
    apply_client_selection(&mut view, Some((4, 1))).unwrap();
    let seat_idx = view.viewer_seat.unwrap().idx();
    assert_eq!(view.seats[seat_idx].own_selection, Some((4, 1)));
    assert!(!view.seats[seat_idx].legal_targets.is_empty());
    assert!(!view.seats[seat_idx].executable_targets.is_empty());

    // Selecting when the seat has already committed or the game has ended must be rejected!
    let mut committed_view = view.clone();
    committed_view.seats[seat_idx].has_committed = true;
    assert!(apply_client_selection(&mut committed_view, Some((4, 1))).is_err());

    let mut ended_view = view.clone();
    ended_view.ended = true;
    assert!(apply_client_selection(&mut ended_view, Some((4, 1))).is_err());
}

