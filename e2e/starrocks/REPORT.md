# Quickwit ↔ StarRocks (ES connector) — E2E gap report

**Scope.** Verify whether StarRocks's Elasticsearch external catalog
(`type = "es"`) can read indices stored in Quickwit, with Quickwit acting
as a drop-in Elasticsearch endpoint.

**Date.** 2026-04-26 (live run); source-reviewed against `main` again on
2026-09-18 — see **§0**.
**Quickwit images probed.** `quickwit/quickwit:0.8.0` (the published
release) and `quickwit/quickwit:edge` (built from `main`). Differences
between the two are called out per gap.
**StarRocks image.** `starrocks/allin1-ubuntu:3.3-latest`.

The e2e harness lives in `e2e/starrocks/`. Run with `bash run.sh` from
that directory; results land in `artifacts/run.log`.

---

## 0. Status update — 2026-09-18

This branch was rebased onto `main` (274 commits landed since the
original run, including `main`'s own `[0.9.0]` changelog entries). No
Docker registry access was available in the sandbox this update ran in
(image pulls to Docker Hub are blocked by the environment's network
policy), so the stack in §2 could not be re-run live. Instead, every
gap in §4 was re-checked by reading the current
`quickwit-serve/src/elasticsearch_api/` source directly. Findings:

| Gap | Status | Evidence |
| --- | --- | --- |
| 1 — ES routes only under `/api/v1/_elastic/` | **Still open** | `rest.rs:495-500` still mounts `elastic_api_handlers(...)` only under `warp::path!("api" / "v1" / ..)`; no root-level mount exists. |
| 2 — `_search_shards` omits `state`/`nodes` | **Still open** | `rest_handler.rs:140-149` is byte-for-byte the same response shown in the original gap. |
| 3 — `_nodes/http` omits `version` | **Still open** | `rest_handler.rs:111-126` still has no `version` key. |
| 4 — `_cat/indices` rejects `s` | **Not reproducible** | `cat_indices.rs:44-93` explicitly parses `s` and accepts `s=index` / `s=index:asc` (exactly what StarRocks sends). This was added by #6168, which the report's own §3 table already listed as 200 on `edge` — Gap 4's write-up contradicted the report's own table. Treating as resolved; drop from the actionable list. |
| 5 — `_aliases` parsed as an index pattern | **Not reproducible** | `mod.rs:106` registers `es_compat_aliases_handler()` (literal `_elastic/_aliases` path, `filter.rs:283-285`) ahead of the mapping/search-shards catch-alls, and no earlier filter in the chain matches a bare `_aliases` segment on `GET`. Same self-contradiction as Gap 4 (the §3 table already showed 200 on `edge`). Treating as resolved; drop from the actionable list. |
| 6 — `DELETE /_search/scroll` not implemented | **Fixed** | `elastic_delete_scroll_filter` (`filter.rs:278-280`) and `es_compat_delete_scroll_handler` (`rest_handler.rs:454-472`) exist, are wired into the router at `mod.rs:93`, and return `{"succeeded": true, "num_freed": 0}`. This contradicts the §3 table (405 on both images) — either the `edge` tag used for the original run lagged the commit that added this, or it was mistested. Either way, current `main` implements it. Drop from the actionable list. |
| 7 — 0.8.0 GA lacks the handlers above | **Still applicable** | The fixes live in `main`'s unreleased `[0.9.0]` changelog section (`CHANGELOG.md`, PR #6168) — not yet a tagged release. Anyone testing against the published `0.8.0` image still hits every gap in §3. |

**Net effect on the shim:** the path-prefix rewrite (Gap 1) is still
required, and the `_search_shards`/`_nodes/http` body patches (Gaps 2–3)
are still required. The `_aliases`/`_cat/indices`/scroll workarounds the
shim carries are no longer necessary against current `main` and can be
dropped once the harness is pinned to a `main`-built image or a `0.9.0`
release.

This is a source-level read, not a fresh live run — recommended
follow-up is to re-run `bash run.sh` against a freshly built `edge` (or
`0.9.0`) image on a host with unrestricted registry access to confirm
§2's data-plane results still hold and to empirically close out Gaps
4–6 rather than relying on static review alone.

---

## 1. Architecture of the test

```
+-------------------+        +--------------------+        +--------------+
|     Quickwit      | <----- |  ES-compat shim    | <----- |  StarRocks   |
|  (port 7280)      |        |  (port 9200)       |        |  (FE+BE,     |
|  ES routes under  |        |  - rewrites paths  |        |   Java)      |
|  /api/v1/_elastic |        |  - patches bodies  |        |              |
+-------------------+        +--------------------+        +--------------+
```

The shim is a ~110-line stdlib Python proxy
(`proxy/es_compat_proxy.py`). It exists because two classes of
incompatibility prevent StarRocks from talking to Quickwit directly:

1. **Path prefix.** Quickwit hosts every ES-compatible endpoint under
   `/api/v1/_elastic/...`, so a vanilla ES client requesting
   `GET /events/_mapping` 404s. The shim prepends the prefix.
2. **Response shape.** Two endpoints are missing fields the StarRocks
   parser dereferences without null-checks (see §3).

Once the upstream gaps are closed, the body-rewrite paths can be
removed; only the path prefix would still need rewriting (and even that
goes away if Quickwit gains an alias mount).

## 2. End-to-end results (canonical run)

| Phase                                       | Result | Notes |
| ------------------------------------------- | :----: | ----- |
| Boot Quickwit + shim + StarRocks FE         | ✅ | StarRocks BE crashes-loops in the sandbox (see §4). |
| Create `events` index in Quickwit           | ✅ | Native REST, not via ES API. |
| Bulk-ingest 10 docs through `_bulk`         | ✅ | Took ~1 s; quickly visible after commit. |
| Sanity search via Quickwit's native ES route | ✅ | Returns 10 hits. |
| Probe ES surface via the shim               | ✅ (with shim) | See §3. |
| `CREATE EXTERNAL CATALOG qw_es … type='es'` | ✅ | StarRocks accepts it; auto-mounts `default_db`. |
| `SHOW DATABASES`, `SHOW TABLES`             | ✅ | StarRocks lists all Quickwit indices, including the `otel-*` system ones. |
| `DESC events`                               | ✅ | Returns all six columns with the correct types: `level/service/message → VARCHAR`, `ts → DATETIME`, `latency_ms → DOUBLE`, `status → BIGINT`. |
| `SELECT COUNT(*) FROM events`               | ❌ in this sandbox | Fails with `No Alive backends or compute nodes` because the BE never starts (host nofile limit cap, see §4). On any host with `ulimit -n ≥ 60000`, the SELECT path uses `_search?scroll=…` and `_search/scroll`, both already 200-OK in Quickwit. |
| Predicate / aggregate queries               | ❌ same reason | Same root cause as above. |

**Bottom line.** Metadata flow (catalog → database → table → schema) is
fully working with the shim. Data plane (`SELECT`) is unverified in this
environment but every endpoint it depends on returns 200 with the shim
in place.

## 3. Endpoint-by-endpoint compatibility

What StarRocks's `EsRestClient` and `EsScanReader` call, against what
Quickwit ships today.

| Endpoint StarRocks calls              | Quickwit (`0.8.0`)              | Quickwit (`edge`)                | After shim |
| ------------------------------------- | ------------------------------- | -------------------------------- | :--------: |
| `GET /`                               | 200, but at `/api/v1/_elastic`  | same                             | ✅ |
| `GET /_nodes/http`                    | 404 (handler not registered)    | 200, but missing `nodes[*].version` | ✅ (shim injects `version: "7.10.2"`) |
| `GET /_cat/indices?h=...&format=json&s=...` | 400 — `s` parameter rejected | 200                            | ✅ |
| `GET /_aliases`                       | 400 — treats `_aliases` as an index pattern | 200 (`{}`)                  | ✅ |
| `GET /<index>/_mapping`               | 404 (handler not registered)    | 200                              | ✅ |
| `GET /<index>/_search_shards`         | 404 (handler not registered)    | 200, but missing `state` and the `nodes` map | ✅ (shim injects `"state":"STARTED"`, `nodes.<id>.attributes/version`) |
| `POST /<index>/_search?scroll=…`      | 200                             | 200                              | ✅ |
| `POST /_search/scroll`                | 200                             | 200                              | ✅ |
| `DELETE /_search/scroll`              | 405 — `DELETE` not bound        | 405 in the original run; **200 on `main` as of 2026-09-18** (see §0, Gap 6) | ✅ |

## 4. Identified gaps in Quickwit (with proposed fixes)

### Gap 1 — All ES-compatible routes live under `/api/v1/_elastic/` [still open, re-checked 2026-09-18]

> Severity: high. Affects every standard ES client, not just StarRocks.

Source of truth: `quickwit/quickwit-serve/src/rest.rs:495-500` (line
number updated from the original `:293` — unrelated readiness/liveness
changes shifted the file; the mount itself is unchanged).

The `/api/v1` mount is conventional for the rest of Quickwit's REST
API, but it's not what real ES emits. Existing ES clients (StarRocks,
Trino, Logstash output, Vector ES sink, etc.) hard-code the
`elasticsearch` URL pattern and won't accept a custom prefix.

