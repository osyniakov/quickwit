# Layer 4: Data-Plane Engines

The engines that move and transform documents: ingestion, indexing, and
distributed search. This is where most of Quickwit's request/document
processing actually happens.

## Crates

| Crate | Responsibility | Depends on (internal) |
|-------|-----------------|------------------------|
| `quickwit-ingest` | Native, WAL-based distributed and replicated ingestion engine. | `quickwit-actors`, `quickwit-cluster`, `quickwit-codegen`, `quickwit-common`, `quickwit-config`, `quickwit-doc-mapper`, `quickwit-metrics`, `quickwit-proto` |
| `quickwit-opentelemetry` | OTLP ingestion server and OTLP-to-document conversion. | `quickwit-common`, `quickwit-config`, `quickwit-ingest`, `quickwit-metastore`, `quickwit-metrics`, `quickwit-parquet-engine`, `quickwit-proto` |
| `quickwit-indexing` | Actor-based indexing pipeline (`Source -> DocProcessor -> Indexer -> IndexSerializer -> Packager -> Uploader -> Sequencer -> Publisher`) plus the parallel merge pipeline. | `quickwit-actors`, `quickwit-aws`, `quickwit-cluster`, `quickwit-common`, `quickwit-config`, `quickwit-directories`, `quickwit-doc-mapper`, `quickwit-dst`, `quickwit-ingest`, `quickwit-metastore`, `quickwit-metrics`, **`quickwit-opentelemetry`**, `quickwit-parquet-engine`, `quickwit-proto`, `quickwit-query`, `quickwit-storage` |
| `quickwit-search` | Distributed search orchestration: root servers parse and coordinate, leaf servers search their assigned splits in parallel, results are merged at the root. | `quickwit-common`, `quickwit-config`, `quickwit-directories`, `quickwit-doc-mapper`, `quickwit-indexing`, `quickwit-metastore`, `quickwit-metrics`, `quickwit-proto`, `quickwit-query`, `quickwit-storage` |
| `quickwit-compaction` | Compactor service that merges and rewrites splits. | `quickwit-actors`, `quickwit-cluster`, `quickwit-common`, `quickwit-config`, `quickwit-doc-mapper`, `quickwit-indexing`, `quickwit-metastore`, `quickwit-metrics`, `quickwit-proto`, `quickwit-storage` |

`quickwit-opentelemetry` sits *between* `quickwit-ingest` and
`quickwit-indexing` in the dependency order: it depends on
`quickwit-ingest`, and `quickwit-indexing` depends on it (for
OTLP<->document conversion helpers used when indexing traces/logs). See
[08-dependency-rules.md](./08-dependency-rules.md#known-layering-nuances)
before assuming this is a mistake.

`quickwit-search` depending on `quickwit-indexing` is a same-layer
dependency, not a violation — search reuses indexing's merge-policy and
split-building types for administrative operations.

## Depended on by

Platform Services (layer 5) and the API surface — `quickwit-serve`,
`quickwit-jaeger`, `quickwit-janitor`, `quickwit-control-plane`, and
`quickwit-cli` all sit on top of this layer.

## Guidance for agents

- This is the actor-heaviest part of the codebase. Read
  `quickwit-actors`' mailbox/supervision model before changing pipeline
  stages — a stage that blocks its mailbox stalls everything downstream.
- `quickwit-search`'s root/leaf split means a change to a search request
  or response type in `quickwit-proto` typically needs updates on both
  the root-side coordination code and the leaf-side execution code in
  this crate.
- The merge policy (in `quickwit-indexing`) and the compactor
  (`quickwit-compaction`) both rewrite splits; see
  `docs/internals/compaction-architecture.md` for how they interact
  before changing either.
- `quickwit-ingest`'s exactly-once guarantee relies on checkpoints stored
  in the metastore (layer 3) being updated atomically with split
  publication — see `docs/overview/concepts/indexing.md`.
