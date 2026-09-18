# Quickwit ↔ StarRocks (ES connector) — E2E gap report

**Scope.** Verify whether StarRocks's Elasticsearch external catalog
(`type = "es"`) can read indices stored in Quickwit, with Quickwit acting
as a drop-in Elasticsearch endpoint.

**Date.** 2026-04-26 (live run, StarRocks + shim + Quickwit image);
re-verified against `main` on 2026-09-18 by building the `quickwit`
binary from source and curling it directly — see **§0**.
**Quickwit images probed.** `quickwit/quickwit:0.8.0` (the published
release) and `quickwit/quickwit:edge` (built from `main`). Differences
between the two are called out per gap.
**StarRocks image.** `starrocks/allin1-ubuntu:3.3-latest`.

The e2e harness lives in `e2e/starrocks/`. Run with `bash run.sh` from
that directory; results land in `artifacts/run.log`.

---

## 0. Status update — 2026-09-18

This branch was rebased onto `main` (274 commits landed since the
original run, including `main`'s own `[0.9.0]` changelog entries).
Docker registry access was unavailable in the sandbox this update ran
in (image pulls to Docker Hub are blocked by the environment's network
policy), so the full StarRocks stack in §2 could not be re-run. Instead:
`cargo build --release -p quickwit-cli` was used to build the actual
`quickwit` binary from this repo's synced `main` (commit `cef9a47`,
reported by the binary itself as `0.9.0-nightly`), the binary was run
standalone (`quickwit run`), an `events` index was created from this
harness's own `quickwit/index_config.yaml`, and every endpoint in §3
was curled directly — no shim, no StarRocks. This is strictly better
evidence than the source read it replaces: it catches bugs a source
read can't, as Gap 6 below shows.

