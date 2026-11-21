
//! Durable SQLite adapter for `ResumableIoExecutor` snapshots.
//!
//! A snapshot is reconciled under one IMMEDIATE transaction. Unchanged rows are
//! not rewritten; unsigned 64-bit and signed 128-bit values use decimal TEXT.

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::HashSet;
use std::path::Path;

use crate::resumable_executor::{
    ExecutorSnapshot, ExecutorSnapshotStore, IoTaskSpec, SourceIdentity, TaskCheckpoint,
    TaskFailure, TaskRecord, TaskState,
};

const MIGRATION: &str = include_str!("../migrations/0001_executor_store.sql");

#[derive(Debug)]
pub struct SqliteSnapshotStore {
    connection: Connection,
}

impl SqliteSnapshotStore {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn from_connection(mut connection: Connection) -> rusqlite::Result<Self> {
        // Foreign-key enforcement must be enabled before opening the migration
        // transaction; SQLite ignores attempts to change it inside a transaction.
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;

        {
            let tx = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(MIGRATION)?;
            tx.commit()?;
        }

        let store = Self { connection };
        store.integrity_check()?;
        Ok(store)
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn integrity_check(&self) -> rusqlite::Result<()> {
        let result: String =
            self.connection
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;

        if result != "ok" {
            return Err(rusqlite::Error::InvalidQuery);
        }

        // PRAGMA integrity_check does not report foreign-key violations.
        // Check them separately so an externally modified DB cannot appear
        // healthy while checkpoints reference missing tasks.
        let mut statement = self.connection.prepare("PRAGMA foreign_key_check")?;
        let mut rows = statement.query([])?;
        if rows.next()?.is_some() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(())
    }

