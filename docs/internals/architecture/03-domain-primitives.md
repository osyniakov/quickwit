# Layer 2: Domain Primitives

Index-schema, query, configuration, and columnar/file-format primitives
shared across the data plane. These crates encode Quickwit's core domain
concepts but don't yet touch durable storage or run any service.

## Crates

| Crate | Responsibility | Depends on (internal) | Tags |
|-------|-----------------|------------------------|------|
| `quickwit-query` | Elasticsearch/OpenSearch-compatible Query DSL parsing. | `quickwit-common`, `quickwit-datetime` | |
| `quickwit-doc-mapper` | Index/document schema mapping: JSON documents to tantivy documents, tokenizer/storage config per field, pruning tags. | `quickwit-query`, `quickwit-proto`, `quickwit-common`, `quickwit-datetime`, `quickwit-macros` | |
| `quickwit-config` | Parses and validates node, index, source, and template configuration objects. | `quickwit-doc-mapper`, `quickwit-proto`, `quickwit-common` | |
| `quickwit-directories` | Tantivy `Directory` implementations backed by `quickwit-storage`: `StorageDirectory`, `BundleDirectory`, `HotDirectory`, `CachingDirectory`, `DebugDirectory`. | `quickwit-common`, `quickwit-storage` (layer 3) | |
| `quickwit-transport` | Node-to-node gRPC transport: channel construction and hot-reloadable TLS. | `quickwit-config`, `quickwit-metrics` | |
| `quickwit-parquet-engine` | Parquet/DataFusion-based storage and query primitives, used as an alternative to tantivy for metrics data. | `quickwit-proto`, `quickwit-dst`, `quickwit-metrics` | |

## Depended on by

Storage & Metadata (layer 3), Data-Plane Engines (layer 4), Platform
Services (layer 5), and the API surface — most crates above this point
need `quickwit-config` and/or `quickwit-doc-mapper`.

## Guidance for agents

- `quickwit-directories` is the one crate in this layer that reaches
  *up* into layer 3 (`quickwit-storage`) by dependency, because a tantivy
  `Directory` has to be backed by something durable. That's expected —
  see the note on it in
  [08-dependency-rules.md](./08-dependency-rules.md#known-layering-nuances).
- `quickwit-config` is the layer's most-depended-on crate; a change to
  its public config structs affects `quickwit-metastore`,
  `quickwit-indexing`, `quickwit-search`, `quickwit-serve`, `quickwit-cli`
  and more. Config schema changes need a migration/back-compat story —
  see `docs/configuration/` for the user-facing contract.
- Adding a new field type or tokenizer touches `quickwit-doc-mapper`
  first, then flows into `quickwit-config` (schema) and
  `quickwit-search`/`quickwit-indexing` (usage) — update in that order.
