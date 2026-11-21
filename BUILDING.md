# Build and validation — Alosh Engineering 0.9.1

## Scope of this package

This is the Rust storage/topology/scheduler/resumable-executor library with its SQLite snapshot adapter. It is **not** the complete desktop application: the Tauri host, React UI, filesystem reader/worker pool, SHA-256 processor, image/EXIF and face pipelines, and Windows installer are not part of this source package.

## Prerequisites

- Rust stable toolchain (the repository pins the channel in `rust-toolchain.toml`).
- Python 3 for the independent SQLite schema tests.
- Internet access on the first Cargo build unless dependencies have already been cached or vendored.

`rusqlite` enables the `bundled` feature, so SQLite itself is built from the bundled source rather than requiring a system SQLite installation. This does **not** vendor all Rust crates; it does not by itself make the first Cargo build fully offline.

## Windows PowerShell

From the directory containing `Cargo.toml`:

```powershell
rustup show
cargo check --all-targets
cargo test --all-targets
cargo build --release
python -m unittest discover -s tests -v
```

## Linux/macOS shell

```sh
rustup show
cargo check --all-targets
cargo test --all-targets
cargo build --release
python3 -m unittest discover -s tests -v
```

## Fully offline Cargo builds (optional preparation)

On a connected machine, fetch dependencies and vendor them:

```sh
cargo fetch
cargo vendor vendor > .cargo/config.toml
```

Commit `Cargo.lock`, `vendor/`, and `.cargo/config.toml` if the release policy requires reproducible, fully offline builds. Review the licenses/notices of vendored dependencies before distribution. The current package does not claim that this vendoring step has already been completed.

## CI

After pushing this directory to GitHub, open **Actions → Rust validation**. The workflow checks and tests the crate on Ubuntu and Windows, builds release artifacts, and runs the Python SQLite schema tests. A green workflow is the required Rust build gate; adding the workflow file does not itself mean the build has passed.
