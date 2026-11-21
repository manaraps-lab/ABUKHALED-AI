# Alosh Engineering — 0.9.1 SQLite Snapshot Adapter

This package is the 0.9.0 integration baseline plus the 0.9.1 SQLite persistence adapter and build/CI preparation. It is a **Rust library component**, not yet the complete Alosh Engineering desktop application.

## Included

- Storage probing, device profile, volume inventory and topology mapping modules.
- Adaptive I/O scheduler, telemetry and topology cache.
- Resumable I/O executor with lease fencing and checkpoint/source-identity validation.
- `src/sqlite_store.rs`: transactional `rusqlite` implementation of `ExecutorSnapshotStore`.
- Versioned SQLite schema in `migrations/0001_executor_store.sql`.
- Regression tests for SQLite schema rules and Rust adapter behavior.
- GitHub Actions workflow for Windows and Ubuntu validation.

## Build and test

See [`BUILDING.md`](BUILDING.md). The minimum local gate is:

```sh
cargo check --all-targets
cargo test --all-targets
cargo build --release
python3 -m unittest discover -s tests -v
```

The repository selects the stable Rust toolchain. `rusqlite` uses its `bundled` feature, so a system SQLite installation is not required. Cargo dependencies are not yet vendored; the first build normally requires fetching crates unless they are already cached.

## Release status

The source archive has been integrity-checked and received static review. The Python SQLite schema suite can run without Rust. **Rust compilation, Rust tests, Windows compilation/runtime, and the release build still require execution on a Rust-enabled host or the included GitHub Actions workflow.** Do not treat the package as compiler-verified until that workflow is green.

This package does not include the Tauri host, React UI, filesystem reader/worker pool, SHA-256 processor, image/EXIF pipeline, face pipeline, or Windows installer. Those remain later integration work, not hidden build prerequisites for this library crate.
