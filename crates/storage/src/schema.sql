PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
CREATE TABLE IF NOT EXISTS projects (
    project_id TEXT PRIMARY KEY, name TEXT NOT NULL, root TEXT NOT NULL UNIQUE,
    latest_local_snapshot TEXT, last_verified_at TEXT, verified_peer_id TEXT,
    verified_snapshot_id TEXT, mailbox_url TEXT, relay_address TEXT,
    error TEXT, recovery_required INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS devices (
    device_id TEXT PRIMARY KEY, name TEXT NOT NULL, peer_id TEXT NOT NULL UNIQUE, public_key TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS trusted_peers (
    device_id TEXT PRIMARY KEY REFERENCES devices(device_id), card_json TEXT NOT NULL,
    trust_status TEXT NOT NULL CHECK(trust_status IN ('TRUSTED','REVOKED'))
);
CREATE TABLE IF NOT EXISTS project_peers (
    project_id TEXT NOT NULL REFERENCES projects(project_id), device_id TEXT NOT NULL REFERENCES trusted_peers(device_id),
    PRIMARY KEY(project_id, device_id)
);
CREATE TABLE IF NOT EXISTS snapshots (
    snapshot_id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(project_id), parent_snapshot_id TEXT,
    author_device_id TEXT NOT NULL, created_at TEXT NOT NULL, manifest_hash TEXT NOT NULL,
    snapshot_json TEXT, announcement_json TEXT, envelope_json TEXT,
    location TEXT NOT NULL CHECK(location IN ('LOCAL','REMOTE')),
    metadata_status TEXT NOT NULL CHECK(metadata_status IN ('PENDING','PUBLISHED','REMOTE')),
    local_sequence INTEGER, total_size INTEGER NOT NULL, changed_bytes INTEGER NOT NULL,
    file_count INTEGER NOT NULL, author_name TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS snapshots_project ON snapshots(project_id, created_at);
CREATE TABLE IF NOT EXISTS snapshot_files (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(snapshot_id), relative_path TEXT NOT NULL,
    size INTEGER NOT NULL, blake3 TEXT NOT NULL, chunks_json TEXT NOT NULL,
    PRIMARY KEY(snapshot_id, relative_path)
);
CREATE TABLE IF NOT EXISTS chunks (
    blake3 TEXT PRIMARY KEY, size INTEGER NOT NULL, verified_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS snapshot_chunks (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(snapshot_id), blake3 TEXT NOT NULL,
    size INTEGER NOT NULL, PRIMARY KEY(snapshot_id, blake3)
);
CREATE TABLE IF NOT EXISTS transfer_sessions (
    transfer_id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(project_id), snapshot_id TEXT NOT NULL,
    source_device_id TEXT NOT NULL, state TEXT NOT NULL, completed_bytes INTEGER NOT NULL DEFAULT 0,
    total_bytes INTEGER NOT NULL, error TEXT, updated_at TEXT NOT NULL, snapshot_json TEXT NOT NULL,
    base_snapshot_id TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS transfer_snapshot ON transfer_sessions(project_id, snapshot_id);
CREATE TABLE IF NOT EXISTS transfer_chunks (
    transfer_id TEXT NOT NULL REFERENCES transfer_sessions(transfer_id), blake3 TEXT NOT NULL,
    size INTEGER NOT NULL, PRIMARY KEY(transfer_id, blake3)
);
CREATE TABLE IF NOT EXISTS mailbox_state (
    project_id TEXT PRIMARY KEY REFERENCES projects(project_id), cursor INTEGER NOT NULL DEFAULT 0
);
