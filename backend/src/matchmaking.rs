use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::models::User;

#[derive(Debug, Clone)]
pub struct MatchmakingEntry {
    pub user_id: Uuid,
    pub username: String,
    pub rating: i32,
    pub joined_at: DateTime<Utc>,
    pub rating_range: Option<(i32, i32)>,
}

pub struct MatchmakingQueue {
    pub queue: Arc<Mutex<Vec<MatchmakingEntry>>>,
    pub user_in_queue: Arc<DashMap<Uuid, bool>>,
}

impl MatchmakingQueue {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
            user_in_queue: Arc::new(DashMap::new()),
        }
    }
    
    pub async fn join(&self, user: &User, rating_range: Option<(i32, i32)>) -> Result<(), String> {
        // Check if user is already in queue
        if self.user_in_queue.contains_key(&user.id) {
            return Err("Already in matchmaking queue".to_string());
        }
        
        let entry = MatchmakingEntry {
            user_id: user.id,
            username: user.username.clone(),
            rating: user.rating,
            joined_at: Utc::now(),
            rating_range,
        };
        
        let mut queue = self.queue.lock().await;
        queue.push(entry);
        self.user_in_queue.insert(user.id, true);
        
        Ok(())
    }
    
    pub async fn leave(&self, user_id: Uuid) -> Result<(), String> {
        self.user_in_queue.remove(&user_id);
        
        let mut queue = self.queue.lock().await;
        queue.retain(|entry| entry.user_id != user_id);
        
        Ok(())
    }
    
    pub async fn find_match(&self, user_id: Uuid) -> Option<MatchmakingEntry> {
        let mut queue = self.queue.lock().await;
        
        // Find the user's entry
        let user_index = queue.iter().position(|e| e.user_id == user_id)?;
        let user_entry = queue[user_index].clone();
        
        // Find a suitable opponent
        let opponent_index = queue.iter().position(|entry| {
            if entry.user_id == user_id {
                return false;
            }
            
            // Check if ratings are within acceptable range
            let rating_diff = (entry.rating - user_entry.rating).abs();
            
            // If user specified a rating range, check it
            if let Some((min, max)) = user_entry.rating_range {
                if entry.rating < min || entry.rating > max {
                    return false;
                }
            }
            
            // If opponent specified a rating range, check it
            if let Some((min, max)) = entry.rating_range {
                if user_entry.rating < min || user_entry.rating > max {
                    return false;
                }
            }
            
            // Default: accept if within 200 rating points
            rating_diff <= 200
        })?;
        
        // Remove both players from queue
        let opponent = if opponent_index < user_index {
            let opponent = queue.remove(opponent_index);
            queue.remove(user_index - 1);
            opponent
        } else {
            queue.remove(user_index);
            queue.remove(opponent_index - 1)
        };
        
        self.user_in_queue.remove(&user_id);
        self.user_in_queue.remove(&opponent.user_id);
        
        Some(opponent)
    }
    
    pub async fn get_queue_status(&self) -> (usize, Option<u32>) {
        let queue = self.queue.lock().await;
        let queue_size = queue.len();
        
        // Simple estimate: average wait time increases with queue size
        let estimated_wait = if queue_size == 0 {
            None
        } else if queue_size < 10 {
            Some(30) // 30 seconds
        } else if queue_size < 50 {
            Some(15) // 15 seconds  
        } else {
            Some(5) // 5 seconds
        };
        
        (queue_size, estimated_wait)
    }
    
    pub fn is_in_queue(&self, user_id: &Uuid) -> bool {
        self.user_in_queue.contains_key(user_id)
    }
}
