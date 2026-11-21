# Alosh Engineering — Unified Architecture Contracts (0.9.0)

## Purpose and status
This freezes integration boundaries for the existing 0.8.x storage components. Version 0.9.1 adds the concrete SQLite snapshot adapter; the desktop application, file reader, hashing engine, and Windows runtime integration remain outside this package.

## Single ownership boundaries
| Concern | Single owner | Contract |
|---|---|---|
| Filesystem path → volume GUID | Windows path resolver | Explicit status; never guess a physical disk from a drive letter. |
| Volume GUID → physical disk set | Volume topology probe | Preserve every extent; multi-disk volumes remain multi-disk. |
| Device media classification | Device profile | Unknown/unsupported stays unknown; no SSD assumption. |
| Current volume inventory | Volume inventory monitor | Unconfirmed enumeration is never interpreted as removal. |
| Confirmed topology cache | `TopologyCache` | Admit only confirmed, matching mapping/profile pairs. |
| Resource/worker decision | Adaptive I/O scheduler | Change scheduling only; never lower fidelity or skip files. |
| Task lifecycle and fencing | `ResumableIoExecutor` | Own task states, leases, checkpoints, retries and completion invariants. |
| Durable task snapshot | `SqliteSnapshotStore` | Save/load complete snapshot atomically through `ExecutorSnapshotStore`. |
| File bytes and digest | Future file processor (not implemented) | Revalidate identity; verify EOF; never infer completion from offset alone. |
| UI and user commands | Future Tauri host | UI does not own task state or mutate DB rows directly. |

## Required data flow
1. Accept only user-selected indexing roots.
2. Resolve each candidate path to a volume GUID. Remote, inaccessible, unavailable and unsupported paths remain explicit non-success outcomes.
3. Obtain source identity (size, high-resolution modification time, optional stable file ID) and enqueue an `IoTaskSpec`.
4. Require confirmed, internally consistent topology before `claim_next` can lease work.
5. Apply scheduler global worker limit; executor continues enforcing per-volume limits.
6. Worker reads the file and revalidates identity before trusting a checkpoint and before completion.
7. Persist task state, offset, source identity, processor state, lease metadata and next-token sequence in one SQLite transaction.
8. Complete only after explicit EOF verification and final identity checks. Access errors and missing devices never become success or silent deletion.

## SQLite adapter transaction contract (implemented in `src/sqlite_store.rs`)
- Use one connection owner or serialized write queue.
- Full snapshot save: `BEGIN IMMEDIATE`; update `app_meta.executor_next_token`; replace task/checkpoint rows; commit only after every write succeeds. Roll back the whole snapshot on any error.
- Enable `PRAGMA foreign_keys=ON` on every connection. Checkpoints reference tasks with `ON DELETE CASCADE`.
- Store Rust `u64` and `i128` as canonical decimal text: SQLite INTEGER is signed 64-bit and cannot represent their full ranges.
- Store file IDs and opaque processor state as BLOB. Processor state must be versioned; unknown versions require safe processor restart, not reinterpretation.
- Persist `executor_next_token` even with no active lease; it must remain greater than every token ever issued in the restored snapshot.
- `ResumableIoExecutor::restore` invalidates in-flight leases. Rebuild/reconfirm topology before scheduling after restart.
- `NeedsRevalidation` is durable and cannot be claimed until fresh identity is obtained and `refresh_source_identity` is explicitly called.
- Offset and processor state are one atomic checkpoint. Never use `INSERT OR REPLACE` on parent task rows: SQLite REPLACE may cascade-delete checkpoints.

## Hashing and resume safety
A byte offset is not a resumable SHA-256 state. Until a selected digest implementation has a documented, versioned, serializable state format and test vectors, a resumed task must restart that digest from byte zero (or use a separately validated chunk-hash design). Never report a full-file digest from a partial prefix.

## Adaptive Engine invariant
Adaptive decisions may defer work, reduce concurrency, or pause memory-heavy stages. They may not change algorithms, sample files, truncate reads, silently skip unsupported files, or replace exact hashes with approximate results. Deferred and failed items remain visible and resumable or explicitly failed.

## Application and offline boundary
Intended host: Tauri 2 + React/TypeScript + Rust + SQLite/FTS5. Rust owns indexing and integrity; UI communicates through typed commands/events and displays durable task state. Arabic normalization belongs in the indexing/query service, not ad-hoc UI filtering. Language preference is stored in SQLite; layout uses direction-aware logical styling.

Final Windows installer must bundle WebView2 runtime strategy, native dependencies, SQLite, ONNX Runtime and only license-cleared model assets. No first-run downloads, cloud calls, API keys or hidden telemetry. This release does not satisfy the distribution gate.

## Compatibility rule
New modules depend inward on stable contracts and cannot create a second owner for topology, scheduling, task lifecycle or persistence. Schema changes require forward migrations and upgrade tests. The SQLite adapter source is present but must pass Rust compilation and round-trip tests on a Rust-enabled host before it is treated as compiler-verified or production-ready. A component is not fully integrated until exercised through the shared host path.
