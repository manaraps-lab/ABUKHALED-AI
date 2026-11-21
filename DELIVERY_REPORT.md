# Delivery Report — Alosh Engineering 0.9.1 (Reviewed Build Package)

## Source identity
- Historical delivery input: `Alosh_Engineering_0.9.1_SQLiteAdapter (1).zip`
- Historical input SHA-256: `c0c592555d2a71525f301b34163d8167f731c161cdb1ae4b609cda998db37eac`
- Archive received for the current follow-up review: `Alosh_Engineering-main.zip`
- Current received archive SHA-256: `b3f2ea959f04607c99195a478a0c8d0e5b804f23636d47b95f18f4fc6359d8af`
- The current archive hash differs from the historical delivery hash; they are not asserted to be byte-identical.
- Current input ZIP integrity: passed.

## Changes in this revision
- Added `.github/workflows/main.yml` for Ubuntu and Windows.
- Added `rust-toolchain.toml`, `.gitignore`, `BUILDING.md`, and MIT `LICENSE`.
- Updated README, changelog, project status, and integration matrix to describe the actual 0.9.1 scope and outstanding release gates.
- Canonicalized Volume GUID values before persisting them.
- Strengthened snapshot validation for stale/incomplete lease metadata, lease token sequence fencing, and incomplete completed-task checkpoints.
- Added Rust regression tests for canonical Volume GUID persistence, active-lease round-trip, stale lease rejection, and incomplete completed-task rejection.

## Checks performed in this environment
- Input ZIP integrity: PASS.
- Python SQLite schema/transaction tests: 8/8 PASS.
- `Cargo.toml` TOML parse and dependency/feature assertions: PASS.
- SQLite migration smoke test: PASS.
- GitHub Actions workflow YAML parse: PASS.
- Output ZIP integrity: verified after packaging (see final delivery message).

## Checks not performed
- `cargo check --all-targets`: NOT RUN (`cargo` is not installed here).
- `cargo test --all-targets`: NOT RUN (`cargo` is not installed here).
- `cargo build --release`: NOT RUN (`cargo` is not installed here).
- Rust formatting/compiler diagnostics: NOT RUN (`rustfmt`/`rustc` are not installed here).
- Windows API runtime tests and installer build: NOT RUN.
- GitHub Actions workflow: included but not yet executed.

## Build acceptance
This archive is prepared for repository upload and CI validation. It is **not yet compiler-verified**. After uploading, the required acceptance gate is a green `Rust validation` workflow on both Ubuntu and Windows. Do not treat the Rust adapter as production-approved before that gate passes.

## Scope boundary
This is the 0.9.1 Rust storage/topology/scheduler/resumable-executor library package. It is not the complete desktop product. The Tauri/React host, file reader/worker pool, SHA-256 processor, image/EXIF and face pipelines, and Windows installer remain outside this archive.


## Deep source review — follow-up findings

- The received archive in this review is `Alosh_Engineering-main.zip` (SHA-256: `b3f2ea959f04607c99195a478a0c8d0e5b804f23636d47b95f18f4fc6359d8af`). This differs from the historical input hash recorded above; the two archives must not be treated as byte-identical.
- Corrected the workflow path filters to reference the actual `.github/workflows/main.yml` file and include `tests/**`; added explicit Python setup for the schema suite.
- Fixed volume inventory construction and diff processing so malformed entries in constructor-created or manually constructed snapshots cannot produce false confirmed removals. The topology cache also rejects malformed confirmed diffs without advancing its generation. Whitespace is no longer silently trimmed from volume GUID inputs.
- Extended filesystem path validation to accept valid `\\?\C:\...` extended-length drive paths while continuing to reject device namespace paths and classify extended UNC paths as remote.
- Restricted volume extent queries to exact `\\.\X:` drive-device paths or syntactically valid Volume GUID paths; arbitrary `\\.\PhysicalDrive...` and path suffixes are rejected.
- Strengthened SQLite health checking to include `PRAGMA foreign_key_check`, since `PRAGMA integrity_check` alone does not detect foreign-key violations.
- Added regression tests for the above changes, including strict restore-state validation. The Python SQLite schema suite was run after the review changes; Rust tests and Windows-specific compilation/runtime tests remain unrun because this environment has no `cargo` or `rustc`.
- Identified a production-scale persistence risk: each snapshot save deletes/reinserts all task rows, and each load performs a separate checkpoint query per task. Incremental/batched persistence and scale benchmarks remain required before large-library use.


## Incremental persistence follow-up (prepared for CI validation)

- Replaced full task-table deletion/reinsertion with transactional reconciliation using a temporary membership table and conditional UPSERTs. Existing rows are updated only when one or more persisted fields differ; absent task IDs are deleted after all snapshot rows are staged and accepted.
- Checkpoints are conditionally upserted or removed in the same transaction. Task deletion continues to rely on the schema's `ON DELETE CASCADE` relationship.
- Replaced per-task checkpoint reads with one ordered `LEFT JOIN`, retaining the enclosing consistent read transaction and rejecting malformed partial checkpoint payloads.
- Reused cached prepared statements across the task set and added duplicate-task-ID validation before database writes.
- Added Rust regression tests for no-op saves, snapshot add/update/remove reconciliation, and rollback after an injected mid-transaction failure.
- Python SQLite schema suite: 8/8 PASS. A separate SQLite SQL simulation passed joined reads, conditional no-op/update UPSERT behavior, membership deletion, and rollback.
- Rust compiler, Rust tests, Windows runtime tests, and scale benchmarks: NOT RUN in this authoring environment (`cargo` and `rustc` are unavailable). This package must pass the repository's Windows and Ubuntu Actions jobs before acceptance.
