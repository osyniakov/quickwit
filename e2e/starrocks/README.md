# Quickwit + StarRocks (ES connector) e2e harness

End-to-end check that StarRocks's Elasticsearch external catalog
(`type = "es"`) can read indices stored in Quickwit.

See **[REPORT.md](./REPORT.md)** for the gap analysis.

## Layout

```
docker-compose.yml      Quickwit + ES-compat shim + StarRocks AllInOne.
proxy/                  Tiny Python ES-compat shim that fronts Quickwit.
nginx/                  Legacy nginx-only translator (kept for reference;
                        unused by docker-compose.yml).
quickwit/               Index config used to create the `events` index.
data/                   NDJSON sample for `_bulk` ingestion.
starrocks/              SQL: catalog creation + analytical queries.
scripts/                Standalone ES-endpoint probe.
run.sh                  Drives the full e2e flow.
artifacts/              Output of run.sh (gitignored).
```

## Run

```bash
bash run.sh                        # uses QW_VERSION=edge
QW_VERSION=0.8.0 bash run.sh       # against the published GA image
```

`run.sh` brings up the stack, creates the index, ingests sample docs,
probes every ES endpoint StarRocks needs, then runs `create_catalog.sql`
+ `queries.sql` against StarRocks. Everything (success + failure)
is captured in `artifacts/run.log`.

## Why a shim?

Quickwit hosts every Elasticsearch-compatible endpoint under
`/api/v1/_elastic/...`, while StarRocks (and any vanilla ES client)
expects them at `/`. Two response payloads also drop fields StarRocks
parses without null-checks. The shim handles both. With the
fixes proposed in REPORT.md, the shim is no longer needed.
