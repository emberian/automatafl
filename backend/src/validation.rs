//! Input validation utilities

use crate::common::AppError;

/// Validate displayname
/// Requirements:
/// - Not empty
/// - Maximum 50 characters
/// - No leading/trailing whitespace
/// - Only printable characters
pub fn validate_displayname(displayname: &str) -> Result<(), AppError> {
    let trimmed = displayname.trim();

    if displayname != trimmed {
        return Err(AppError::ValidationError(
            "Displayname cannot have leading or trailing whitespace".to_string(),
        ));
    }

    if displayname.is_empty() {
        return Err(AppError::ValidationError(
            "Displayname cannot be empty".to_string(),
        ));
    }

    if displayname.len() > 50 {
        return Err(AppError::ValidationError(
            "Displayname cannot exceed 50 characters".to_string(),
        ));
    }

    // Check for control characters
    if displayname.chars().any(|c| c.is_control()) {
        return Err(AppError::ValidationError(
            "Displayname cannot contain control characters".to_string(),
        ));
    }

    Ok(())
}

/// Validate username
/// Requirements:
/// - 3-32 characters
/// - Alphanumeric, underscore, hyphen only
/// - Must start with alphanumeric
#[allow(unused)]
pub fn validate_username(username: &str) -> Result<(), AppError> {
    if username.len() < 3 {
        return Err(AppError::ValidationError(
            "Username must be at least 3 characters".to_string(),
        ));
    }

    if username.len() > 32 {
        return Err(AppError::ValidationError(
            "Username cannot exceed 32 characters".to_string(),
        ));
    }

    // Must start with alphanumeric
    if let Some(first_char) = username.chars().next() {
        if !first_char.is_alphanumeric() {
            return Err(AppError::ValidationError(
                "Username must start with a letter or number".to_string(),
            ));
        }
    } else {
        return Err(AppError::ValidationError(
            "Username cannot be empty".to_string(),
        ));
    }

    // Only alphanumeric, underscore, hyphen
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AppError::ValidationError(
            "Username can only contain letters, numbers, underscores, and hyphens".to_string(),
        ));
    }

    Ok(())
}

/// Validate bio text
pub fn validate_bio(bio: &str) -> Result<(), AppError> {
    if bio.len() > 500 {
        return Err(AppError::ValidationError(
            "Bio must not exceed 500 characters".to_string(),
        ));
    }
    Ok(())
}

/// Validate avatar URL
/// In production, only HTTPS is allowed to prevent mixed content warnings
pub fn validate_avatar_url(url: &str) -> Result<(), AppError> {
    if url.is_empty() {
        return Ok(()); // Empty is OK (optional field)
    }

    if url.len() > 2048 {
        return Err(AppError::ValidationError(
            "Avatar URL must not exceed 2048 characters".to_string(),
        ));
    }

    // Production: require HTTPS for security (prevent mixed content)
    // Development: allow HTTP for testing
    if cfg!(debug_assertions) {
        // Development: allow both HTTP and HTTPS
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(AppError::ValidationError(
                "Avatar URL must start with http:// or https://".to_string(),
            ));
        }
    } else {
        // Production: HTTPS only
        if !url.starts_with("https://") {
            return Err(AppError::ValidationError(
                "Avatar URL must use HTTPS (https://) in production".to_string(),
            ));
        }
    }

    Ok(())
}

/// Validate chat message
pub fn validate_chat_message(message: &str) -> Result<(), AppError> {
    if message.is_empty() {
        return Err(AppError::ValidationError(
            "Message cannot be empty".to_string(),
        ));
    }

    if message.len() > 1000 {
        return Err(AppError::ValidationError(
            "Message must not exceed 1000 characters".to_string(),
        ));
    }

    // Check for control characters (except newline/tab)
    if message
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(AppError::ValidationError(
            "Message contains invalid characters".to_string(),
        ));
    }

    Ok(())
}

/// Validate move coordinates are within board bounds
pub fn validate_move_coords(from_x: u8, from_y: u8, to_x: u8, to_y: u8) -> Result<(), AppError> {
    const MAX_COORD: u8 = 10;

    if from_x > MAX_COORD || from_y > MAX_COORD || to_x > MAX_COORD || to_y > MAX_COORD {
        return Err(AppError::ValidationError(
            "Move coordinates must be between 0 and 10".to_string(),
        ));
    }

    Ok(())
}

#[allow(unused)]
/// Validate game name
pub fn validate_game_name(name: &str) -> Result<(), AppError> {
    let trimmed = name.trim();

    if name != trimmed {
        return Err(AppError::ValidationError(
            "Game name cannot have leading or trailing whitespace".to_string(),
        ));
    }

    if name.is_empty() {
        return Err(AppError::ValidationError(
            "Game name cannot be empty".to_string(),
        ));
    }

    if name.len() > 100 {
        return Err(AppError::ValidationError(
            "Game name cannot exceed 100 characters".to_string(),
        ));
    }

    // Check for control characters
    if name.chars().any(|c| c.is_control()) {
        return Err(AppError::ValidationError(
            "Game name cannot contain control characters".to_string(),
        ));
    }

    Ok(())
}

#[allow(unused)]
/// Validate player count for game creation
pub fn validate_player_count(count: u8) -> Result<(), AppError> {
    if count < 2 {
        return Err(AppError::ValidationError(
            "Game must have at least 2 players".to_string(),
        ));
    }

    if count > 4 {
        return Err(AppError::ValidationError(
            "Game cannot have more than 4 players".to_string(),
        ));
    }

    Ok(())
}