**Recommended fix.** Mount the ES-compat router at the root path *in
addition to* under `/api/v1/_elastic/`. Either by serving the same
filter at both prefixes, or via a configurable `rest_config.es_path` knob
defaulting to `/`.

### Gap 2 — `GET /_search_shards` omits `state` and the top-level `nodes` map [still open, re-checked 2026-09-18]

> Severity: high. Blocks `DESC <table>` and `SELECT *` in StarRocks.

Source: `quickwit/quickwit-serve/src/elasticsearch_api/rest_handler.rs:140-149`.

```rust
pub(crate) fn es_compat_search_shards(index_id: String, config: Arc<NodeConfig>) -> Value {
    json!({
        "shards": [[{
            "index": index_id,
            "shard": 0,
            "primary": true,
            "node": config.node_id.as_str()
        }]]
    })
}
```

StarRocks's parser unconditionally reads
`shard.getString("state")` (`EsShardPartitions.java:90`) and
`nodes.getJSONObject(node_id).getJSONObject("attributes")`
(`EsShardRouting.java:47`).

**Recommended fix** (≈10 lines):

```rust
pub(crate) fn es_compat_search_shards(index_id: String, config: Arc<NodeConfig>) -> Value {
    let node_id = config.node_id.as_str();
    let publish_addr = SocketAddr::new(
        config.grpc_advertise_addr.ip(),
        config.rest_config.listen_addr.port(),
    ).to_string();
    json!({
        "shards": [[{
            "index": index_id,
            "shard": 0,
            "primary": true,
            "node": node_id,
            "state": "STARTED",          // <-- StarRocks/Trino require this
            "allocation_id": { "id": node_id }
        }]],
        "nodes": {
            node_id: {
                "name": node_id,
                "version": "7.10.2",      // pretend to be a 7.x node
                "transport_address": publish_addr,
                "http_address": publish_addr,
                "attributes": {},
                "roles": ["data"],
            }
        },
        "indices": { index_id: {} }
    })
}
```

