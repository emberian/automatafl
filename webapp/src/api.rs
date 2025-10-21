// Wrapper around automatafl-backend-client for webapp usage
// Re-exports the client with Deref for transparent method access

use automatafl_backend_client::AutomataflClient;
use std::ops::Deref;
use uuid::Uuid;

// Note: Re-exports removed - import types directly where needed

/// Webapp-specific wrapper around the backend client
/// Uses Deref to forward all method calls to the inner client
#[derive(Clone)]
pub struct ApiClient {
    inner: AutomataflClient,
}

impl ApiClient {
    pub fn new(base_url: String, session_token: Option<Uuid>) -> Self {
        let inner = if let Some(token) = session_token {
            AutomataflClient::with_session(base_url, token)
        } else {
            AutomataflClient::new(base_url)
        };

        Self { inner }
    }

    pub fn with_session(base_url: String, session_token: Uuid) -> Self {
        Self::new(base_url, Some(session_token))
    }
}

// Implement Deref to forward all method calls automatically to inner client
impl Deref for ApiClient {
    type Target = AutomataflClient;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// Also implement DerefMut to allow mutable access when needed
impl std::ops::DerefMut for ApiClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
