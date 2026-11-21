# Project Status — Alosh Engineering 0.9.1 SQLite Adapter (incremental persistence follow-up)

## Delivered in source
- Retained the 0.9.0 integration baseline and prior storage/topology/scheduler/cache/executor modules.
- Added `src/sqlite_store.rs`, a concrete `rusqlite` implementation of `ExecutorSnapshotStore`.
- Adapter applies the versioned SQL migration, enables foreign keys, configures a bounded busy timeout, supports file-backed and in-memory connections, and exposes an integrity check.
- Snapshot writes use one `BEGIN IMMEDIATE` transaction; token sequence, task rows, and checkpoint rows commit atomically. The follow-up implementation stages snapshot membership, upserts task/checkpoint rows only when values differ, and deletes rows absent from the new snapshot.
- Snapshot writes reuse prepared statements across the task set and reject duplicate task IDs before persistence changes begin.
- Snapshot reads use one SQLite read transaction so metadata/tasks/checkpoints come from one consistent database snapshot. Task and checkpoint records are fetched with one ordered `LEFT JOIN`, eliminating the previous per-task checkpoint query pattern.
- Full-range `u64`/`i128` values are stored as decimal TEXT.
- Missing `executor_next_token` metadata fails closed.
- Persisted Volume GUID paths are normalized to canonical uppercase GUID form without a trailing separator.
- Follow-up review makes malformed volume enumeration entries and manually constructed malformed snapshots/diffs fail closed, accepts valid extended-length drive paths, validates volume-device query targets exactly, and includes foreign-key validation in SQLite health checks.
- Restore rejects malformed lease metadata on both leased and non-leased records instead of carrying contradictory state forward.
- Snapshot validation rejects mismatched/oversized checkpoints, incomplete or stale lease fields, token sequences that do not fence active leases, and completed tasks without a full checkpoint.
- Added GitHub Actions validation for Ubuntu and Windows, stable toolchain selection, build instructions, `.gitignore`, and license text.

## Verification actually performed for this delivery
- The archive supplied for this review has SHA-256 `b3f2ea959f04607c99195a478a0c8d0e5b804f23636d47b95f18f4fc6359d8af`; it differs from the historical 0.9.1 hash recorded in the prior delivery report.
- ZIP integrity test passed; no damaged archive entries were found.
- Python SQLite schema/transaction tests: 8/8 passed during the current archive review.
- Rust compile/tests, Windows API compilation/runtime, and release build: NOT RUN in this environment because `cargo`, `rustc`, and `rustfmt` are not installed.
- GitHub Actions workflow is included but has NOT run; its success is not assumed.

## Required build gate
Run `cargo check --all-targets`, `cargo test --all-targets`, and `cargo build --release` on a Rust-enabled host. The included workflow performs these on both Ubuntu and Windows. Treat 0.9.1 as unverified until both matrix jobs pass.

## Known scope limitations
- The adapter does not perform filesystem reads, hashing, or bypass topology/lease validation.
- Restart lease invalidation remains owned by `ResumableIoExecutor::restore`.
- The actual filesystem reader/worker pool, SHA-256 processor, Tauri host, React UI, FTS5 search integration, image/EXIF pipeline, face pipeline, and offline Windows installer remain unimplemented.
- The current snapshot adapter rewrites the complete task table on every save and performs one checkpoint query per task on load. Large-queue performance is unverified; before production-scale indexing, persistence should move to incremental/batched writes and batched reads with load/stress benchmarks.
- `rusqlite` bundles SQLite, but the Rust dependency graph is not yet vendored and this archive contains no `Cargo.lock`; first Cargo build may need network access or a populated Cargo cache, and transitive dependency resolution is not lockfile-pinned.
- No pretrained face weights are bundled. Every runtime/model/license asset must be audited before distribution.

## Safety invariants retained
- No automatic deletion; quarantine/restore remains a separate user-authorized workflow.
- Offline volumes are not interpreted as deleted content.
- Adaptive scheduling changes concurrency only, never indexing fidelity or file coverage.
- A byte offset is not a resumable SHA-256 state; unsupported processor state must restart safely.
