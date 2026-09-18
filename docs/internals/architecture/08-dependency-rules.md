# Dependency Rules & Full Adjacency Reference

## The rule

A crate in layer *N* (see [README.md](./README.md#the-layers)) may depend
on any crate in a layer `<= N`, including other crates in the same layer.
It must never depend on a crate in a layer `> N`. Layers may be skipped —
e.g. `quickwit-cluster` (layer 3) depends directly on `quickwit-proto`
(layer 1) without going through layer 2.

Layer membership is assigned bottom-up: a crate's layer is the lowest
layer at which every one of its real dependencies is already satisfied.
This is a mechanical rule applied to the actual `Cargo.toml` graph, not a
judgment about which crate is "more important".

## Known layering nuances

These look like violations at first glance; they aren't, but they're
worth knowing about before you "fix" them:

- **`quickwit-opentelemetry` sits inside layer 4 (Data-Plane Engines),
  between `quickwit-ingest` and `quickwit-indexing`.** It depends on
  `quickwit-ingest`, and `quickwit-indexing` depends on it — it's a
  genuine three-crate chain (`ingest -> opentelemetry -> indexing`)
  inside one layer, not a separate "platform" concern. It ended up there
  because it provides OTLP<->document conversion helpers that indexing
  itself needs for trace/log sources, in addition to running its own
  OTLP ingestion server.
- **`quickwit-rest-client` (layer 6) depends on `quickwit-serve` (layer
  6).** Same-layer, not upward — it reuses `quickwit-serve`'s REST
  request/response DTOs rather than duplicating them. `quickwit-serve`
  does not depend back on `quickwit-rest-client`, so there's no cycle.
- **`quickwit-lambda-client` (layer 5) depends on `quickwit-lambda-server`
  (layer 5).** Same reasoning: same-layer, reuses request types, no
  cycle.
- **`quickwit-directories` (layer 2) depends on `quickwit-storage` (layer
  3).** A tantivy `Directory` implementation has to be backed by
  something durable, so this one domain-primitives crate reaches up into
  storage. It's the one edge in layer 2 that points above its own layer's
  "typical" ceiling; every other layer-2 crate stays within layers 0-2.
- **`quickwit-search` (layer 4) depends on `quickwit-indexing` (layer
  4).** Same-layer — search reuses indexing's merge-policy/split-building
  types for administrative operations, it doesn't call into the indexing
  pipeline at query time.

If you find a *new* edge that looks like a genuine upward violation (a
foundation crate importing `quickwit-config`, for example), treat it as a
bug in the change that introduced it, not as a new nuance to document
here.

## Cross-cutting test infrastructure

`quickwit-integration-tests` is not part of any production layer. It
depends on `quickwit-cli`, `quickwit-serve`, `quickwit-control-plane`,
and most of the stack below them, to spin up a real node and drive it
through the REST API. Nothing in the production workspace depends on it.
Treat any edge *into* this crate as normal; an edge *out of* it into a
crate that isn't already a dependency of `quickwit-cli`/`quickwit-serve`
is a smell — it means a production crate would need a test-only crate,
which `cargo` would reject anyway via `[dev-dependencies]`.

## Full crate adjacency (generated from `Cargo.toml`, `path` dependencies only)

`crate -> [direct internal dependencies]`. Self-references used only to
enable a crate's own `testsuite` Cargo feature (e.g. `quickwit-indexing`
listing itself under `[dev-dependencies]`) are omitted as noise, not
architecture.

```
quickwit-actors             -> [common, metrics]
quickwit-aws                -> [common]
quickwit-cli                -> [actors, cluster, common, config, index-management,
                                 indexing, ingest, metastore, metrics, proto,
                                 rest-client, search, serve, storage,
                                 telemetry-exporters, transport]
quickwit-cluster            -> [common, config, metrics, proto, transport]
quickwit-codegen            -> []
quickwit-common             -> [metrics]
quickwit-compaction         -> [actors, cluster, common, config, doc-mapper,
                                 indexing, metastore, metrics, proto, storage]
quickwit-config             -> [common, doc-mapper, proto]
quickwit-control-plane      -> [actors, cluster, common, config, indexing, ingest,
                                 metastore, metrics, proto]
quickwit-datafusion         -> [common, config, df-core, metastore,
                                 parquet-engine, proto, search, storage]
quickwit-datetime           -> []
quickwit-df-core            -> []
quickwit-directories        -> [common, storage]
quickwit-doc-mapper         -> [common, datetime, macros, proto, query]
quickwit-dst                -> []
quickwit-index-management   -> [common, config, indexing, metastore, metrics,
                                 parquet-engine, proto, storage]
quickwit-indexing           -> [actors, aws, cluster, common, config, directories,
                                 doc-mapper, dst, ingest, metastore, metrics,
                                 opentelemetry, parquet-engine, proto, query,
                                 storage]
quickwit-ingest             -> [actors, cluster, codegen, common, config,
                                 doc-mapper, metrics, proto]
quickwit-integration-tests  -> [actors, cli, common, config, control-plane,
                                 datafusion, indexing, ingest, metastore,
                                 opentelemetry, parquet-engine, proto, rest-client,
                                 search, serve, storage, transport]
quickwit-jaeger              -> [actors, cluster, common, config, indexing, ingest,
                                 metastore, metrics, opentelemetry, proto, query,
                                 search, storage]
quickwit-janitor             -> [actors, common, compaction, config, doc-mapper,
                                 index-management, indexing, metastore, metrics,
                                 parquet-engine, proto, query, search, storage]
quickwit-lambda-client       -> [common, config, lambda-server, metrics, proto,
                                 search, storage]
quickwit-lambda-server       -> [common, config, doc-mapper, proto, search,
                                 storage]
quickwit-macros              -> []
quickwit-metastore           -> [common, config, doc-mapper, metrics,
                                 parquet-engine, proto, query, storage]
quickwit-metrics-inventory   -> [metrics]
quickwit-metrics             -> []
quickwit-opentelemetry       -> [common, config, ingest, metastore, metrics,
                                 parquet-engine, proto]
quickwit-parquet-engine      -> [dst, metrics, proto]
quickwit-proto               -> [actors, codegen, common]
quickwit-query                -> [common, datetime]
quickwit-rest-client           -> [cluster, common, config, indexing, ingest,
                                 metastore, proto, serve]
quickwit-search                 -> [common, config, directories, doc-mapper,
                                 indexing, metastore, metrics, proto, query,
                                 storage]
quickwit-serve                   -> [actors, cluster, common, compaction, config,
                                 control-plane, datafusion, doc-mapper,
                                 index-management, indexing, ingest, jaeger,
                                 janitor, lambda-client, metastore, metrics,
                                 opentelemetry, proto, query, search, storage,
                                 telemetry-exporters, transport]
quickwit-storage                  -> [aws, common, config, metrics, proto]
quickwit-telemetry-exporters        -> [common, metrics]
quickwit-transport                   -> [config, metrics]
```

(`quickwit-` prefix dropped inside the brackets for readability.)

Highest fan-in (most depended-upon): `quickwit-common` (~30 dependents),
`quickwit-proto` (~26), `quickwit-metrics` (~22), `quickwit-config`
(~17), `quickwit-storage` (~15), `quickwit-metastore` (~13). Highest
fan-out (biggest integration points): `quickwit-serve` (22 internal
dependencies) and `quickwit-indexing` (16).

## Verifying the graph

`cargo tree` reflects the same graph this document describes and is the
fastest way to spot-check a specific edge or catch drift after a
`Cargo.toml` change:

```bash
# What does quickwit-search depend on, one level of quickwit-* crates?
cd quickwit
cargo tree -p quickwit-search -e normal --prefix none | grep '^quickwit-' | sort -u

# Does anything in this layer depend on quickwit-serve? (should be empty
# except quickwit-cli, quickwit-rest-client, quickwit-integration-tests)
cargo tree -e normal --invert quickwit-serve --prefix none | grep '^quickwit-'
```

There is currently no CI check enforcing the layering rule automatically;
treat this document and the [LikeC4 model](./likec4/) as the checked-by-
review source of truth, and update both in the same PR that changes a
crate's dependencies.

## Excluded from the workspace

- **`quickwit-metastore-utils`** — has its own `Cargo.toml` with
  `replay`/`proxy` binaries, but is commented out of `[workspace]
  members` in the root `Cargo.toml` ("to ease build/deps... re-enable
  when we need it"). Not buildable via `cargo build --workspace` today.
- **`quickwit-codegen/example`** — an example crate under
  `quickwit-codegen/example/`, used to exercise `quickwit-codegen`'s
  generated code in that crate's own tests. Not part of any layer.