    fn read_snapshot(connection: &Connection) -> rusqlite::Result<Option<ExecutorSnapshot>> {
        let token_text: Option<String> = connection
            .query_row(
                "SELECT value FROM app_meta WHERE key='executor_next_token'",
                [],
                |row| row.get(0),
            )
            .optional()?;

        // The migration always seeds this key. Missing metadata is corruption,
        // not an empty database: fail closed rather than silently losing tasks.
        let Some(token_text) = token_text else {
            return Err(rusqlite::Error::InvalidQuery);
        };
        let next_token = parse_u64(&token_text)?;

        // One ordered LEFT JOIN reads tasks and their optional checkpoints in a
        // single SQLite statement. This avoids N+1 queries and keeps the whole
        // result inside the caller's read transaction.
        let mut stmt = connection.prepare(
            "SELECT t.task_id,t.path,t.volume_guid,t.state,\
                    t.source_size_bytes,t.source_modified_time_ns,t.source_file_id,\
                    t.attempt,t.lease_token,t.lease_deadline_ms,\
                    t.lease_topology_generation,t.last_failure,\
                    c.bytes_processed,c.processor_state,c.source_size_bytes,\
                    c.source_modified_time_ns,c.source_file_id \
             FROM io_tasks AS t \
             LEFT JOIN io_checkpoints AS c ON c.task_id=t.task_id \
             ORDER BY t.task_id",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<Vec<u8>>>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
                row.get::<_, Option<Vec<u8>>>(13)?,
                row.get::<_, Option<String>>(14)?,
                row.get::<_, Option<String>>(15)?,
                row.get::<_, Option<Vec<u8>>>(16)?,
            ))
        })?;

        let mut records = Vec::new();
        for row in rows {
            let (
                task_id, path, volume_guid, state, size, modified, file_id, attempt,
                lease_token, lease_deadline, lease_generation, failure,
                cp_bytes, cp_processor_state, cp_size, cp_modified, cp_file_id,
            ) = row?;

            let checkpoint = match cp_bytes {
                Some(bytes) => Some(TaskCheckpoint {
                    bytes_processed: parse_u64(&bytes)?,
                    processor_state: cp_processor_state.ok_or(rusqlite::Error::InvalidQuery)?,
                    source_identity: SourceIdentity {
                        size_bytes: parse_u64(&cp_size.ok_or(rusqlite::Error::InvalidQuery)?)?,
                        modified_time_ns: parse_i128(
                            &cp_modified.ok_or(rusqlite::Error::InvalidQuery)?,
                        )?,
                        file_id: cp_file_id,
                    },
                }),
                None => {
                    // A partial LEFT JOIN payload indicates malformed/corrupt
                    // checkpoint storage; never silently treat it as absent.
                    if cp_processor_state.is_some() || cp_size.is_some()
                        || cp_modified.is_some() || cp_file_id.is_some()
                    {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                    None
                }
            };

            records.push(TaskRecord {
                spec: IoTaskSpec {
                    task_id,
                    path,
                    volume_guid,
                    source_identity: SourceIdentity {
                        size_bytes: parse_u64(&size)?,
                        modified_time_ns: parse_i128(&modified)?,
                        file_id,
                    },
                },
                state: parse_state(&state)?,
                checkpoint,
                attempt: u32::try_from(attempt).map_err(|_| {
                    rusqlite::Error::IntegralValueOutOfRange(7, attempt)
                })?,
                lease_token: lease_token.as_deref().map(parse_u64).transpose()?,
                lease_deadline_ms: lease_deadline.as_deref().map(parse_u64).transpose()?,
                lease_topology_generation: lease_generation
                    .as_deref().map(parse_u64).transpose()?,
                last_failure: failure.as_deref().map(parse_failure).transpose()?,
            });
        }

        Ok(Some(ExecutorSnapshot { records, next_token }))
    }

    fn write_snapshot(
        tx: &Transaction<'_>,
        snapshot: &ExecutorSnapshot,
    ) -> rusqlite::Result<()> {
        if snapshot.next_token == 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "executor_next_token must be non-zero".into(),
            ));
        }

        let mut max_lease_token = 0u64;
        let mut normalized_volume_paths = Vec::with_capacity(snapshot.records.len());
        let mut seen_task_ids = HashSet::with_capacity(snapshot.records.len());

        for record in &snapshot.records {
            if !seen_task_ids.insert(record.spec.task_id.as_str()) {
                return Err(rusqlite::Error::InvalidParameterName(
                    "snapshot contains duplicate task IDs".into(),
                ));
            }
            if record.spec.task_id.trim().is_empty()
                || record.spec.task_id.len() > 512
                || crate::validate_filesystem_path(&record.spec.path).is_err()
            {
                return Err(rusqlite::Error::InvalidParameterName(
                    "invalid task identity".into(),
                ));
            }

            let normalized_volume =
                crate::volume_inventory::normalize_volume_guid_path(&record.spec.volume_guid)
                    .ok_or_else(|| {
                        rusqlite::Error::InvalidParameterName("invalid volume GUID".into())
                    })?;

            normalized_volume_paths.push(normalized_volume);

            if record.state == TaskState::Leased {
                let token = record.lease_token.ok_or_else(|| {
                    rusqlite::Error::InvalidParameterName(
                        "leased task is missing lease token".into(),
                    )
                })?;

                if token == 0
                    || record.lease_deadline_ms.is_none()
                    || record.lease_topology_generation.is_none()
                {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "leased task has incomplete lease fencing data".into(),
                    ));
                }

                max_lease_token = max_lease_token.max(token);
            } else if record.lease_token.is_some()
                || record.lease_deadline_ms.is_some()
                || record.lease_topology_generation.is_some()
            {
                return Err(rusqlite::Error::InvalidParameterName(
                    "non-leased task contains stale lease data".into(),
                ));
            }

            if let Some(checkpoint) = &record.checkpoint {
                if checkpoint.source_identity != record.spec.source_identity
                    || checkpoint.bytes_processed > record.spec.source_identity.size_bytes
                {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "checkpoint does not match source identity or exceeds source size"
                            .into(),
                    ));
                }
            }

            if record.state == TaskState::Completed
                && record.spec.source_identity.size_bytes > 0
                && record.checkpoint.as_ref().map_or(true, |checkpoint| {
                    checkpoint.bytes_processed != record.spec.source_identity.size_bytes
                })
            {
                return Err(rusqlite::Error::InvalidParameterName(
                    "completed task lacks a complete checkpoint".into(),
                ));
            }

            // Canonical paths are retained in order and written below.
        }

        if snapshot.next_token <= max_lease_token {
            return Err(rusqlite::Error::InvalidParameterName(
                "executor_next_token must exceed every active lease token".into(),
            ));
        }

        tx.execute(
            "INSERT INTO app_meta(key,value) VALUES('executor_next_token',?1) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value \
             WHERE app_meta.value IS NOT excluded.value",
            [snapshot.next_token.to_string()],
        )?;

        // A transaction-local staging table identifies the exact membership of
        // this snapshot. Existing task rows are upserted in place; unchanged
        // rows are not rewritten. Rows absent from the new snapshot are deleted
        // only after all new/updated rows have been accepted.
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS snapshot_task_ids (\
                 task_id TEXT PRIMARY KEY NOT NULL\
             );\
             DELETE FROM snapshot_task_ids;",
        )?;

        // Reuse prepared statements across the snapshot instead of reparsing
        // SQL for every task in a large queue.
        let mut stage_task = tx.prepare_cached(
            "INSERT INTO snapshot_task_ids(task_id) VALUES(?1)",
        )?;
        let mut upsert_task = tx.prepare_cached(
            "INSERT INTO io_tasks(\
                task_id,path,volume_guid,state,source_size_bytes,\
                source_modified_time_ns,source_file_id,attempt,lease_token,\
                lease_deadline_ms,lease_topology_generation,last_failure\
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) \
             ON CONFLICT(task_id) DO UPDATE SET \
                path=excluded.path,volume_guid=excluded.volume_guid,\
                state=excluded.state,source_size_bytes=excluded.source_size_bytes,\
                source_modified_time_ns=excluded.source_modified_time_ns,\
                source_file_id=excluded.source_file_id,attempt=excluded.attempt,\
                lease_token=excluded.lease_token,lease_deadline_ms=excluded.lease_deadline_ms,\
                lease_topology_generation=excluded.lease_topology_generation,\
                last_failure=excluded.last_failure \
             WHERE io_tasks.path IS NOT excluded.path \
                OR io_tasks.volume_guid IS NOT excluded.volume_guid \
                OR io_tasks.state IS NOT excluded.state \
                OR io_tasks.source_size_bytes IS NOT excluded.source_size_bytes \
                OR io_tasks.source_modified_time_ns IS NOT excluded.source_modified_time_ns \
                OR io_tasks.source_file_id IS NOT excluded.source_file_id \
                OR io_tasks.attempt IS NOT excluded.attempt \
                OR io_tasks.lease_token IS NOT excluded.lease_token \
                OR io_tasks.lease_deadline_ms IS NOT excluded.lease_deadline_ms \
                OR io_tasks.lease_topology_generation IS NOT excluded.lease_topology_generation \
                OR io_tasks.last_failure IS NOT excluded.last_failure",
        )?;
        let mut upsert_checkpoint = tx.prepare_cached(
            "INSERT INTO io_checkpoints(\
                task_id,bytes_processed,processor_state,source_size_bytes,\
                source_modified_time_ns,source_file_id\
             ) VALUES(?1,?2,?3,?4,?5,?6) \
             ON CONFLICT(task_id) DO UPDATE SET \
                bytes_processed=excluded.bytes_processed,\
                processor_state=excluded.processor_state,\
                source_size_bytes=excluded.source_size_bytes,\
                source_modified_time_ns=excluded.source_modified_time_ns,\
                source_file_id=excluded.source_file_id \
             WHERE io_checkpoints.bytes_processed IS NOT excluded.bytes_processed \
                OR io_checkpoints.processor_state IS NOT excluded.processor_state \
                OR io_checkpoints.source_size_bytes IS NOT excluded.source_size_bytes \
                OR io_checkpoints.source_modified_time_ns IS NOT excluded.source_modified_time_ns \
                OR io_checkpoints.source_file_id IS NOT excluded.source_file_id",
        )?;
        let mut delete_checkpoint = tx.prepare_cached(
            "DELETE FROM io_checkpoints WHERE task_id=?1",
        )?;

        for (record, normalized_volume) in snapshot
            .records
            .iter()
            .zip(normalized_volume_paths.iter())
        {
            stage_task.execute([&record.spec.task_id])?;

            let (token, deadline, generation) = if record.state == TaskState::Leased {
                (
                    record.lease_token.map(|value| value.to_string()),
                    record.lease_deadline_ms.map(|value| value.to_string()),
                    record.lease_topology_generation.map(|value| value.to_string()),
                )
            } else {
                (None, None, None)
            };

            upsert_task.execute(
                params![
                    record.spec.task_id,
                    record.spec.path,
                    normalized_volume,
                    state_name(record.state),
                    record.spec.source_identity.size_bytes.to_string(),
                    record.spec.source_identity.modified_time_ns.to_string(),
                    record.spec.source_identity.file_id,
                    i64::from(record.attempt),
                    token,
                    deadline,
                    generation,
                    record.last_failure.map(failure_name),
                ],
            )?;

            if let Some(checkpoint) = &record.checkpoint {
                upsert_checkpoint.execute(
                    params![
                        record.spec.task_id,
                        checkpoint.bytes_processed.to_string(),
                        checkpoint.processor_state,
                        checkpoint.source_identity.size_bytes.to_string(),
                        checkpoint.source_identity.modified_time_ns.to_string(),
                        checkpoint.source_identity.file_id,
                    ],
                )?;
            } else {
                delete_checkpoint.execute([&record.spec.task_id])?;
            }
        }

        tx.execute(
            "DELETE FROM io_tasks \
             WHERE task_id NOT IN (SELECT task_id FROM snapshot_task_ids)",
            [],
        )?;

        Ok(())
    }
}

