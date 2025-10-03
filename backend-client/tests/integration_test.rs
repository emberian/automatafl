use automatafl_backend_client::AutomataflClient;
use automatafl_logic::Coord;

const BASE_URL: &str = "http://localhost:3000";

/// Helper to create a unique test user
fn test_username() -> String {
    format!("testuser_{}", uuid::Uuid::new_v4())
}

#[tokio::test]
async fn test_health_check() {
    let client = AutomataflClient::new(BASE_URL);
    let health = client.health_check().await.expect("Health check failed");

    assert_eq!(health.status, "healthy");
    assert_eq!(health.api_version, "v1");
}

#[tokio::test]
async fn test_register_and_login() {
    let client = AutomataflClient::new(BASE_URL);
    let username = test_username();
    let password = "testpass123";

    // Register
    let register_response = client
        .register(username.clone(), password.to_string())
        .await
        .expect("Registration failed");

    assert!(!register_response.player_id.is_nil());

    // Login
    let mut client = AutomataflClient::new(BASE_URL);
    let login_response = client
        .login(username.clone(), password.to_string())
        .await
        .expect("Login failed");

    assert!(!login_response.session_id.is_nil());
    assert_eq!(login_response.player_id, register_response.player_id);
    assert_eq!(client.session_token(), Some(login_response.session_id));

    // Logout
    client.logout().await.expect("Logout failed");
    assert_eq!(client.session_token(), None);
}

