-- Migration 0007: where a finished download lives on disk (ADR 0016).
-- Set once the bytes are in the user's download folder; the blob is then gone.
ALTER TABLE download ADD COLUMN file_path TEXT;