impl ExecutorSnapshotStore for SqliteSnapshotStore {
    type Error = rusqlite::Error;

    fn load_snapshot(&mut self) -> rusqlite::Result<Option<ExecutorSnapshot>> {
        // Keep metadata, tasks, and checkpoints on one SQLite read snapshot even
        // when another process/connection commits a writer transaction concurrently.
        let tx = self.connection.transaction()?;
        let snapshot = Self::read_snapshot(&tx)?;
        tx.commit()?;
        Ok(snapshot)
    }

    fn save_snapshot_transactional(
        &mut self,
        snapshot: &ExecutorSnapshot,
    ) -> rusqlite::Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        Self::write_snapshot(&tx, snapshot)?;
        tx.commit()
    }
}

fn parse_u64(value: &str) -> rusqlite::Result<u64> {
    value.parse().map_err(|_| {
        rusqlite::Error::InvalidColumnType(
            0,
            "decimal u64".into(),
            rusqlite::types::Type::Text,
        )
    })
}

fn parse_i128(value: &str) -> rusqlite::Result<i128> {
    value.parse().map_err(|_| {
        rusqlite::Error::InvalidColumnType(
            0,
            "decimal i128".into(),
            rusqlite::types::Type::Text,
        )
    })
}

fn parse_state(value: &str) -> rusqlite::Result<TaskState> {
    Ok(match value {
        "Pending" => TaskState::Pending,
        "Leased" => TaskState::Leased,
        "Retryable" => TaskState::Retryable,
        "Deferred" => TaskState::Deferred,
        "NeedsRevalidation" => TaskState::NeedsRevalidation,
        "Completed" => TaskState::Completed,
        "Failed" => TaskState::Failed,
        _ => return Err(rusqlite::Error::InvalidQuery),
    })
}

