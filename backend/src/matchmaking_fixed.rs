// Fixed matchmaking queue with single source of truth
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::models::User;

#[derive(Debug, Clone)]
pub struct MatchmakingEntry {
    pub user_id: Uuid,
    pub username: String,
    pub rating: i32,
    pub joined_at: DateTime<Utc>,
    pub rating_range: Option<(i32, i32)>,
}

// Single source of truth design
pub struct MatchmakingQueue {
    // Single HashMap as source of truth
    queue: Arc<Mutex<HashMap<Uuid, MatchmakingEntry>>>,
}

impl MatchmakingQueue {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    pub async fn join(&self, user: &User, rating_range: Option<(i32, i32)>) -> Result<(), String> {
        let entry = MatchmakingEntry {
            user_id: user.id,
            username: user.username.clone(),
            rating: user.rating,
            joined_at: Utc::now(),
            rating_range,
        };
        
        let mut queue = self.queue.lock().await;
        
        // Check if already in queue
        if queue.contains_key(&user.id) {
            return Err("Already in matchmaking queue".to_string());
        }
        
        queue.insert(user.id, entry);
        Ok(())
    }
    
    pub async fn leave(&self, user_id: Uuid) -> Result<(), String> {
        let mut queue = self.queue.lock().await;
        
        if queue.remove(&user_id).is_none() {
            return Err("Not in matchmaking queue".to_string());
        }
        
        Ok(())
    }
    
    pub async fn find_match(&self, user_id: Uuid) -> Option<MatchmakingEntry> {
        let mut queue = self.queue.lock().await;
        
        // Get user's entry
        let user_entry = queue.get(&user_id)?.clone();
        
        // Find suitable opponent
        let opponent_id = queue
            .iter()
            .filter(|(id, entry)| {
                // Skip self
                if **id == user_id {
                    return false;
                }
                
                // Check rating compatibility
                let rating_diff = (entry.rating - user_entry.rating).abs();
                
                // Check user's rating range preference
                if let Some((min, max)) = user_entry.rating_range {
                    if entry.rating < min || entry.rating > max {
                        return false;
                    }
                }
                
                // Check opponent's rating range preference
                if let Some((min, max)) = entry.rating_range {
                    if user_entry.rating < min || user_entry.rating > max {
                        return false;
                    }
                }
                
                // Default: within 200 rating points
                rating_diff <= 200
            })
            .map(|(id, _)| *id)
            .next()?;
        
        // Remove both players atomically
        queue.remove(&user_id);
        let opponent = queue.remove(&opponent_id)?;
        
        Some(opponent)
    }
    
    pub async fn get_queue_status(&self) -> (usize, Option<u32>) {
        let queue = self.queue.lock().await;
        let queue_size = queue.len();
        
        // More realistic wait time estimation
        let estimated_wait = match queue_size {
            0 => None,
            1..=5 => Some(60),    // 1 minute for small queue
            6..=20 => Some(30),   // 30 seconds for medium queue
            21..=50 => Some(15),  // 15 seconds for large queue
            _ => Some(5),         // 5 seconds for very large queue
        };
        
        (queue_size, estimated_wait)
    }
    
    pub async fn is_in_queue(&self, user_id: &Uuid) -> bool {
        let queue = self.queue.lock().await;
        queue.contains_key(user_id)
    }
    
    // Helper for matchmaking worker - get all entries efficiently
    pub async fn get_all_entries(&self) -> Vec<MatchmakingEntry> {
        let queue = self.queue.lock().await;
        queue.values().cloned().collect()
    }
    
    // Batch remove for matched players
    pub async fn remove_matched_players(&self, player_ids: &[(Uuid, Uuid)]) {
        let mut queue = self.queue.lock().await;
        for (id1, id2) in player_ids {
            queue.remove(id1);
            queue.remove(id2);
        }
    }
}

// Optimized matchmaking worker
pub async fn matchmaking_worker_optimized(state: crate::state::AppState) {
    use tokio::time::{sleep, Duration};
    
    loop {
        sleep(Duration::from_secs(5)).await;
        
        // Get all entries at once
        let entries = state.matchmaking_queue.get_all_entries().await;
        
        if entries.len() < 2 {
            continue;
        }
        
        // Find all possible matches
        let mut matched_pairs = Vec::new();
        let mut matched_users = std::collections::HashSet::new();
        
        for i in 0..entries.len() {
            if matched_users.contains(&entries[i].user_id) {
                continue;
            }
            
            for j in (i + 1)..entries.len() {
                if matched_users.contains(&entries[j].user_id) {
                    continue;
                }
                
                let rating_diff = (entries[i].rating - entries[j].rating).abs();
                
                // Check rating ranges
                let compatible = check_rating_compatibility(&entries[i], &entries[j]) 
                    && check_rating_compatibility(&entries[j], &entries[i])
                    && rating_diff <= 200;
                
                if compatible {
                    matched_pairs.push((entries[i].user_id, entries[j].user_id));
                    matched_users.insert(entries[i].user_id);
                    matched_users.insert(entries[j].user_id);
                    
                    tracing::info!(
                        "Matched {} ({}) with {} ({})",
                        entries[i].username, entries[i].rating,
                        entries[j].username, entries[j].rating
                    );
                    
                    break; // Move to next player
                }
            }
        }
        
        // Remove all matched players at once
        if !matched_pairs.is_empty() {
            state.matchmaking_queue.remove_matched_players(&matched_pairs).await;
            
            // Create games for matched pairs
            for (player1_id, player2_id) in matched_pairs {
                // TODO: Create game and notify players via WebSocket
            }
        }
    }
}

fn check_rating_compatibility(player: &MatchmakingEntry, opponent: &MatchmakingEntry) -> bool {
    if let Some((min, max)) = player.rating_range {
        opponent.rating >= min && opponent.rating <= max
    } else {
        true
    }
}
