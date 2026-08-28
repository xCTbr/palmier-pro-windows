# Quickstart: validating spec 001

**Feature**: 001-project-model

How to prove this feature works. Every check maps to a success criterion in
[spec.md](./spec.md).

## Prerequisites

```bash
cargo --version   # 1.97+
```

No FFmpeg, no network, no macOS. This feature is pure data.

## Run the suite

```bash
cargo test -p palmier-core
```

| Test file | Proves |
|---|---|
| `decoding_contract.rs` | SC-001 — every field decodes at its audited strictness |
| `roundtrip.rs` | SC-002 — decode → encode → decode is semantically stable |
| `edge_cases.rs` | SC-003 — the spec's 13 edge cases behave as specified |
| `no_panic.rs` | SC-004 — arbitrary JSON never panics |

## Check the strictness contract specifically

The likeliest silent failure is a field decoded at the wrong strictness — it looks
correct on well-formed input and diverges only on malformed input.

```bash
cargo test -p palmier-core --test decoding_contract
```

Expected: a `Clip` with `"speed": "fast"` loads with `speed == 1.0`, while a
`Transform` with `"width": "wide"` fails to load. If both behave the same way, a
helper was chosen wrong — see the strictness table in [plan.md](./plan.md).

## Check the performance gate

```bash
cargo test -p palmier-core --release -- --ignored bench_large_project
```

Expected: a 10,000-clip project loads in under 500 ms (SC-005). This gates the
map-buffering kernel; if it fails, the kernel needs a streaming path, not the feature
a different design.

## Check it end to end

```bash
cargo run -p palmier -- inspect crates/palmier-core/tests/fixtures/multi-timeline.palmier
```

Expected: a summary per timeline with fps, resolution, duration, and per-track clip
counts, exit code 0.

```bash
cargo run -p palmier -- inspect /tmp/nonexistent
```

Expected: a diagnostic naming the problem, non-zero exit, no partial summary.

## Check CI parity

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Both must pass locally before pushing. Windows and macOS results come from CI — a
local run on WSL2 proves Linux only, and must not be reported otherwise (SC-006).

## What this does *not* prove

Round-trip tests prove self-consistency: what this code writes, this code reads.
They do not prove a real Mac accepts the output. That requires opening a
written project in Palmier Pro on macOS, and it stays a manual step.
