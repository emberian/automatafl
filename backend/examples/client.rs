use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let base_url = "http://localhost:3000";

    // Register a new user
    println!("Registering user...");
    let register_response = client
        .post(&format!("{}/api/auth/register", base_url))
        .json(&json!({
            "username": "testuser",
            "password": "password123"
        }))
        .send()
        .await?;

    if register_response.status().is_success() {
        let auth_data: serde_json::Value = register_response.json().await?;
        let token = auth_data["token"].as_str().unwrap();
        println!("Registered successfully! Token: {}", token);

        // Create a new game
        println!("\nCreating a new game...");
        let create_game_response = client
            .post(&format!("{}/api/games/create", base_url))
            .header("Authorization", format!("Bearer {}", token))
            .json(&json!({}))
            .send()
            .await?;

        if create_game_response.status().is_success() {
            let game_data: serde_json::Value = create_game_response.json().await?;
            println!("Game created! ID: {}", game_data["id"]);
            println!("Board state: {}", serde_json::to_string_pretty(&game_data["board"])?);
        } else {
            println!("Failed to create game: {}", create_game_response.text().await?);
        }
    } else {
        println!("Registration failed: {}", register_response.text().await?);
    }

    Ok(())
}

// Add this to Cargo.toml to run:
// [[example]]
// name = "client"
// 
// [dev-dependencies]
// reqwest = { version = "0.11", features = ["json"] }