fn state_name(state: TaskState) -> &'static str {
    match state {
        TaskState::Pending => "Pending",
        TaskState::Leased => "Leased",
        TaskState::Retryable => "Retryable",
        TaskState::Deferred => "Deferred",
        TaskState::NeedsRevalidation => "NeedsRevalidation",
        TaskState::Completed => "Completed",
        TaskState::Failed => "Failed",
    }
}

fn parse_failure(value: &str) -> rusqlite::Result<TaskFailure> {
    Ok(match value {
        "AccessDenied" => TaskFailure::AccessDenied,
        "SharingViolation" => TaskFailure::SharingViolation,
        "DeviceUnavailable" => TaskFailure::DeviceUnavailable,
        "SourceChanged" => TaskFailure::SourceChanged,
        "ReadError" => TaskFailure::ReadError,
        "ProcessorError" => TaskFailure::ProcessorError,
        "InvalidCheckpoint" => TaskFailure::InvalidCheckpoint,
        "LeaseExpired" => TaskFailure::LeaseExpired,
        "ExplicitFailure" => TaskFailure::ExplicitFailure,
        _ => return Err(rusqlite::Error::InvalidQuery),
    })
}

fn failure_name(failure: TaskFailure) -> &'static str {
    match failure {
        TaskFailure::AccessDenied => "AccessDenied",
        TaskFailure::SharingViolation => "SharingViolation",
        TaskFailure::DeviceUnavailable => "DeviceUnavailable",
        TaskFailure::SourceChanged => "SourceChanged",
        TaskFailure::ReadError => "ReadError",
        TaskFailure::ProcessorError => "ProcessorError",
        TaskFailure::InvalidCheckpoint => "InvalidCheckpoint",
        TaskFailure::LeaseExpired => "LeaseExpired",
        TaskFailure::ExplicitFailure => "ExplicitFailure",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending_record(task_id: &str) -> TaskRecord {
        TaskRecord {
            spec: IoTaskSpec {
                task_id: task_id.to_owned(),
                path: format!(r"C:\data\{task_id}.bin"),
                volume_guid: r"\\?\Volume{01234567-89AB-CDEF-0123-456789ABCDEF}".into(),
                source_identity: SourceIdentity {
                    size_bytes: 10,
                    modified_time_ns: 20,
                    file_id: Some(vec![1, 2, 3]),
                },
            },
            state: TaskState::Pending,
            checkpoint: None,
            attempt: 0,
            lease_token: None,
            lease_deadline_ms: None,
            lease_topology_generation: None,
            last_failure: None,
        }
    }

    #[test]
    fn unchanged_snapshot_does_not_rewrite_existing_task_rows() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();
        let snapshot = ExecutorSnapshot {
            records: vec![pending_record("stable-a"), pending_record("stable-b")],
            next_token: 1,
        };
        store.save_snapshot_transactional(&snapshot).unwrap();
        store.connection.execute_batch(
            "CREATE TABLE write_audit(kind TEXT NOT NULL);\
             CREATE TRIGGER count_task_updates AFTER UPDATE ON io_tasks BEGIN\
                 INSERT INTO write_audit(kind) VALUES('task-update');\
             END;\
             CREATE TRIGGER count_task_deletes AFTER DELETE ON io_tasks BEGIN\
                 INSERT INTO write_audit(kind) VALUES('task-delete');\
             END;",
        ).unwrap();

        store.save_snapshot_transactional(&snapshot).unwrap();

        let writes: i64 = store.connection.query_row(
            "SELECT COUNT(*) FROM write_audit", [], |row| row.get(0),
        ).unwrap();
        assert_eq!(writes, 0, "an identical snapshot must not rewrite task rows");
        assert_eq!(store.load_snapshot().unwrap(), Some(snapshot));
    }

    #[test]
    fn incremental_snapshot_updates_adds_and_removes_tasks_atomically() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();
        let initial = ExecutorSnapshot {
            records: vec![pending_record("keep"), pending_record("remove")],
            next_token: 1,
        };
        store.save_snapshot_transactional(&initial).unwrap();

        let mut changed = pending_record("keep");
        changed.attempt = 3;
        let next = ExecutorSnapshot {
            records: vec![changed, pending_record("add")],
            next_token: 1,
        };
        store.save_snapshot_transactional(&next).unwrap();

        assert_eq!(store.load_snapshot().unwrap(), Some(next));
        let count: i64 = store.connection.query_row(
            "SELECT COUNT(*) FROM io_tasks", [], |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 2);
        let removed: i64 = store.connection.query_row(
            "SELECT COUNT(*) FROM io_tasks WHERE task_id='remove'", [], |row| row.get(0),
        ).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn failed_incremental_write_rolls_back_all_prior_changes() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();
        let initial = ExecutorSnapshot {
            records: vec![pending_record("stable")],
            next_token: 1,
        };
        store.save_snapshot_transactional(&initial).unwrap();
        store.connection.execute_batch(
            "CREATE TRIGGER reject_exploding_task BEFORE INSERT ON io_tasks             WHEN NEW.task_id='explode' BEGIN SELECT RAISE(ABORT,'injected failure'); END;",
        ).unwrap();

        let mut changed = pending_record("stable");
        changed.attempt = 7;
        let attempted = ExecutorSnapshot {
            records: vec![changed, pending_record("explode")],
            next_token: 2,
        };
        assert!(store.save_snapshot_transactional(&attempted).is_err());
        assert_eq!(store.load_snapshot().unwrap(), Some(initial));
    }

    #[test]
    fn opens_migrates_and_checks_integrity() {
        let store = SqliteSnapshotStore::open_in_memory().unwrap();
        store.integrity_check().unwrap();
    }

    #[test]
    fn integrity_check_detects_foreign_key_corruption() {
        let store = SqliteSnapshotStore::open_in_memory().unwrap();
        store.connection.pragma_update(None, "foreign_keys", "OFF").unwrap();
        store.connection.execute(
            "INSERT INTO io_checkpoints(task_id,bytes_processed,processor_state,source_size_bytes,source_modified_time_ns,source_file_id) VALUES('orphan','0',X'', '0','0',NULL)",
            [],
        ).unwrap();
        store.connection.pragma_update(None, "foreign_keys", "ON").unwrap();
        assert!(store.integrity_check().is_err());
    }

    #[test]
    fn empty_snapshot_round_trips_token() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();

        let snapshot = ExecutorSnapshot {
            records: vec![],
            next_token: u64::MAX,
        };

        store.save_snapshot_transactional(&snapshot).unwrap();
        assert_eq!(store.load_snapshot().unwrap(), Some(snapshot));
    }

    #[test]
    fn missing_executor_token_fails_closed_instead_of_looking_empty() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();

        store
            .connection
            .execute(
                "DELETE FROM app_meta WHERE key='executor_next_token'",
                [],
            )
            .unwrap();

        assert!(store.load_snapshot().is_err());
    }

    #[test]
    fn refuses_checkpoint_with_different_source_identity() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();

        let identity = SourceIdentity {
            size_bytes: 10,
            modified_time_ns: 20,
            file_id: Some(vec![1, 2, 3]),
        };

        let other_identity = SourceIdentity {
            size_bytes: 10,
            modified_time_ns: 21,
            file_id: Some(vec![1, 2, 3]),
        };

        let record = TaskRecord {
            spec: IoTaskSpec {
                task_id: "task-mismatch".into(),
                path: r"C:\data\file.bin".into(),
                volume_guid: r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\".into(),
                source_identity: identity,
            },
            state: TaskState::Retryable,
            checkpoint: Some(TaskCheckpoint {
                bytes_processed: 5,
                processor_state: vec![1],
                source_identity: other_identity,
            }),
            attempt: 1,
            lease_token: None,
            lease_deadline_ms: None,
            lease_topology_generation: None,
            last_failure: Some(TaskFailure::SourceChanged),
        };

        let snapshot = ExecutorSnapshot {
            records: vec![record],
            next_token: 2,
        };

        assert!(store.save_snapshot_transactional(&snapshot).is_err());
        assert_eq!(store.load_snapshot().unwrap().unwrap().records.len(), 0);
    }

    #[test]
    fn persists_volume_guid_in_canonical_form() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();

        let identity = SourceIdentity {
            size_bytes: 10,
            modified_time_ns: 20,
            file_id: None,
        };

        let record = TaskRecord {
            spec: IoTaskSpec {
                task_id: "canonical-volume".into(),
                path: r"C:\data\file.bin".into(),

                // Intentionally non-canonical input: the store must normalize it.
                volume_guid: r"\\?\volume{01234567-89ab-cdef-0123-456789abcdef}\".into(),

                source_identity: identity,
            },
            state: TaskState::Pending,
            checkpoint: None,
            attempt: 0,
            lease_token: None,
            lease_deadline_ms: None,
            lease_topology_generation: None,
            last_failure: None,
        };

        store
            .save_snapshot_transactional(&ExecutorSnapshot {
                records: vec![record],
                next_token: 1,
            })
            .unwrap();

        let persisted: String = store
            .connection
            .query_row(
                "SELECT volume_guid FROM io_tasks WHERE task_id='canonical-volume'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(
            persisted,
            r"\\?\Volume{01234567-89AB-CDEF-0123-456789ABCDEF}"
        );
    }

    #[test]
    fn leased_task_round_trips_fencing_metadata_and_token_sequence() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();

        let identity = SourceIdentity {
            size_bytes: 10,
            modified_time_ns: 20,
            file_id: Some(vec![4, 5]),
        };

        let record = TaskRecord {
            spec: IoTaskSpec {
                task_id: "leased-task".into(),
                path: r"C:\data\file.bin".into(),

                // Use the canonical form because the round-trip comparison
                // checks exact snapshot equality after persistence.
                volume_guid: r"\\?\Volume{01234567-89AB-CDEF-0123-456789ABCDEF}".into(),

                source_identity: identity,
            },
            state: TaskState::Leased,
            checkpoint: None,
            attempt: 1,
            lease_token: Some(1),
            lease_deadline_ms: Some(900),
            lease_topology_generation: Some(7),
            last_failure: None,
        };

        let snapshot = ExecutorSnapshot {
            records: vec![record],
            next_token: 2,
        };

        store.save_snapshot_transactional(&snapshot).unwrap();
        assert_eq!(store.load_snapshot().unwrap(), Some(snapshot));
    }

    #[test]
    fn rejects_stale_lease_fields_on_non_leased_task() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();

        let identity = SourceIdentity {
            size_bytes: 10,
            modified_time_ns: 20,
            file_id: None,
        };

        let record = TaskRecord {
            spec: IoTaskSpec {
                task_id: "stale-lease".into(),
                path: r"C:\data\file.bin".into(),
                volume_guid: r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\".into(),
                source_identity: identity,
            },
            state: TaskState::Retryable,
            checkpoint: None,
            attempt: 0,
            lease_token: Some(1),
            lease_deadline_ms: None,
            lease_topology_generation: None,
            last_failure: None,
        };

        assert!(
            store
                .save_snapshot_transactional(&ExecutorSnapshot {
                    records: vec![record],
                    next_token: 2,
                })
                .is_err()
        );
    }

    #[test]
    fn rejects_completed_task_without_full_checkpoint() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();

        let identity = SourceIdentity {
            size_bytes: 10,
            modified_time_ns: 20,
            file_id: None,
        };

        let record = TaskRecord {
            spec: IoTaskSpec {
                task_id: "incomplete-completed".into(),
                path: r"C:\data\file.bin".into(),
                volume_guid: r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\".into(),
                source_identity: identity.clone(),
            },
            state: TaskState::Completed,
            checkpoint: Some(TaskCheckpoint {
                bytes_processed: 5,
                processor_state: vec![],
                source_identity: identity,
            }),
            attempt: 0,
            lease_token: None,
            lease_deadline_ms: None,
            lease_topology_generation: None,
            last_failure: None,
        };

        assert!(
            store
                .save_snapshot_transactional(&ExecutorSnapshot {
                    records: vec![record],
                    next_token: 1,
                })
                .is_err()
        );
    }

    #[test]
    fn complete_task_and_checkpoint_round_trip_full_integer_ranges() {
        let mut store = SqliteSnapshotStore::open_in_memory().unwrap();

        let identity = SourceIdentity {
            size_bytes: u64::MAX,
            modified_time_ns: i128::MIN,
            file_id: Some(vec![0, 1, 255]),
        };

        let record = TaskRecord {
            spec: IoTaskSpec {
                task_id: "task-1".into(),
                path: r"C:\data\file.bin".into(),

                // Use the canonical form because the round-trip comparison
                // checks exact snapshot equality after persistence.
                volume_guid: r"\\?\Volume{01234567-89AB-CDEF-0123-456789ABCDEF}".into(),

                source_identity: identity.clone(),
            },
            state: TaskState::Retryable,
            checkpoint: Some(TaskCheckpoint {
                bytes_processed: u64::MAX,
                processor_state: vec![9, 8, 7],
                source_identity: identity,
            }),
            attempt: u32::MAX,
            lease_token: None,
            lease_deadline_ms: None,
            lease_topology_generation: None,
            last_failure: Some(TaskFailure::ReadError),
        };

        let snapshot = ExecutorSnapshot {
            records: vec![record],
            next_token: u64::MAX,
        };

        store.save_snapshot_transactional(&snapshot).unwrap();
        assert_eq!(store.load_snapshot().unwrap(), Some(snapshot));
    }
}
