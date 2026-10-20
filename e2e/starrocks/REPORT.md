# Quickwit ↔ StarRocks (ES connector) — E2E gap report

**Scope.** Verify whether StarRocks's Elasticsearch external catalog
(`type = "es"`) can read indices stored in Quickwit, with Quickwit acting
as a drop-in Elasticsearch endpoint.

**Revision history.**

| Date       | Quickwit under test | What changed |
| ---------- | ------------------- | ------------ |
| 2026-04-26 | `0.8.0` image, `edge` image (`main` @ `d27d240`) | Initial report; full stack run incl. StarRocks FE. |
| 2026-10-20 | `main` @ `d140cfc` (0.9.1 + 311 commits), built from source | Re-evaluated every gap against current `main`. Gaps 4, 5 and 7 are closed; Gap 6 root cause corrected; Gaps 1–3 still open. |

**StarRocks image.** `starrocks/allin1-ubuntu:3.3-latest`.

The e2e harness lives in `e2e/starrocks/`. Run with `bash run.sh` from
that directory; results land in `artifacts/run.log`. A shim-less probe of
the raw Quickwit wire format is in `scripts/probe_direct.sh`.

---

## 0. What changed since the 2026-04-26 run

- **Quickwit 0.9.0 / 0.9.1 shipped** and includes #6168 (the ES endpoints
  for Trino/StarRocks-style connectors). Gap 7 is closed for anyone on a
  current GA release; `QW_VERSION=0.9.1` is the recommended GA target.
- **Gap 4 (`_cat/indices?s=…`) and Gap 5 (`_aliases`) are closed.** Both
  were fixed by #6168 and return 200 on current `main`; the first report
  had already noted this for `edge`, but listed them as open gaps.
- **Gap 6 root cause corrected.** The `DELETE /_search/scroll` handler
  *does* exist and is registered; it is shadowed by the scroll handler
  that precedes it (see §4). Verified with a local patch.
- **`_mapping` now accepts query parameters** (#6443,
  `IndexMappingQueryParams`: `fields`/`field_patterns`,
  `start_timestamp`, `end_timestamp`). Unknown parameters are ignored, so
  StarRocks's plain `GET /<index>/_mapping` is unaffected. The response
  shape is unchanged.