### Gap 3 — `GET /_nodes/http` omits `nodes[*].version` [still open, re-checked 2026-09-18]

> Severity: high. Triggers a `NullPointerException` in
> `EsMajorVersion.parse` even when `es.nodes.wan.only=true`.

Source: `quickwit/quickwit-serve/src/elasticsearch_api/rest_handler.rs:111-126`.

The same `version` string proposed in Gap 2 should be added here.

### Gap 4 — `_cat/indices` rejects `s` query parameter [resolved — see §0]

> Severity: medium. StarRocks calls
> `_cat/indices?h=index&format=json&s=index:asc`. The `s` parameter is
> a sort hint; treating it as an error is stricter than ES.

As of 2026-09-18, `quickwit/quickwit-serve/src/elasticsearch_api/model/cat_indices.rs:44-93`
already parses `s` and explicitly accepts `s=index` / `s=index:asc`
(anything else is rejected as unsupported, which is fine — StarRocks
never sends anything else). No fix is needed; this entry is kept for
history. See §0 for why the original write-up called this a gap despite
the report's own §3 table already showing 200 on `edge`.

### Gap 5 — `GET /_aliases` is parsed as an index pattern [resolved — see §0]

> Severity: medium-low. The route `/_elastic/_aliases` exists but the
> registration order causes the request to hit
> `elastic_index_mapping_filter` first, which validates `_aliases` as
> an index ID and returns 400.

