# Changelog

## 0.9.1 — SQLite Snapshot Adapter (build validation pending)
- Added a concrete `rusqlite` implementation of `ExecutorSnapshotStore`.
- Added transactional snapshot persistence and checkpoint persistence.
- Follow-up persistence optimization reconciles task membership, upserts only changed task/checkpoint rows, and avoids full-table delete/reinsert churn.
- Snapshot loading now uses one ordered LEFT JOIN instead of one checkpoint query per task.
- Reuses prepared SQLite statements across snapshot writes and rejects duplicate task IDs before touching persisted state.
- Added Rust regression tests for no-op saves, add/update/remove reconciliation, and rollback after an injected mid-transaction failure.
- Added snapshot loading, decimal integer decoding, connection setup, migration application, and SQLite integrity check.
- Pinned snapshot reads to one SQLite read transaction and made missing executor token metadata fail closed.
- Wrapped schema migration in one immediate transaction after enabling foreign-key enforcement.
- Canonicalized persisted Volume GUID paths rather than merely validating their input form.
- Rejects stale lease fields on non-leased tasks, incomplete lease fencing data, invalid token sequences, and completed tasks without a full checkpoint.
- Rejects checkpoints whose source identity differs from the task or whose byte offset exceeds source size.
- Added Rust regression tests for canonical Volume GUID persistence and snapshot invariants.
- Added stable toolchain selection, build instructions, ignore rules, MIT license text, and Windows/Ubuntu GitHub Actions validation.
- Follow-up review corrected CI path filters, made partial volume inventories fail closed, added extended-length path support, tightened volume-device path validation, checked SQLite foreign-key integrity, and hardened executor restore validation.
- Rust compilation and tests remain unverified in the authoring environment; CI must pass before release acceptance.

## 0.9.0 — Integration Baseline
- Preserved all 0.8.9 Rust source modules without claiming new Rust runtime behavior.
- Added a versioned SQLite migration for durable executor tasks, checkpoints, lease fencing fields, and the monotonic token sequence.
- Added architecture ownership/data-flow contracts and a component integration/verification matrix.
- Added executable Python SQLite schema/transaction tests.
- Explicitly documented that the Rust SQLite adapter, filesystem worker, SHA-256 processor, Tauri host, Windows build and offline installer remain unimplemented/unverified.

## 0.8.9 — Resumable I/O Executor
- Added topology-gated resumable task state machine, leases, fencing, checkpoints, and snapshot persistence interface.
