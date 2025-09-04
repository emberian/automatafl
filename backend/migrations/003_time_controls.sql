-- Add time control fields to games table
ALTER TABLE games ADD COLUMN time_control_seconds INTEGER DEFAULT NULL;
ALTER TABLE games ADD COLUMN white_time_remaining_ms INTEGER DEFAULT NULL;
ALTER TABLE games ADD COLUMN black_time_remaining_ms INTEGER DEFAULT NULL;
ALTER TABLE games ADD COLUMN last_move_at DATETIME DEFAULT NULL;

-- Create index for finding abandoned games
CREATE INDEX IF NOT EXISTS idx_games_last_move_at ON games(last_move_at) WHERE status = 'active';
