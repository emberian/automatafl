use std::sync::Arc;

use askama::Template;
use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, StatusCode, request::Parts},
    response::{Html, IntoResponse, Redirect, Response},
};
use uuid::Uuid;

use crate::{
    common::{AppState, timestamp},
    db::{PlayerRecord, as_uuid},
};

/// Session cookie key used by the web frontend
pub const SESSION_COOKIE_NAME: &str = "automatafl_session";

/// Extract session id from request cookies
pub fn session_id_from_headers(headers: &HeaderMap) -> Option<Uuid> {
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|cookie| {
            let trimmed = cookie.trim();
            if let Some(value) = trimmed.strip_prefix(&format!("{}=", SESSION_COOKIE_NAME)) {
                Uuid::parse_str(value).ok()
            } else {
                None
            }
        })
}

/// Resolve a player and their id from a session cookie
pub async fn resolve_session_player(
    state: &AppState,
    session_id: Uuid,
) -> Option<(Uuid, PlayerRecord)> {
    let session = match state.auth_service.get_session(session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return None,
        Err(err) => {
            tracing::error!(session_id = %session_id, "Failed to fetch session: {}", err);
            return None;
        }
    };

    if session.expires_at < timestamp() {
        return None;
    }

    let player_id = as_uuid(&session.player_id);
    match state.player_service.get_player(player_id).await {
        Ok(Some(player)) => Some((player_id, player)),
        Ok(None) => None,
        Err(err) => {
            tracing::error!(player_id = %player_id, "Failed to load player: {}", err);
            None
        }
    }
}

/// Extracts an authenticated web player from request cookies
pub struct WebPlayer {
    pub session_id: Uuid,
    pub player_id: Uuid,
    pub player: PlayerRecord,
}

impl WebPlayer {
    pub fn into_inner(self) -> PlayerRecord {
        self.player
    }

    pub fn player(&self) -> &PlayerRecord {
        &self.player
    }

    pub fn id(&self) -> Uuid {
        self.player_id
    }

    pub fn session_id(&self) -> Uuid {
        self.session_id
    }
}

impl FromRequestParts<Arc<AppState>> for WebPlayer {
    type Rejection = Redirect;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let session_id =
            session_id_from_headers(&parts.headers).ok_or_else(|| Redirect::to("/login"))?;

        let (player_id, player) = resolve_session_player(state.as_ref(), session_id)
            .await
            .ok_or_else(|| Redirect::to("/login"))?;

        Ok(Self {
            session_id,
            player_id,
            player,
        })
    }
}

/// Extracts an authenticated admin player from request cookies
pub struct AdminWebPlayer(pub WebPlayer);

impl AdminWebPlayer {
    pub fn player(&self) -> &PlayerRecord {
        self.0.player()
    }

    pub fn player_id(&self) -> Uuid {
        self.0.id()
    }

    pub fn session_id(&self) -> Uuid {
        self.0.session_id()
    }

    pub fn into_inner(self) -> WebPlayer {
        self.0
    }
}

impl FromRequestParts<Arc<AppState>> for AdminWebPlayer {
    type Rejection = Redirect;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let web_player = WebPlayer::from_request_parts(parts, state).await?;

        if !web_player.player.is_admin {
            return Err(Redirect::to("/login"));
        }

        Ok(AdminWebPlayer(web_player))
    }
}

/// Wrapper type that renders Askama templates into responses
pub struct HtmlTemplate<T: Template>(pub T);

impl<T: Template> IntoResponse for HtmlTemplate<T> {
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(rendered) => Html(rendered).into_response(),
            Err(err) => {
                tracing::error!("Template rendering failed: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Template rendering failed: {}", err),
                )
                    .into_response()
            }
        }
    }
}
