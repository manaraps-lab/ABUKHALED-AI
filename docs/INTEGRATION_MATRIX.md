# Integration and Verification Matrix — 0.9.1

| Component | Source present | Contract frozen | Runtime integrated | Verification status |
|---|---:|---:|---:|---|
| Storage probe/topology | Yes | Yes | No | Source only; Windows API not run here |
| Path resolver | Yes | Yes | No | Source only; Windows API not run here |
| Device profile/volume inventory | Yes | Yes | No | Source only; Rust tests pending |
| Adaptive scheduler/telemetry | Yes | Yes | No | Source only; Rust tests pending |
| Topology cache | Yes | Yes | No | Source only; Rust tests pending |
| Resumable executor | Yes | Yes | No | Source only; Rust tests pending |
| SQLite migration | Yes | Yes | Schema-level | Python schema tests available |
| Rust SQLite adapter | Yes | Yes | Adapter boundary only | Static review; Rust compile/tests pending |
| GitHub Actions validation | Yes | Yes | Not yet run | Pending first push/workflow run |
| File reader/worker pool | No | Boundary specified | No | Not implemented |
| SHA-256 processor | No | Resume safety specified | No | Not implemented |
| Tauri/React host | No | Boundary specified | No | Not implemented |
| Full offline installer | No | Acceptance gate specified | No | Not implemented |

## Explicit non-claims
No Rust compile, `cargo test`, release build, Windows API runtime, Tauri build, or installer build is claimed for this authoring environment. A green Windows and Ubuntu CI run is required before accepting the Rust crate as build-verified.
