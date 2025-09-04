mod auth;
mod game;
mod health;
mod home;
mod leaderboard;
mod matchmaking;
mod profile;

pub use auth::{LoginPage, RegisterPage};
pub use game::{CreateGamePage, GameHistoryPage, GamePage, GamesListPage, SpectatePage};
pub use health::{HealthDashboardPage, MetricsPage};
pub use home::HomePage;
pub use leaderboard::LeaderboardPage;
pub use matchmaking::MatchmakingPage;
pub use profile::{UserGamesPage, UserProfilePage};
