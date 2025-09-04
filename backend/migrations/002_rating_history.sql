-- Create rating history table to track rating changes
CREATE TABLE IF NOT EXISTS rating_history (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    user_id TEXT NOT NULL REFERENCES users(id),
    game_id TEXT NOT NULL REFERENCES games(id),
    rating_before INTEGER NOT NULL,
    rating_after INTEGER NOT NULL,
    rating_change INTEGER NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for efficient queries
CREATE INDEX IF NOT EXISTS idx_rating_history_user_id ON rating_history(user_id);
CREATE INDEX IF NOT EXISTS idx_rating_history_game_id ON rating_history(game_id);
CREATE INDEX IF NOT EXISTS idx_rating_history_created_at ON rating_history(created_at);
