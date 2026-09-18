# Layer 6: API Surface & Entry Points

What operators and other processes actually run or call. Every layer
below this one exists to serve this layer; nothing above it depends on
anything else in the workspace.

## Crates

| Crate | Responsibility | Depends on (internal) | Tags |
|-------|-----------------|------------------------|------|
| `quickwit-serve` | Hosts the REST and gRPC APIs over the same service traits, wires together every platform service, and serves the embedded UI. | Nearly the whole workspace: `quickwit-actors`, `quickwit-cluster`, `quickwit-common`, `quickwit-compaction`, `quickwit-config`, `quickwit-control-plane`, `quickwit-datafusion`, `quickwit-doc-mapper`, `quickwit-index-management`, `quickwit-indexing`, `quickwit-ingest`, `quickwit-jaeger`, `quickwit-janitor`, `quickwit-lambda-client`, `quickwit-metastore`, `quickwit-metrics`, `quickwit-opentelemetry`, `quickwit-proto`, `quickwit-query`, `quickwit-search`, `quickwit-storage`, `quickwit-telemetry-exporters`, `quickwit-transport` | `entrypoint` |
| `quickwit-cli` | The `quickwit` binary: run/start a node, manage indexes, ingest files, and other operator commands. Also builds a `generate_markdown` binary for CLI reference docs. | `quickwit-actors`, `quickwit-cluster`, `quickwit-common`, `quickwit-config`, `quickwit-index-management`, `quickwit-indexing`, `quickwit-ingest`, `quickwit-metastore`, `quickwit-metrics`, `quickwit-proto`, **`quickwit-rest-client`**, `quickwit-search`, **`quickwit-serve`**, `quickwit-storage`, `quickwit-telemetry-exporters`, `quickwit-transport` | `entrypoint` |
| `quickwit-rest-client` | Rust client library for the Quickwit REST API. | `quickwit-cluster`, `quickwit-common`, `quickwit-config`, `quickwit-indexing`, `quickwit-ingest`, `quickwit-metastore`, `quickwit-proto`, **`quickwit-serve`** | |
| `quickwit-ui` | React/TypeScript single-page app embedded into and served by `quickwit-serve`. **Not part of the Cargo workspace** (no `Cargo.toml`; own `package.json`/Vite/Playwright toolchain). | — | `entrypoint` |

`quickwit-rest-client` depending on `quickwit-serve` (a client on "the
server") is intentional: it reuses `quickwit-serve`'s REST
request/response DTOs instead of duplicating them. `quickwit-cli` then
depends on both `quickwit-serve` (to run a node) and
`quickwit-rest-client` (to talk to one), which is why it sits at the very
top.

## Depended on by

Nothing in the production workspace. Only
[`quickwit-integration-tests`](./08-dependency-rules.md#cross-cutting-test-infrastructure)
depends on this layer, and only to drive it end-to-end in tests.

## Guidance for agents

- `quickwit-serve` is the workspace's biggest integration point (22
  internal dependencies). A build break here after touching an unrelated
  crate almost always means a trait or type this crate re-exports moved —
  check `quickwit-serve`'s wiring code for the service you changed.
- `quickwit-ui` is built separately (`quickwit-ui/README.md`,
  `Makefile` targets) and its build output gets embedded into the
  `quickwit-serve` binary. A UI change doesn't require a Rust rebuild
  unless the embedding step runs, and a REST API change in
  `quickwit-serve` can silently break the UI without a Rust-side test
  catching it — check `quickwit-ui/src` for callers of the changed
  endpoint, and prefer running the UI's own test/e2e suite
  (`quickwit-ui/package.json`) when changing REST responses it consumes.
- When adding a new CLI subcommand or REST endpoint, prefer going through
  `quickwit-rest-client` for anything that needs a typed client (the CLI
  itself uses it for some commands) rather than hand-rolling HTTP calls.