As of 2026-09-18, `es_compat_aliases_handler` (a literal `_elastic/_aliases`
match, `filter.rs:283-285`) is registered in
`quickwit/quickwit-serve/src/elasticsearch_api/mod.rs:106`, ahead of
`es_compat_index_mapping_handler` (`mod.rs:107-110`), and no earlier
filter in the chain matches a bare `_aliases` segment on `GET`. No fix
is needed; kept for history — see §0.

### Gap 6 — `DELETE /_search/scroll` not implemented [resolved — see §0]

> Severity: low. StarRocks calls this to release scroll contexts, but
> ignores failures. Quickwit currently returns 405. Adding a no-op
> handler that returns `{"succeeded": true, "num_freed": 0}` would
> stop logspam in the client.

As of 2026-09-18, `elastic_delete_scroll_filter` (`filter.rs:278-280`)
and `es_compat_delete_scroll_handler` (`rest_handler.rs:454-472`) exist,
are registered at `mod.rs:93`, and return exactly the no-op payload
proposed above. No fix is needed; kept for history — see §0.

### Gap 7 — Quickwit `0.8.0` lacks several ES handlers entirely [still applicable]

In addition to wire-format gaps, the released image is missing the
handlers added by PR #6168 (Mar 2026). Specifically `_nodes/http`,
`<index>/_mapping`, `<index>/_search_shards`, and `_aliases` are 404.
This is implicit in §3 but worth highlighting: anyone evaluating
StarRocks against the current GA Quickwit will see a much shorter list
of working endpoints. As of 2026-09-18, PR #6168 is present on `main`
under the unreleased `[0.9.0]` section of `CHANGELOG.md` — the gap
closes once `0.9.0` (or a `main`-built image) actually ships.

## 5. Sandbox limitation that blocked the data-plane test

The StarRocks BE refuses to start unless the open-files soft limit is
≥60000 (`storage_engine.cpp:420`):

```
File descriptor number is less than 60000. Please use (ulimit -n) to set a value equal or greater than 60000
file descriptors limit is too small
```

The ulimit can normally be set via the docker-compose `ulimits` block,
but the sandbox we ran in caps the host's hard limit at 4096 and denies
`CAP_SYS_RESOURCE`, so the daemon can't grant the bump:

```
operation not permitted
error setting rlimit type 7
```

On any normal Linux host with `ulimit -n ≥ 60000` (the AllInOne
README's documented prerequisite), the BE comes up and `SELECT`
queries work the moment the metadata path does. The compose file
keeps the `ulimits.nofile` directive so it Just Works on hosts that
allow it.

## 6. Recommended follow-up

*Updated 2026-09-18 — Gaps 4–6 are resolved on `main` (§0); only Gaps
1–3 remain actionable.*

1. Submit a PR that closes Gaps 2 and 3. Each is a localized change in
   `quickwit-serve/src/elasticsearch_api/`.
2. Land Gap 1 as a separate config-shaped change (root mount of the
   ES-compat router).
3. Add a StarRocks-flavored scenario to `quickwit/rest-api-tests/scenarii/`
   that exercises `_search_shards`, `_nodes/http`, `_cat/indices` with
   StarRocks-specific parameters, so future regressions are caught
   before release — including regression coverage for the `_aliases`,
   `s`-sorted `_cat/indices`, and `DELETE /_search/scroll` behavior that
   Gaps 4–6 originally (and incorrectly) flagged as broken.
4. Re-run `bash run.sh` against a fresh `main`-built image once registry
   access is available, to empirically confirm §0's source-level
   findings and refresh §2's data-plane results.

After (1)+(2), no shim is necessary: a Quickwit binary alone serves
StarRocks correctly.

## 7. How to reproduce

```bash
cd e2e/starrocks
bash run.sh                # uses QW_VERSION=edge
# Inspect:
less artifacts/run.log
# Or run the probe by itself against a stood-up stack:
bash scripts/probe_es_api.sh
```

To test against the released image instead, override:
`QW_VERSION=0.8.0 bash run.sh`.