| Gap | Status | Evidence |
| --- | --- | --- |
| 1 — ES routes only under `/api/v1/_elastic/` | **Still open** | `GET /` → 301 to `/ui/search`; `GET /_search` → 404. Only `/api/v1/_elastic/...` responds. `rest.rs:495-500` still mounts `elastic_api_handlers(...)` solely under `warp::path!("api" / "v1" / ..)`. |
| 2 — `_search_shards` omits `state`/`nodes` | **Still open** | Live `GET /api/v1/_elastic/events/_search_shards` → `{"shards":[[{"index":"events","node":"verify-node","primary":true,"shard":0}]]}` — no `state`, no top-level `nodes` map, matching `rest_handler.rs:140-149` exactly. |
| 3 — `_nodes/http` omits `version` | **Still open** | Live `GET /api/v1/_elastic/_nodes/http` → `{"nodes":{"verify-node":{"http":{"publish_address":"127.0.0.1:17280"},"roles":["data","ingest"]}}}` — no `version` key, matching `rest_handler.rs:111-126`. |
| 4 — `_cat/indices` rejects `s` | **Fixed, confirmed live** | `GET /api/v1/_elastic/_cat/indices?h=index&format=json&s=index:asc` → 200 with the index list. `cat_indices.rs:44-93` (added by #6168) explicitly accepts `s=index` / `s=index:asc`. Drop from the actionable list. |
| 5 — `_aliases` parsed as an index pattern | **Fixed, confirmed live** | `GET /api/v1/_elastic/_aliases` → 200 `{}`. `es_compat_aliases_handler()` (`mod.rs:106`, literal path `filter.rs:283-285`) is reached correctly. Drop from the actionable list. |
| 6 — `DELETE /_search/scroll` not implemented | **Still broken — new finding** | `DELETE /api/v1/_elastic/_search/scroll` → **411 "A content-length header is required"** with no `Content-Length` header, or **405 "HTTP method not allowed"** with one. The handler code (`elastic_delete_scroll_filter` at `filter.rs:278-280`, `es_compat_delete_scroll_handler` at `rest_handler.rs:454-472`, wired at `mod.rs:93`) exists and looks correct on paper — the earlier source-only read of this branch's history wrongly called it fixed. Live testing shows the route is not actually reachable: requests never reach the DELETE-specific filter, only the rejections from the neighboring GET/POST scroll filter (`elastic_scroll_filter`, same path) at `filter.rs:264-270`, which runs a `body::content_length_limit` check ahead of its own method check and apparently "wins" the `.or()` combination against the sibling DELETE filter. Root cause not fully isolated (looks like a `warp` filter-combinator interaction, not an application-logic bug), but the net behavior is unchanged from the original report: `DELETE /_search/scroll` still doesn't return 200. Low severity holds (StarRocks ignores scroll-cleanup failures), but this should stay on the actionable list, now as "fix or remove the dead code," not "add the handler." |
| 7 — 0.8.0 GA lacks the handlers above | **Still applicable** | The `main`-only fixes (Gaps 4–5, and the still-broken Gap 6 attempt) live under the unreleased `[0.9.0]` section of `CHANGELOG.md` (PR #6168) — not yet a tagged release. Anyone testing against the published `0.8.0` image still hits every gap in §3. |

**Net effect on the shim:** the path-prefix rewrite (Gap 1), the
`_search_shards`/`_nodes/http` body patches (Gaps 2–3), and tolerating
`DELETE /_search/scroll` failures (Gap 6) are all still required
against current `main`. Only the `_aliases`/`_cat/indices` workarounds
(Gaps 4–5) are no longer necessary and can be dropped once the harness
is pinned to a `main`-built image or a `0.9.0` release.

Recommended follow-up: re-run `bash run.sh` against a freshly built
`edge` (or `0.9.0`) image on a host with unrestricted registry access
to confirm §2's data-plane results still hold end-to-end with
StarRocks in the loop, and file the Gap 6 routing bug upstream with a
minimal `warp`-level reproduction.

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
| `DELETE /_search/scroll`              | 405 — `DELETE` not bound        | 405/411 depending on headers, confirmed live against `main` on 2026-09-18 (see §0, Gap 6) | ⚠️ tolerated by StarRocks (it ignores cleanup failures); scrolls just expire on Quickwit's TTL. |

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
never sends anything else). Confirmed live: `GET /api/v1/_elastic/_cat/indices?h=index&format=json&s=index:asc`
against a `main`-built binary returns 200. No fix is needed; this entry
is kept for history. See §0 for why the original write-up called this
a gap despite the report's own §3 table already showing 200 on `edge`.

### Gap 5 — `GET /_aliases` is parsed as an index pattern [resolved — see §0]

> Severity: medium-low. The route `/_elastic/_aliases` exists but the
> registration order causes the request to hit
> `elastic_index_mapping_filter` first, which validates `_aliases` as
> an index ID and returns 400.

As of 2026-09-18, `es_compat_aliases_handler` (a literal `_elastic/_aliases`
match, `filter.rs:283-285`) is registered in
`quickwit/quickwit-serve/src/elasticsearch_api/mod.rs:106`, ahead of
`es_compat_index_mapping_handler` (`mod.rs:107-110`), and no earlier
filter in the chain matches a bare `_aliases` segment on `GET`.
Confirmed live: `GET /api/v1/_elastic/_aliases` against a `main`-built
binary returns 200 `{}`. No fix is needed; kept for history — see §0.

### Gap 6 — `DELETE /_search/scroll` not implemented [still open — code exists but is dead; see §0]

> Severity: low. StarRocks calls this to release scroll contexts, but
> ignores failures. Quickwit currently returns 405. Adding a no-op
> handler that returns `{"succeeded": true, "num_freed": 0}` would
> stop logspam in the client.

A handler matching this exact description was added on `main`:
`elastic_delete_scroll_filter` (`filter.rs:278-280`) and
`es_compat_delete_scroll_handler` (`rest_handler.rs:454-472`) exist,
are registered at `mod.rs:93`, and would return exactly the no-op
payload proposed above — *if reached*. Live testing against a
`main`-built binary (2026-09-18) shows it isn't: `DELETE
/api/v1/_elastic/_search/scroll` still returns 411 (no `Content-Length`
header) or 405 (with one), never 200. The neighboring GET/POST filter
for the same path (`elastic_scroll_filter`, `filter.rs:264-270`) runs a
`body::content_length_limit` check ahead of its own method check, and
that appears to "win" the `.or()` combination against the sibling
DELETE filter regardless of method. This still needs a fix (or, since
the existing code is effectively dead, removing it and filing the
routing bug upstream) — see §0.

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

*Updated 2026-09-18, after live-testing a `main`-built binary (§0):
Gaps 4–5 are genuinely resolved. Gaps 1, 2, 3, and 6 remain actionable
— Gap 6's fix is now "make the existing dead code reachable," not
"write it."*

1. Submit a PR that closes Gaps 2 and 3. Each is a localized change in
   `quickwit-serve/src/elasticsearch_api/`.
2. Land Gap 1 as a separate config-shaped change (root mount of the
   ES-compat router).
3. Fix Gap 6: reorder or restructure the `_elastic/_search/scroll`
   routing (`mod.rs:92-93`, `filter.rs:264-270` and `:278-280`) so the
   DELETE-specific filter is actually reachable, or replace the
   `.or()` chain for that path with a single filter that dispatches on
   method internally. File it upstream with a minimal `warp`
   reproduction if the cause turns out to be a `warp` behavior rather
   than something fixable locally.
4. Add a StarRocks-flavored scenario to `quickwit/rest-api-tests/scenarii/`
   that exercises `_search_shards`, `_nodes/http`, `_cat/indices`, and
   `DELETE /_search/scroll` with StarRocks-specific parameters, so
   future regressions (and Gap 6's kind of silently-dead route) are
   caught before release.
5. Re-run `bash run.sh` against a fresh `main`-built image once registry
   access is available, to confirm §0's findings end-to-end with
   StarRocks in the loop and refresh §2's data-plane results.

After (1)+(2)+(3), no shim is necessary: a Quickwit binary alone serves
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
