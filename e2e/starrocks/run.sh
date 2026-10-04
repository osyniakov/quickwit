#!/usr/bin/env bash
# End-to-end driver:
#   1. Bring up Quickwit + ES-compat shim + StarRocks.
#   2. Create the Quickwit `events` index and ingest sample data.
#   3. Probe each ES endpoint StarRocks's connector relies on.
#   4. Create the StarRocks ES catalog and run analytical queries.
# All output is appended to artifacts/run.log. Failures don't abort the
# script -- the goal is to surface every gap in one pass.

set -u

ROOT="$(cd "$(dirname "$0")" && pwd)"
ART="$ROOT/artifacts"
LOG="$ART/run.log"
mkdir -p "$ART"
: >"$LOG"

# Default to the edge image since the v0.8.0 release predates several ES
# endpoints StarRocks needs. Override with QW_VERSION in .env or env.
export QW_VERSION="${QW_VERSION:-edge}"

log() { echo "[$(date -Is)] $*" | tee -a "$LOG"; }
section() { log ""; log "============================================================"; log "$*"; log "============================================================"; }

run() {
    log "+ $*"
    if ! "$@" >>"$LOG" 2>&1; then
        log "  -> exit $?"
        return 1
    fi
}

cd "$ROOT"

section "1. Boot stack (QW_VERSION=$QW_VERSION)"
run docker compose down -v --remove-orphans || true
run docker compose up -d --build

log "Waiting for Quickwit + proxy + StarRocks FE to become healthy..."
for i in $(seq 1 90); do
    state=$(docker compose ps --format '{{.Name}} {{.Health}}' 2>/dev/null || true)
    log "  health: $(echo "$state" | tr '\n' '|')"
    if echo "$state" | grep quickwit | grep -q healthy && \
       echo "$state" | grep es_proxy | grep -q healthy && \
       echo "$state" | grep starrocks | grep -q healthy; then
        log "  all healthy"
        break
    fi
    sleep 5
done

section "2. Create Quickwit index"
run curl -sS -X POST -H 'Content-Type: application/yaml' \
    --data-binary @quickwit/index_config.yaml \
    http://127.0.0.1:7280/api/v1/indexes

section "3. Ingest documents"
run curl -sS -X POST -H 'Content-Type: application/json' \
    --data-binary @data/events.ndjson \
    'http://127.0.0.1:7280/api/v1/_elastic/_bulk?refresh=true'
log "Sleeping 8s for the indexer to flush a split..."
sleep 8

section "4. Sanity check via Quickwit's native ES route"
run curl -sS -X POST -H 'Content-Type: application/json' \
    --data '{"query":{"match_all":{}}}' \
    'http://127.0.0.1:7280/api/v1/_elastic/events/_search'

section "5. Probe ES endpoints via shim"
bash scripts/probe_es_api.sh >>"$LOG" 2>&1 || true

section "6. Create StarRocks ES catalog"
docker compose exec -T starrocks mysql -h127.0.0.1 -P9030 -uroot \
    < starrocks/create_catalog.sql >>"$LOG" 2>&1 || log "  -> create_catalog.sql failed"

section "7. Run StarRocks SQL queries"
# The first few statements (SHOW DATABASES / SHOW TABLES / DESC) only need
# the FE; SELECT statements need a healthy BE (which requires nofile>=60000).
docker compose exec -T starrocks mysql -h127.0.0.1 -P9030 -uroot \
    < starrocks/queries.sql >>"$LOG" 2>&1 || log "  -> queries.sql failed (expected if BE is down)"

section "8. Capture container logs"
for svc in quickwit es_proxy starrocks; do
    log "--- $svc logs (tail) ---"
    docker compose logs --tail 60 "$svc" >>"$LOG" 2>&1 || true
done

section "Done. Artifacts: $LOG"
