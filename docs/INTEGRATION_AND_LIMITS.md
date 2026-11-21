# Integration and Limits — 0.9.1

## Safe execution flow

`path -> resolved Volume GUID -> confirmed disk extents -> confirmed device profile -> TopologyCache -> AdaptiveIoScheduler decision -> claim -> read/process -> checkpoint -> EOF + identity recheck -> completed`

Every arrow is a trust boundary. Unknown path resolution, incomplete extents, partial device probes, stale cache generations, stale leases, identity changes, and read failures must block or retry work. A detected source change specifically requires a fresh identity from the host before another claim is permitted. None means a file is absent.

## Persistence

`ExecutorSnapshotStore` is intentionally a host adapter contract. `SqliteSnapshotStore` implements the SQLite persistence boundary using the migration and one atomic snapshot transaction. Before writing, it rejects malformed task paths/Volume GUIDs and checkpoints whose source identity differs from the task or whose acknowledged byte count exceeds the source size. Migration execution and snapshot replacement are transactional. It stores:
- task ID and normalized Volume GUID;
- source identity fields;
- task state and attempt;
- committed byte offset and opaque processor-state blob;
- lease metadata, the monotonic next-token sequence, and last failure.

Persisting offset and processor state in separate transactions can corrupt resume semantics. On process restart, leases are invalidated and reissued with fresh fencing tokens.

## Processor caveat

A byte offset is not enough to resume every algorithm. For a cryptographic hash, serialize a supported versioned hasher state only if the library explicitly supports it. Otherwise restart the hash from zero; do not treat a partial digest as final. For image decoders or parsers, use their own safe restart/checkpoint contract.

## Not yet implemented
- Actual file I/O worker pool and Windows overlapped I/O.
- Rust compiler verification and adapter unit tests remain pending; the SQLite schema/migration and concrete `ExecutorSnapshotStore` source implementation are present in 0.9.1.
- Hash engine and processor-state serialization.
- Integration with the main Tauri application and UI progress display.
- Rust/Windows compilation and runtime validation.


## Large-queue persistence limitation

The persistence follow-up reconciles snapshot membership inside one `IMMEDIATE` transaction, stages the task IDs, uses conditional UPSERTs so unchanged task/checkpoint rows are not rewritten, and deletes only records absent from the new snapshot. A single ordered `LEFT JOIN` loads tasks and checkpoints, replacing the former N+1 checkpoint reads. Prepared statements are reused for the duration of each write. This removes full-row rewrite churn and N+1 reads, but does not by itself prove production-scale throughput: representative queue benchmarks, WAL/locking tests, and crash-recovery tests remain release gates.
