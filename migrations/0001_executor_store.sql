-- Alosh Engineering 0.9.0: durable executor snapshot schema.
-- u64 and i128 values are decimal TEXT: SQLite INTEGER is signed 64-bit only.
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS schema_migrations (
 version INTEGER PRIMARY KEY, name TEXT NOT NULL,
 applied_at_utc TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS app_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS io_tasks (
 task_id TEXT PRIMARY KEY NOT NULL,
 path TEXT NOT NULL CHECK(length(path)>0),
 volume_guid TEXT NOT NULL CHECK(length(volume_guid)>0),
 state TEXT NOT NULL CHECK(state IN ('Pending','Leased','Retryable','Deferred','NeedsRevalidation','Completed','Failed')),
 source_size_bytes TEXT NOT NULL CHECK(length(source_size_bytes)>0 AND source_size_bytes NOT GLOB '*[^0-9]*'),
 source_modified_time_ns TEXT NOT NULL CHECK(length(source_modified_time_ns)>0),
 source_file_id BLOB,
 attempt INTEGER NOT NULL CHECK(attempt>=0),
 lease_token TEXT CHECK(lease_token IS NULL OR (length(lease_token)>0 AND lease_token NOT GLOB '*[^0-9]*')),
 lease_deadline_ms TEXT CHECK(lease_deadline_ms IS NULL OR (length(lease_deadline_ms)>0 AND lease_deadline_ms NOT GLOB '*[^0-9]*')),
 lease_topology_generation TEXT CHECK(lease_topology_generation IS NULL OR (length(lease_topology_generation)>0 AND lease_topology_generation NOT GLOB '*[^0-9]*')),
 last_failure TEXT CHECK(last_failure IS NULL OR last_failure IN ('AccessDenied','SharingViolation','DeviceUnavailable','SourceChanged','ReadError','ProcessorError','InvalidCheckpoint','LeaseExpired','ExplicitFailure')),
 CHECK ((state='Leased' AND lease_token IS NOT NULL AND lease_deadline_ms IS NOT NULL AND lease_topology_generation IS NOT NULL)
     OR (state<>'Leased' AND lease_token IS NULL AND lease_deadline_ms IS NULL AND lease_topology_generation IS NULL))
);
CREATE TABLE IF NOT EXISTS io_checkpoints (
 task_id TEXT PRIMARY KEY NOT NULL REFERENCES io_tasks(task_id) ON DELETE CASCADE,
 bytes_processed TEXT NOT NULL CHECK(length(bytes_processed)>0 AND bytes_processed NOT GLOB '*[^0-9]*'),
 processor_state BLOB NOT NULL,
 source_size_bytes TEXT NOT NULL CHECK(length(source_size_bytes)>0 AND source_size_bytes NOT GLOB '*[^0-9]*'),
 source_modified_time_ns TEXT NOT NULL CHECK(length(source_modified_time_ns)>0),
 source_file_id BLOB
);
CREATE INDEX IF NOT EXISTS idx_io_tasks_claim ON io_tasks(state,volume_guid,attempt,task_id);
CREATE INDEX IF NOT EXISTS idx_io_tasks_volume ON io_tasks(volume_guid,state);
INSERT OR IGNORE INTO app_meta(key,value) VALUES('executor_next_token','1');
INSERT OR IGNORE INTO schema_migrations(version,name) VALUES(1,'executor_store');