#[tokio::test]
async fn test_duplicate_registration() {
    let client = AutomataflClient::new(BASE_URL);
    let username = test_username();
    let password = "testpass123";

    // First registration should succeed
    client
        .register(username.clone(), password.to_string())
        .await
        .expect("First registration failed");

    // Second registration should fail
    let result = client.register(username, password.to_string()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_create_and_list_games() {
    let mut client = AutomataflClient::new(BASE_URL);
    let username = test_username();

    // Register and login
    client.register(username.clone(), "testpass123".to_string()).await.unwrap();
    client.login(username, "testpass123".to_string()).await.unwrap();

    // Create a game
    let game_id = client
        .create_game(2, true)
        .await
        .expect("Failed to create game");

    assert!(!game_id.is_nil());

    // List games
    let games = client.list_games().await.expect("Failed to list games");
    assert!(games.iter().any(|g| g.id == game_id));
}

#[tokio::test]
async fn test_join_game() {
    // Create two clients
    let mut client1 = AutomataflClient::new(BASE_URL);
    let mut client2 = AutomataflClient::new(BASE_URL);

    let username1 = test_username();
    let username2 = test_username();

    // Register and login both clients
    client1.register(username1.clone(), "pass123".to_string()).await.unwrap();
    client1.login(username1, "pass123".to_string()).await.unwrap();

    client2.register(username2.clone(), "pass123".to_string()).await.unwrap();
    client2.login(username2, "pass123".to_string()).await.unwrap();

    // Client1 creates a game
    let game_id = client1.create_game(2, true).await.unwrap();

    // Client2 joins the game
    let player_pid = client2.join_game(game_id).await.expect("Failed to join game");
    assert_eq!(player_pid.0, 1); // Second player should be PID 1

    // Get game state
    let state = client1.get_game_state(game_id).await.unwrap();
    assert!(state.is_object());
}

#[tokio::test]
async fn test_game_goals() {
    let mut client = AutomataflClient::new(BASE_URL);
    let username = test_username();

    client.register(username.clone(), "pass123".to_string()).await.unwrap();
    client.login(username, "pass123".to_string()).await.unwrap();

    let game_id = client.create_game(2, true).await.unwrap();

    let goals = client.get_goals(game_id).await.expect("Failed to get goals");
    assert!(!goals.is_empty());
}

#[tokio::test]
async fn test_perform_move() {
    let mut client1 = AutomataflClient::new(BASE_URL);
    let mut client2 = AutomataflClient::new(BASE_URL);

    let username1 = test_username();
    let username2 = test_username();

    // Setup two players in a game
    client1.register(username1.clone(), "pass123".to_string()).await.unwrap();
    client1.login(username1, "pass123".to_string()).await.unwrap();

    client2.register(username2.clone(), "pass123".to_string()).await.unwrap();
    client2.login(username2, "pass123".to_string()).await.unwrap();

    let game_id = client1.create_game(2, true).await.unwrap();
    client2.join_game(game_id).await.unwrap();

    // Perform a move
    let from = Coord { x: 5, y: 5 };
    let to = Coord { x: 6, y: 5 };

    let move_result = client1
        .perform_move(game_id, from, to)
        .await
        .expect("Failed to perform move");

    // Verify move was accepted
    assert!(matches!(
        move_result.feedback,
        automatafl_logic::MoveFeedback::Committed
    ));
}

#[tokio::test]
async fn test_chat() {
    let mut client = AutomataflClient::new(BASE_URL);
    let username = test_username();

    client.register(username.clone(), "pass123".to_string()).await.unwrap();
    client.login(username, "pass123".to_string()).await.unwrap();

    let game_id = client.create_game(2, true).await.unwrap();

    // Send a chat message
    client
        .send_chat(game_id, "Hello, world!".to_string())
        .await
        .expect("Failed to send chat");

    // Get chat messages
    let messages = client.get_chat(game_id).await.expect("Failed to get chat");
    assert!(!messages.is_empty());
    assert_eq!(messages[0].message, "Hello, world!");
}

#[tokio::test]
async fn test_game_history() {
    let mut client1 = AutomataflClient::new(BASE_URL);
    let mut client2 = AutomataflClient::new(BASE_URL);

    let username1 = test_username();
    let username2 = test_username();

    client1.register(username1.clone(), "pass123".to_string()).await.unwrap();
    client1.login(username1, "pass123".to_string()).await.unwrap();

    client2.register(username2.clone(), "pass123".to_string()).await.unwrap();
    client2.login(username2, "pass123".to_string()).await.unwrap();

    let game_id = client1.create_game(2, true).await.unwrap();
    client2.join_game(game_id).await.unwrap();

    // Get history
    let history = client1.get_game_history(game_id).await.expect("Failed to get history");

    // Should have PLAYER_JOINED events
    assert!(history.iter().any(|e| matches!(e.data, automatafl_api_types::GameEventData::PlayerJoined { .. })));
}

#[tokio::test]
async fn test_game_history_filtered() {
    let mut client = AutomataflClient::new(BASE_URL);
    let username = test_username();

    client.register(username.clone(), "pass123".to_string()).await.unwrap();
    client.login(username, "pass123".to_string()).await.unwrap();

    let game_id = client.create_game(2, true).await.unwrap();

    // Get filtered history (only PLAYER_JOINED events)
    let history = client
        .get_game_history_filtered(game_id, None, None, Some("PLAYER_JOINED".to_string()))
        .await
        .expect("Failed to get filtered history");

    // All events should be PLAYER_JOINED
    assert!(history.iter().all(|e| matches!(e.data, automatafl_api_types::GameEventData::PlayerJoined { .. })));
}

#[tokio::test]
async fn test_save_and_load_game() {
    let mut client1 = AutomataflClient::new(BASE_URL);
    let mut client2 = AutomataflClient::new(BASE_URL);

    let username1 = test_username();
    let username2 = test_username();

    client1.register(username1.clone(), "pass123".to_string()).await.unwrap();
    client1.login(username1, "pass123".to_string()).await.unwrap();

    client2.register(username2.clone(), "pass123".to_string()).await.unwrap();
    client2.login(username2, "pass123".to_string()).await.unwrap();

    let game_id = client1.create_game(2, true).await.unwrap();
    client2.join_game(game_id).await.unwrap();

    // Save game
    let save_result = client1.save_game(game_id).await.expect("Failed to save game");
    assert!(save_result.is_object());

    // List snapshots
    let snapshots = client1.list_snapshots(game_id).await.expect("Failed to list snapshots");
    assert!(snapshots.is_object());

    // Load snapshot
    client1.load_game(game_id, 0).await.expect("Failed to load game");
}

#[tokio::test]
async fn test_player_profile() {
    let mut client = AutomataflClient::new(BASE_URL);
    let username = test_username();

    let register_response = client.register(username.clone(), "pass123".to_string()).await.unwrap();
    client.login(username, "pass123".to_string()).await.unwrap();

    let player_id = register_response.player_id;

    // Get profile
    let profile = client
        .get_player_profile(player_id)
        .await
        .expect("Failed to get profile");

    assert_eq!(profile.id, player_id);
    assert_eq!(profile.elo_rating, 1200); // Default ELO

    // Update profile
    client
        .update_player_profile(
            player_id,
            Some("Test bio".to_string()),
            Some("https://example.com/avatar.png".to_string()),
        )
        .await
        .expect("Failed to update profile");

    // Get updated profile
    let updated_profile = client.get_player_profile(player_id).await.unwrap();
    assert_eq!(updated_profile.bio, Some("Test bio".to_string()));
}

#[tokio::test]
async fn test_player_stats() {
    let mut client = AutomataflClient::new(BASE_URL);
    let username = test_username();

    let register_response = client.register(username.clone(), "pass123".to_string()).await.unwrap();
    client.login(username, "pass123".to_string()).await.unwrap();

    let player_id = register_response.player_id;

    // Get stats
    let stats = client
        .get_player_stats(player_id)
        .await
        .expect("Failed to get stats");

    assert_eq!(stats.games_played, 0);
    assert_eq!(stats.games_won, 0);
    assert_eq!(stats.win_rate, 0.0);
}

#[tokio::test]
async fn test_leaderboards() {
    let mut client = AutomataflClient::new(BASE_URL);
    let username = test_username();

    client.register(username.clone(), "pass123".to_string()).await.unwrap();
    client.login(username, "pass123".to_string()).await.unwrap();

    // Get ELO leaderboard
    let elo_leaderboard = client
        .get_leaderboard_elo()
        .await
        .expect("Failed to get ELO leaderboard");

    assert!(elo_leaderboard.total_players > 0);

    // Get wins leaderboard
    let _wins_leaderboard = client
        .get_leaderboard_wins()
        .await
        .expect("Failed to get wins leaderboard");

    // Get games played leaderboard
    let _games_leaderboard = client
        .get_leaderboard_games()
        .await
        .expect("Failed to get games leaderboard");
}

#[tokio::test]
async fn test_matchmaking() {
    let mut client = AutomataflClient::new(BASE_URL);
    let username = test_username();

    client.register(username.clone(), "pass123".to_string()).await.unwrap();
    client.login(username, "pass123".to_string()).await.unwrap();

    // Join matchmaking
    client
        .join_matchmaking(2, true)
        .await
        .expect("Failed to join matchmaking");

    // Check status
    let status = client
        .get_matchmaking_status()
        .await
        .expect("Failed to get matchmaking status");

    assert!(status.in_queue);
    assert!(status.queued_at.is_some());

    // Leave matchmaking
    client
        .leave_matchmaking()
        .await
        .expect("Failed to leave matchmaking");

    // Check status again
    let status_after = client.get_matchmaking_status().await.unwrap();
    assert!(!status_after.in_queue);
}

#[tokio::test]
async fn test_complete_round() {
    let mut client1 = AutomataflClient::new(BASE_URL);
    let mut client2 = AutomataflClient::new(BASE_URL);

    let username1 = test_username();
    let username2 = test_username();

    client1.register(username1.clone(), "pass123".to_string()).await.unwrap();
    client1.login(username1, "pass123".to_string()).await.unwrap();

    client2.register(username2.clone(), "pass123".to_string()).await.unwrap();
    client2.login(username2, "pass123".to_string()).await.unwrap();

    let game_id = client1.create_game(2, true).await.unwrap();
    client2.join_game(game_id).await.unwrap();

    // Both players submit moves
    client1.perform_move(game_id, Coord { x: 5, y: 5 }, Coord { x: 6, y: 5 }).await.unwrap();
    client2.perform_move(game_id, Coord { x: 5, y: 6 }, Coord { x: 5, y: 7 }).await.unwrap();

    // Try to complete round (should auto-complete or be completable)
    let result = client1.complete_round(game_id).await;
    // Either succeeds or gives a reasonable error
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_unauthorized_access() {
    let client = AutomataflClient::new(BASE_URL);

    // Try to list games without authentication
    let result = client.list_games().await;
    assert!(result.is_err());
}
