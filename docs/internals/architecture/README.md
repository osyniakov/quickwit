# Layered Architecture

This directory documents the `quickwit/` Cargo workspace as a stack of
layers, derived directly from the real `path` dependencies declared in
each crate's `Cargo.toml` (not from intent or aspiration). It exists so
that both human contributors and AI coding agents can quickly answer:

- "Which layer is this crate in, and what may it depend on?"
- "If I add a dependency from crate A to crate B, am I changing the
  architecture or just using it?"
- "What does the system look like as a diagram?"

**Source of truth for the dependency graph:** each crate's `Cargo.toml`.
When this changes, re-run the check in
[`08-dependency-rules.md`](./08-dependency-rules.md#verifying-the-graph)
and update the affected layer doc and the LikeC4 model in
[`likec4/`](./likec4/).

## The layers

Layers are ordered bottom-up. A crate in layer *N* may depend on crates in
any layer `< N` (skipping layers is fine) but never on a crate in a layer
`> N`. Layer membership is by "lowest layer that satisfies every real
dependency", not by subjective importance — see
[`08-dependency-rules.md`](./08-dependency-rules.md) for the exceptions
this produces and why they exist.

| # | Layer | Doc | Crates |
|---|-------|-----|--------|
| 0 | Foundation & Shared Kernel | [01-foundation.md](./01-foundation.md) | `quickwit-common`, `quickwit-metrics`, `quickwit-macros`, `quickwit-codegen`, `quickwit-datetime`, `quickwit-aws`, `quickwit-dst`, `quickwit-metrics-inventory` |
| 1 | Protocol & Actor Core | [02-protocol-actors.md](./02-protocol-actors.md) | `quickwit-proto`, `quickwit-actors` |
| 2 | Domain Primitives | [03-domain-primitives.md](./03-domain-primitives.md) | `quickwit-query`, `quickwit-doc-mapper`, `quickwit-config`, `quickwit-directories`, `quickwit-transport`, `quickwit-parquet-engine` |
| 3 | Storage & Metadata | [04-storage-metadata.md](./04-storage-metadata.md) | `quickwit-storage`, `quickwit-metastore`, `quickwit-cluster` |
| 4 | Data-Plane Engines | [05-data-plane-engines.md](./05-data-plane-engines.md) | `quickwit-ingest`, `quickwit-opentelemetry`, `quickwit-indexing`, `quickwit-search`, `quickwit-compaction` |
| 5 | Platform Services | [06-platform-services.md](./06-platform-services.md) | `quickwit-control-plane`, `quickwit-index-management`, `quickwit-janitor`, `quickwit-jaeger`, `quickwit-lambda-server`, `quickwit-lambda-client`, `quickwit-datafusion`, `quickwit-df-core`, `quickwit-telemetry-exporters` |
| 6 | API Surface & Entry Points | [07-api-surface.md](./07-api-surface.md) | `quickwit-serve`, `quickwit-cli`, `quickwit-rest-client`, `quickwit-ui` (frontend, not a crate) |
| — | Cross-Cutting Test Infrastructure | [08-dependency-rules.md](./08-dependency-rules.md#cross-cutting-test-infrastructure) | `quickwit-integration-tests` |

Not modeled (excluded from the Cargo workspace or from the diagrams,
documented for completeness):

- `quickwit-metastore-utils` — commented out of `[workspace] members` in
  the root `Cargo.toml`; a standalone `replay`/`proxy` dev tool.
- `quickwit-codegen/example` — an example-only crate that exists to
  exercise `quickwit-codegen` in tests; not part of any runtime layer.

## Diagrams

Diagrams are authored with [LikeC4](https://likec4.dev) in
[`likec4/`](./likec4/) and are the same graph as the tables in this
directory, just rendered. See [`likec4/README.md`](./likec4/README.md)
to preview or export them. There are three levels:

1. **System landscape** (`index` view) — Quickwit as one box plus the
   external systems it talks to (object storage, PostgreSQL, message
   queues, OTel clients, AWS Lambda).
2. **Layers** (`layers` view) — the 8 layers above as boxes, with an
   edge whenever a crate in one layer depends on a crate in another.
3. **Per-layer detail** (`<layer>Detail` views) — every crate in one
   layer, plus the layers/external systems it talks to, collapsed to
   single boxes.

## How to use this as an agent

- **Before adding a new `quickwit-*` path dependency**, find both
  crates' layers in the table above. A same-layer or downward
  dependency is business as usual. An **upward** dependency (a lower
  layer needing something from a higher one) is an architectural change,
  not a drive-by import — it almost always means either the dependency
  belongs in a lower layer, or the new code belongs in a higher one.
  Don't add it silently; flag it in the PR/commit description and, if
  there's an ADR process for the area (see `../adr/`), consider recording
  it there.
- **Before renaming, splitting, or merging a crate**, update the matching
  layer doc and the LikeC4 model together in the same change — they are
  meant to never drift from `Cargo.toml`.
- **When you need "what does X depend on / what depends on X"**, check
  the layer doc's crate table first; it links each crate to its
  responsibility and its direct dependencies. For the exhaustive,
  generated-from-source adjacency list, see
  [`08-dependency-rules.md`](./08-dependency-rules.md).
