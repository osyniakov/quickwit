# Layer 0: Foundation & Shared Kernel

Crates with no, or almost no, internal `quickwit-*` dependencies. Nearly
every other crate in the workspace depends on `quickwit-common` and/or
`quickwit-metrics`, directly or transitively — this is the base of the
dependency DAG.

## Crates

| Crate | Responsibility | Depends on (internal) | Tags |
|-------|-----------------|------------------------|------|
| `quickwit-metrics` | Zero-allocation metric declaration macros (`counter!`, `gauge!`, `histogram!`) on top of the `metrics` crate. | — | |
| `quickwit-common` | Shared utilities: metrics helpers, rate-limited logging, env-var reading, `run_cpu_intensive` for offloading CPU-bound work off the Tokio runtime, and more. | `quickwit-metrics` | |
| `quickwit-macros` | Workspace proc-macro definitions. | — | |
| `quickwit-codegen` | Generates service traits, adapters, and gRPC clients/servers from `.proto` files at build time. Used by `quickwit-proto`. | — | |
| `quickwit-datetime` | Date/time parsing utilities shared by config, query, and doc-mapper. | — | |
| `quickwit-aws` | Shared, Rustls-based AWS HTTP client construction reused by every AWS SDK client in the workspace (S3, Kinesis, SQS, Lambda). | `quickwit-common` | |
| `quickwit-dst` | Deterministic simulation testing harness and shared invariants (TLA+/`stateright` model-checking). Consumed by `quickwit-indexing` and `quickwit-parquet-engine` directly, not just from test code. | — | |
| `quickwit-metrics-inventory` | Dev-tool binary enumerating every registered `MetricInfo` via the `inventory` crate, run via `scripts/run_inventory.sh`. | `quickwit-metrics` | `tool` |

Not modeled here: `quickwit-codegen/example`, an example-only crate that
exercises `quickwit-codegen` in tests.

## Depended on by

Every other layer, directly or transitively. Layer 1
([Protocol & Actor Core](./02-protocol-actors.md)) is the first layer
built on top of it.

## Guidance for agents

- This layer has an extremely high fan-in (dozens of dependents). A
  breaking change to `quickwit-common` or `quickwit-metrics` — a renamed
  public function, a changed macro signature — has workspace-wide blast
  radius. Grep for usages across the whole `quickwit/` tree, not just the
  crate you're touching, before changing a public item here.
- `quickwit-dst` is *not* test-only despite the name: `quickwit-indexing`
  depends on it for production invariants, not just its own tests. Don't
  gate it behind `#[cfg(test)]` assumptions.
- Keep this layer dependency-free of everything above it. If you're
  tempted to add a `quickwit-config` or `quickwit-proto` dependency to a
  foundation crate, that's a sign the new code belongs in a higher layer
  instead (see [08-dependency-rules.md](./08-dependency-rules.md)).