#[allow(unused)]
/// Validate ELO rating is within reasonable bounds
pub fn validate_elo_rating(elo: i32) -> Result<(), AppError> {
    if elo < 0 {
        return Err(AppError::ValidationError(
            "ELO rating cannot be negative".to_string(),
        ));
    }

    if elo > 5000 {
        return Err(AppError::ValidationError(
            "ELO rating cannot exceed 5000".to_string(),
        ));
    }

    Ok(())
}

#[allow(unused)]
/// Validate pagination parameters
pub fn validate_pagination(
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<(usize, usize), AppError> {
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(50);

    if limit == 0 {
        return Err(AppError::ValidationError(
            "Limit must be greater than 0".to_string(),
        ));
    }

    if limit > 1000 {
        return Err(AppError::ValidationError(
            "Limit cannot exceed 1000 items".to_string(),
        ));
    }

    Ok((offset, limit))
}

#[allow(unused)]
/// Validate snapshot name
pub fn validate_snapshot_name(name: &str) -> Result<(), AppError> {
    if name.is_empty() {
        return Err(AppError::ValidationError(
            "Snapshot name cannot be empty".to_string(),
        ));
    }

    if name.len() > 100 {
        return Err(AppError::ValidationError(
            "Snapshot name cannot exceed 100 characters".to_string(),
        ));
    }

    // Only alphanumeric, underscore, hyphen, space
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ')
    {
        return Err(AppError::ValidationError(
            "Snapshot name can only contain letters, numbers, underscores, hyphens, and spaces"
                .to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bio_validation() {
        assert!(validate_bio("Short bio").is_ok());
        assert!(validate_bio(&"a".repeat(500)).is_ok());
        assert!(validate_bio(&"a".repeat(501)).is_err());
    }

    #[test]
    fn test_avatar_url_validation() {
        assert!(validate_avatar_url("").is_ok()); // Empty is OK
        assert!(validate_avatar_url("https://example.com/avatar.png").is_ok());
        assert!(validate_avatar_url("http://example.com/avatar.png").is_ok());
        assert!(validate_avatar_url("ftp://example.com").is_err());
        assert!(validate_avatar_url(&"a".repeat(2049)).is_err());
    }

    #[test]
    fn test_chat_message_validation() {
        assert!(validate_chat_message("Hello world").is_ok());
        assert!(validate_chat_message("").is_err());
        assert!(validate_chat_message(&"a".repeat(1001)).is_err());
        assert!(validate_chat_message("Hello\nWorld").is_ok()); // Newlines OK
        assert!(validate_chat_message("Hello\x00World").is_err()); // Null byte not OK
    }

    #[test]
    fn test_move_coords_validation() {
        assert!(validate_move_coords(0, 0, 10, 10).is_ok());
        assert!(validate_move_coords(5, 5, 5, 7).is_ok());
        assert!(validate_move_coords(0, 0, 11, 5).is_err());
        assert!(validate_move_coords(11, 11, 5, 5).is_err());
    }

    #[test]
    fn test_displayname_validation() {
        assert!(validate_displayname("Valid Name").is_ok());
        assert!(validate_displayname("").is_err());
        assert!(validate_displayname(&"a".repeat(51)).is_err());
        assert!(validate_displayname(" Leading space").is_err());
        assert!(validate_displayname("Trailing space ").is_err());
        assert!(validate_displayname("Has\x00null").is_err());
    }

    #[test]
    fn test_username_validation() {
        assert!(validate_username("valid_user123").is_ok());
        assert!(validate_username("ab").is_err()); // Too short
        assert!(validate_username(&"a".repeat(33)).is_err()); // Too long
        assert!(validate_username("_invalid").is_err()); // Must start with alphanumeric
        assert!(validate_username("invalid!").is_err()); // Invalid character
    }

    #[test]
    fn test_game_name_validation() {
        assert!(validate_game_name("My Game").is_ok());
        assert!(validate_game_name("").is_err());
        assert!(validate_game_name(&"a".repeat(101)).is_err());
        assert!(validate_game_name(" Leading").is_err());
    }

    #[test]
    fn test_player_count_validation() {
        assert!(validate_player_count(2).is_ok());
        assert!(validate_player_count(4).is_ok());
        assert!(validate_player_count(1).is_err());
        assert!(validate_player_count(5).is_err());
    }

    #[test]
    fn test_elo_validation() {
        assert!(validate_elo_rating(1200).is_ok());
        assert!(validate_elo_rating(0).is_ok());
        assert!(validate_elo_rating(-1).is_err());
        assert!(validate_elo_rating(5001).is_err());
    }

    #[test]
    fn test_pagination_validation() {
        assert_eq!(validate_pagination(None, None).unwrap(), (0, 50));
        assert_eq!(validate_pagination(Some(100), Some(25)).unwrap(), (100, 25));
        assert!(validate_pagination(None, Some(0)).is_err());
        assert!(validate_pagination(None, Some(1001)).is_err());
    }

    #[test]
    fn test_snapshot_name_validation() {
        assert!(validate_snapshot_name("my_snapshot-1").is_ok());
        assert!(validate_snapshot_name("").is_err());
        assert!(validate_snapshot_name(&"a".repeat(101)).is_err());
        assert!(validate_snapshot_name("invalid!name").is_err());
    }
}