- **New `tie_breaker` field type** (#6859, #6882) is exposed in `_mapping`
  as `long`, so it appears in StarRocks as a `BIGINT` column. Opt-in only;
  no impact on existing indexes.
- **MCP endpoint moved to the root path `/mcp`** (#6863). Not used by
  StarRocks, but it is the first REST surface served outside `/api/v1`,
  which is a useful precedent for Gap 1.
- `track_total_hits` in the URL is now parsed correctly (`true`, `false`,
  integers). Not sent by StarRocks's scan path.

No change on `main` regressed any endpoint StarRocks depends on.

## 1. Architecture of the test

```
+-------------------+        +--------------------+        +--------------+
|     Quickwit      | <----- |  ES-compat shim    | <----- |  StarRocks   |
|  (port 7280)      |        |  (port 9200)       |        |  (FE+BE,     |
|  ES routes under  |        |  - rewrites paths  |        |   Java)      |
|  /api/v1/_elastic |        |  - patches bodies  |        |              |
+-------------------+        +--------------------+        +--------------+
```

The shim is a small stdlib Python proxy (`proxy/es_compat_proxy.py`). It
exists because two classes of incompatibility prevent StarRocks from
talking to Quickwit directly:

1. **Path prefix.** Quickwit hosts every ES-compatible endpoint under
   `/api/v1/_elastic/...`, so a vanilla ES client requesting
   `GET /events/_mapping` 404s. The shim prepends the prefix.
2. **Response shape.** Two endpoints are missing fields the StarRocks
   parser dereferences without null-checks (see §3).

Both are still required on current `main`.

## 2. End-to-end results

### 2.1 Canonical stack run (2026-04-26, unchanged)

| Phase                                       | Result | Notes |
| ------------------------------------------- | :----: | ----- |
| Boot Quickwit + shim + StarRocks FE         | ✅ | StarRocks BE crash-loops in the sandbox (see §5). |
| Create `events` index in Quickwit           | ✅ | Native REST, not via ES API. |
| Bulk-ingest 10 docs through `_bulk`         | ✅ | |
| Sanity search via Quickwit's native ES route | ✅ | Returns 10 hits. |
| Probe ES surface via the shim               | ✅ (with shim) | See §3. |
| `CREATE EXTERNAL CATALOG qw_es … type='es'` | ✅ | |
| `SHOW DATABASES`, `SHOW TABLES`             | ✅ | Lists all Quickwit indices, including the `otel-*` system ones. |
| `DESC events`                               | ✅ | `level/service/message → VARCHAR`, `ts → DATETIME`, `latency_ms → DOUBLE`, `status → BIGINT`. |
| `SELECT COUNT(*) FROM events`               | ❌ in sandbox | `No Alive backends or compute nodes` — BE never starts (nofile limit, §5). |
| Predicate / aggregate queries               | ❌ in sandbox | Same root cause. |

### 2.2 Re-run against `main` @ `d140cfc` (2026-10-20)

Docker Hub returned `429 Too Many Requests` for both the Quickwit and
StarRocks images from this environment, so the full stack could not be
brought up. Instead, Quickwit was built from the merged source
(`cargo build -p quickwit-cli`), run as a single node, loaded with the
same `events` index and NDJSON sample, and probed directly with
`scripts/probe_direct.sh` (no shim). This exercises exactly the HTTP
surface StarRocks's `EsRestClient` / `EsScanReader` touch; the StarRocks
side of the contract is unchanged since the first run.

| Request (as StarRocks sends it) | At `/` | At `/api/v1/_elastic` | Body OK for StarRocks? |
| ------------------------------- | :----: | :-------------------: | ---------------------- |
| `GET /`                          | 301 | 200 | ✅ |
| `GET /_nodes/http`               | 404 | 200 | ❌ no `nodes[*].version` (Gap 3) |
| `GET /_cat/indices?h=index&format=json&s=index:asc` | 404 | 200 | ✅ |
| `GET /_aliases`                  | 404 | 200 (`{}`) | ✅ |
| `GET /events/_mapping`           | 404 | 200 | ✅ six fields, correct ES types |
| `GET /events/_search_shards`     | 404 | 200 | ❌ no `state`, no `nodes` map (Gap 2) |
| `POST /events/_search?scroll=1m&preference=_shards:0` | 404 | 200 | ✅ returns `_scroll_id`, `hits.total = 10` |
| `POST /_search/scroll`           | 404 | 200 | ✅ |
| `DELETE /_search/scroll`         | 404 | **405** | ⚠️ tolerated by StarRocks (Gap 6) |

Actual bodies of the two endpoints that still break StarRocks:

```json
GET /api/v1/_elastic/_nodes/http
{"nodes":{"qw-node-1":{"http":{"publish_address":"127.0.0.1:7280"},"roles":["data","ingest"]}}}

GET /api/v1/_elastic/events/_search_shards
{"shards":[[{"index":"events","node":"qw-node-1","primary":true,"shard":0}]]}
```

**Bottom line.** Unchanged from April: with the shim, the metadata flow
(catalog → database → table → schema) works and every data-plane
endpoint returns 200. Without the shim, StarRocks cannot connect at all
(Gap 1), and with only path rewriting it fails on `_nodes/http` and
`_search_shards` (Gaps 2, 3).

## 3. Endpoint-by-endpoint compatibility

| Endpoint StarRocks calls              | Quickwit `0.8.0`                 | Quickwit `0.9.x` / `main` @ `d140cfc` | After shim |
| ------------------------------------- | -------------------------------- | -------------------------------------- | :--------: |
| `GET /`                               | 200, but at `/api/v1/_elastic`   | same                                   | ✅ |
| `GET /_nodes/http`                    | 404 (handler not registered)     | 200, missing `nodes[*].version`        | ✅ (shim injects `version: "7.10.2"`) |
| `GET /_cat/indices?h=…&format=json&s=index:asc` | 400 — `s` rejected     | 200                                    | ✅ |
| `GET /_aliases`                       | 400 — parsed as index pattern    | 200 (`{}`)                             | ✅ |
| `GET /<index>/_mapping`               | 404 (handler not registered)     | 200                                    | ✅ |
| `GET /<index>/_search_shards`         | 404 (handler not registered)     | 200, missing `state` and `nodes` map   | ✅ (shim injects them) |
| `POST /<index>/_search?scroll=…`      | 200                              | 200                                    | ✅ |
| `POST /_search/scroll`                | 200                              | 200                                    | ✅ |
| `DELETE /_search/scroll`              | 405                              | 405 (handler shadowed)                 | ⚠️ tolerated; scrolls expire on TTL |

## 4. Gaps in Quickwit

Status summary:

| # | Gap | Severity | Status on `main` @ `d140cfc` |
| - | --- | -------- | ---------------------------- |
| 1 | ES routes only under `/api/v1/_elastic/` | high | **Open** |
| 2 | `_search_shards` omits `state` and `nodes` | high | **Open** |
| 3 | `_nodes/http` omits `version` | high | **Open** |
| 4 | `_cat/indices` rejects `s` | medium | ✅ Closed (fixed in 0.9.0, #6168) |
| 5 | `_aliases` parsed as index pattern | medium-low | ✅ Closed (fixed in 0.9.0, #6168) |
| 6 | `DELETE /_search/scroll` returns 405 | low | **Open** — root cause corrected below |
| 7 | GA release lacks the ES handlers | — | ✅ Closed (0.9.0 / 0.9.1 ship #6168) |

### Gap 1 — All ES-compatible routes live under `/api/v1/_elastic/` (open)

> Severity: high. Affects every standard ES client, not just StarRocks.

Source: `quickwit/quickwit-serve/src/rest.rs` — `api_v1_routes()` mounts
`elastic_api_handlers(...)` only under `warp::path!("api" / "v1" / ..)`.
On current `main`, `GET /` returns 301 and every other ES path at the
root returns 404.

Existing ES clients (StarRocks, Trino, Logstash output, Vector ES sink,
etc.) hard-code the Elasticsearch URL layout and won't accept a custom
prefix.

**Recommended fix.** Additionally mount the ES-compat router at the root
path, gated by a config knob (e.g. `rest.es_compat_root_mount: bool`,
default `false` to avoid shadowing the UI redirect at `/`). `/mcp` (#6863)
already establishes a precedent for serving a REST surface outside
`/api/v1`.

### Gap 2 — `GET /_search_shards` omits `state` and the `nodes` map (open)

> Severity: high. Blocks `DESC <table>` and `SELECT *` in StarRocks.

Source: `quickwit/quickwit-serve/src/elasticsearch_api/rest_handler.rs`,
`es_compat_search_shards()` (unchanged since April):

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

StarRocks's parser unconditionally reads `shard.getString("state")`
(`EsShardPartitions.java:90`) and
`nodes.getJSONObject(node_id).getJSONObject("attributes")`
(`EsShardRouting.java:47`).

**Recommended fix** (≈15 lines):

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

### Gap 3 — `GET /_nodes/http` omits `nodes[*].version` (open)

> Severity: high. Triggers a `NullPointerException` in
> `EsMajorVersion.parse` even when `es.nodes.wan.only=true`.

Source: `rest_handler.rs`, `es_compat_nodes_info()` (unchanged). The
same `version` string proposed in Gap 2 should be added here; ideally
both reuse the version string already reported by `GET /`
(`es_compat_cluster_info`) so the three endpoints stay consistent.

### Gap 4 — `_cat/indices` rejects `s` (closed)

Fixed in 0.9.0 (#6168). `CatIndexQueryParams::validate()` in
`model/cat_indices.rs` now accepts `s=index` and `s=index:asc`, which is
what StarRocks sends; other sort keys are still rejected with 400.
Verified 200 on `main`.

### Gap 5 — `GET /_aliases` parsed as an index pattern (closed)

Fixed in 0.9.0 (#6168). `_aliases` has its own route and returns `{}`.
Verified 200 on `main`.

### Gap 6 — `DELETE /_search/scroll` returns 405 (open, root cause corrected)

> Severity: low. StarRocks calls this to release scroll contexts but
> ignores failures; scrolls expire on Quickwit's TTL.

The April report stated the handler was a stub. That is wrong: a no-op
`es_compat_delete_scroll_handler` returning
`{"succeeded": true, "num_freed": 0}` exists and is registered in
`elasticsearch_api/mod.rs`. It is never reached because the preceding
`es_compat_scroll_handler` (`rest_handler.rs`) ends in
`.recover(recover_fn)`. The scroll filter matches the path
`_elastic/_search/scroll`, rejects `DELETE` with `MethodNotAllowed`, and
`recover_fn` converts that rejection into a final 405 *response*, so
warp's `.or()` chain never tries the next handler.

Verification on `main`: swapping the registration order makes `DELETE`
return 200 with the expected body, but then `GET`/`POST` on the same
path return 405 — the shadowing is symmetric. Reordering alone is
therefore **not** a fix.

**Recommended fix.** Either drop the per-handler `.recover(recover_fn)`
from `es_compat_scroll_handler` and `es_compat_delete_scroll_handler`
(the router already applies `.recover(recover_fn)` at the end of
`elastic_api_handlers`), or fold both methods into one filter on
`_elastic/_search/scroll` that dispatches on method. The same
per-handler `recover` pattern may shadow other same-path/different-method
pairs and is worth auditing.

### Gap 7 — GA release lacks the ES handlers (closed)

Quickwit 0.9.0 (and the 0.9.1 security patch) ship #6168. Anyone on a
current GA release sees the same endpoint surface as `main`. Only
deployments still on `0.8.x` are affected.

## 5. Environment limitations

**StarRocks BE (both runs).** The BE refuses to start unless the
open-files soft limit is ≥ 60000 (`storage_engine.cpp:420`). The April
sandbox capped the hard limit at 4096; the October environment caps it
at 20000. On any host with `ulimit -n ≥ 60000` (the AllInOne README's
documented prerequisite) the BE comes up; the compose file keeps the
`ulimits.nofile` directive so it works there.

**Docker Hub rate limit (October run).** Image pulls returned 429, so
the October run used a source build of Quickwit instead of the `edge`
image and could not re-run the StarRocks half. The Quickwit-side
evidence (§2.2) is sufficient to assess every gap, because none of the
StarRocks request shapes changed.

## 6. Recommended follow-up

1. Submit a PR that closes Gaps 2, 3 and 6. All three are localized to
   `quickwit-serve/src/elasticsearch_api/` (Gap 6 is a one-line removal
   of `.recover(recover_fn)` on two handlers plus a test that issues
   `GET`, `POST` and `DELETE` on `_search/scroll`).
2. Land Gap 1 as a separate, config-gated change (root mount of the
   ES-compat router).
3. Add a StarRocks-flavored scenario to
   `quickwit/rest-api-tests/scenarii/es_compatibility/` that asserts the
   fields StarRocks dereferences (`_nodes/http` → `version`,
   `_search_shards` → `state` and `nodes.<id>.attributes`) and the
   `DELETE /_search/scroll` status, so regressions are caught before
   release.

After (1)+(2), no shim is necessary: a Quickwit binary alone serves
StarRocks correctly.

## 7. How to reproduce

```bash
cd e2e/starrocks
bash run.sh                       # full stack, QW_VERSION=edge by default
QW_VERSION=0.9.1 bash run.sh      # against the current GA image
less artifacts/run.log

# Shim-less probe of the raw Quickwit wire format (any running Quickwit
# with the `events` index loaded):
QW=http://127.0.0.1:7280 bash scripts/probe_direct.sh
```

To reproduce the October run without Docker: build with
`cargo build -p quickwit-cli --bin quickwit` (needs `protoc`), start
`quickwit run` with a node config whose `data_dir` exists, create the
index from `quickwit/index_config.yaml`, ingest `data/events.ndjson` via
`/api/v1/_elastic/_bulk?refresh=true`, then run `probe_direct.sh`.
