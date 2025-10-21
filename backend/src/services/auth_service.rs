use std::fmt;

use crate::{
    common::timestamp,
    db::{PlayerRecord, SessionRecord, as_uuid},
    repositories::{PlayerRepository, SessionRepository},
    validation,
};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use tracing::{debug, warn};
use uuid::Uuid;

/// Result returned after a successful login
pub struct LoginResult {
    pub session_id: Uuid,
    pub player_id: Uuid,
    pub is_admin: bool,
}

/// Result returned after a successful registration
pub struct RegisterResult {
    pub player_id: Uuid,
}

/// Service responsible for player authentication and session lifecycle
pub struct AuthService {
    player_repo: PlayerRepository,
    session_repo: SessionRepository,
    session_duration: u64,
}

#[derive(Debug)]
pub enum AuthServiceError {
    DisplaynameTaken(String),
    InvalidCredentials,
    ValidationError(String),
    Database(surrealdb::Error),
    PasswordHash(String),
}

impl From<surrealdb::Error> for AuthServiceError {
    fn from(value: surrealdb::Error) -> Self {
        Self::Database(value)
    }
}

impl fmt::Display for AuthServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthServiceError::DisplaynameTaken(name) => {
                write!(f, "Displayname already taken: {}", name)
            }
            AuthServiceError::InvalidCredentials => write!(f, "Invalid credentials"),
            AuthServiceError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            AuthServiceError::Database(e) => write!(f, "Database error: {}", e),
            AuthServiceError::PasswordHash(msg) => write!(f, "Password hashing error: {}", msg),
        }
    }
}

impl std::error::Error for AuthServiceError {}

impl AuthService {
    pub async fn new(
        player_repo: PlayerRepository,
        session_repo: SessionRepository,
        session_duration: u64,
    ) -> Self {
        let this = Self {
            player_repo,
            session_repo,
            session_duration,
        };

        // Create admin account if it doesn't exist
        // FIXED: Handle missing ADMIN_PASSWORD gracefully with proper error message
        if this
            .player_repo
            .find_by_displayname("admin".to_string())
            .await
            .is_ok_and(|o| o.is_none())
        {
            match std::env::var("ADMIN_PASSWORD") {
                Ok(admin_password) => {
                    match this
                        .register("admin".to_string(), admin_password, true)
                        .await
                    {
                        Ok(_) => {
                            tracing::info!("Admin account created successfully");
                        }
                        Err(err) => {
                            tracing::error!(
                                "Failed to create admin account: {}. Admin features will not be available.",
                                err
                            );
                            tracing::error!("This is a critical error if you need admin access.");
                        }
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        "ADMIN_PASSWORD environment variable not set. Admin account will not be created."
                    );
                    tracing::warn!(
                        "To create an admin account, set ADMIN_PASSWORD and restart the server."
                    );
                    tracing::warn!(
                        "Admin features will not be available until an admin account exists."
                    );
                }
            }
        } else {
            tracing::info!("Admin account already exists, skipping creation");
        }
        this
    }

    /// Register a new player account
    pub async fn register(
        &self,
        displayname: String,
        password: String,
        is_admin: bool,
    ) -> Result<RegisterResult, AuthServiceError> {
        // Validate incoming payload
        validation::validate_displayname(&displayname)
            .map_err(|e| AuthServiceError::ValidationError(e.to_string()))?;
        validate_password(&password)?;

        if self
            .player_repo
            .find_by_displayname(displayname.clone())
            .await?
            .is_some()
        {
            return Err(AuthServiceError::DisplaynameTaken(displayname));
        }

        // Hash password
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| AuthServiceError::PasswordHash(e.to_string()))?
            .to_string();

        let player_id = Uuid::new_v4();
        debug!(player_id = %player_id, displayname = %displayname, "Creating player with password hash");
        self.player_repo
            .create(
                player_id,
                displayname,
                password_hash,
                salt.to_string(),
                is_admin,
            )
            .await?;

        Ok(RegisterResult { player_id })
    }

    /// Attempt to log a player in and return a new session
    pub async fn login(
        &self,
        displayname: String,
        password: String,
    ) -> Result<LoginResult, AuthServiceError> {
        let player = self
            .player_repo
            .find_by_displayname(displayname.clone())
            .await?
            .ok_or(AuthServiceError::InvalidCredentials)?;

        verify_password(&player, &password)?;
        let player_id = as_uuid(&player.id);

        let session_id = Uuid::new_v4();
        let expires_at = timestamp() + self.session_duration;
        self.session_repo
            .create(session_id, player_id, expires_at)
            .await?;

        Ok(LoginResult {
            session_id,
            player_id,
            is_admin: player.is_admin,
        })
    }

    /// Delete an existing session
    pub async fn logout(&self, session_id: Uuid) -> Result<(), AuthServiceError> {
        self.session_repo.delete(session_id).await?;
        Ok(())
    }

    /// Fetch a session for authentication middleware
    pub async fn get_session(
        &self,
        session_id: Uuid,
    ) -> Result<Option<SessionRecord>, AuthServiceError> {
        self.session_repo.get(session_id).await.map_err(Into::into)
    }

    /// Cleanup expired sessions
    pub async fn cleanup_expired(&self, now: u64) -> Result<u64, AuthServiceError> {
        self.session_repo
            .cleanup_expired(now)
            .await
            .map_err(Into::into)
    }
}

fn validate_password(password: &str) -> Result<(), AuthServiceError> {
    // FIXED: Strengthened password requirements for production
    if password.len() < 12 {
        return Err(AuthServiceError::ValidationError(
            "Password must be at least 12 characters long".to_string(),
        ));
    }
    if password.len() > 128 {
        return Err(AuthServiceError::ValidationError(
            "Password must not exceed 128 characters".to_string(),
        ));
    }
    if !password.chars().all(|c| !c.is_control()) {
        return Err(AuthServiceError::ValidationError(
            "Password cannot contain control characters".to_string(),
        ));
    }

    // Check for complexity - at least one uppercase, lowercase, digit
    let has_upper = password.chars().any(|c| c.is_uppercase());
    let has_lower = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_numeric());

    if !has_upper || !has_lower || !has_digit {
        return Err(AuthServiceError::ValidationError(
            "Password must contain at least one uppercase letter, one lowercase letter, and one digit".to_string(),
        ));
    }

    Ok(())
}

fn verify_password(player: &PlayerRecord, password: &str) -> Result<(), AuthServiceError> {
    let player_id = crate::db::as_uuid(&player.id);
    debug!(player_id = %player_id, "Verifying player password");

    let parsed_hash = PasswordHash::new(&player.password_hash).map_err(|e| {
        warn!(player_id = %player_id, error = %e, "Stored password hash invalid");
        AuthServiceError::PasswordHash(e.to_string())
    })?;

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|e| {
            warn!(player_id = %player_id, error = %e, "Password verification failed");
            AuthServiceError::InvalidCredentials
        })
}
