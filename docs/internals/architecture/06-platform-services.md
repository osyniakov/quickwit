# Layer 5: Platform Services

Cluster-wide services built on top of the data-plane engines: scheduling,
index lifecycle, maintenance, and a few specialized backends and
deployment targets.

## Crates

| Crate | Responsibility | Depends on (internal) | Tags |
|-------|-----------------|------------------------|------|
| `quickwit-control-plane` | Schedules indexing tasks to indexers and tracks the pool of available indexer nodes. Reacts to metastore events, a 3s heartbeat, and a 1-minute full replan. | `quickwit-actors`, `quickwit-cluster`, `quickwit-common`, `quickwit-config`, `quickwit-indexing`, `quickwit-ingest`, `quickwit-metastore`, `quickwit-metrics`, `quickwit-proto` | |
| `quickwit-index-management` | Creates and manages indexes, sources, and templates. | `quickwit-common`, `quickwit-config`, `quickwit-indexing`, `quickwit-metastore`, `quickwit-metrics`, `quickwit-parquet-engine`, `quickwit-proto`, `quickwit-storage` | |
| `quickwit-janitor` | Maintenance service: garbage collection, delete-query tasks, retention policies. | `quickwit-actors`, `quickwit-common`, `quickwit-compaction`, `quickwit-config`, `quickwit-doc-mapper`, `quickwit-index-management`, `quickwit-indexing`, `quickwit-metastore`, `quickwit-metrics`, `quickwit-parquet-engine`, `quickwit-proto`, `quickwit-query`, `quickwit-search`, `quickwit-storage` | |
| `quickwit-jaeger` | Jaeger-compatible trace storage and query backend. | `quickwit-actors`, `quickwit-cluster`, `quickwit-common`, `quickwit-config`, `quickwit-indexing`, `quickwit-ingest`, `quickwit-metastore`, `quickwit-metrics`, `quickwit-opentelemetry`, `quickwit-proto`, `quickwit-query`, `quickwit-search`, `quickwit-storage` | |
| `quickwit-lambda-server` | AWS Lambda handler executing leaf-search requests. | `quickwit-common`, `quickwit-config`, `quickwit-doc-mapper`, `quickwit-proto`, `quickwit-search`, `quickwit-storage` | `entrypoint` |
| `quickwit-lambda-client` | Invokes and auto-deploys the leaf-search Lambda function. | `quickwit-common`, `quickwit-config`, **`quickwit-lambda-server`**, `quickwit-metrics`, `quickwit-proto`, `quickwit-search`, `quickwit-storage` | |
| `quickwit-datafusion` | Quickwit-specific DataFusion glue: metrics data source, object-store adapter, searcher-pool worker resolver. | `quickwit-common`, `quickwit-config`, `quickwit-df-core`, `quickwit-metastore`, `quickwit-parquet-engine`, `quickwit-proto`, `quickwit-search`, `quickwit-storage` | `experimental` |
| `quickwit-df-core` | Generic DataFusion runtime framework (session, query service, distributed worker, Substrait) with no Quickwit domain coupling. | — | `experimental` |
| `quickwit-telemetry-exporters` | Exports Quickwit usage/analytics telemetry. | `quickwit-common`, `quickwit-metrics` | |

`quickwit-lambda-client` depending on `quickwit-lambda-server` (a client
depending on "the server") looks backwards but isn't a cycle:
`lambda-server` doesn't depend on `lambda-client`. The client reuses
request/response types the server crate defines for the Lambda
invocation payload.

`quickwit-datafusion` and `quickwit-df-core` are opt-in: they're in the
workspace's `members` list but excluded from `default-members`, so a
plain `cargo build` skips them. Treat both as experimental/evolving.

## Depended on by

The API surface (layer 6): `quickwit-serve` wires together nearly every
crate in this layer, and `quickwit-cli` uses a subset directly.

## Guidance for agents

- Most crates here are individually-addressable services mounted by
  `quickwit-serve` (see [07-api-surface.md](./07-api-surface.md)). When
  adding a new platform service, follow the existing pattern: a crate in
  this layer implementing a `*Service` trait from `quickwit-proto`, wired
  into `quickwit-serve`'s startup.
- `quickwit-control-plane`'s scheduling logic has three trigger paths
  (metastore events, heartbeat, full replan) — a bug fix that only
  handles one path likely needs to handle the other two for consistency.
  See `docs/overview/architecture.md#control-plane`.
- `quickwit-janitor` and `quickwit-compaction` (layer 4) both mutate
  splits; a change to one that isn't reflected in the other can produce
  splits that are simultaneously "being compacted" and "being deleted".
