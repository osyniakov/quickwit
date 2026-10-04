#!/usr/bin/env bash
# Exercise every ES endpoint StarRocks's connector calls, going through the
# nginx proxy. Each request reports HTTP status + a short snippet of the body
# so we can see which endpoints are compatible and which aren't.

set -u

ES="http://127.0.0.1:9200"
INDEX="${INDEX:-events}"

probe() {
    local method="$1" path="$2" data="${3:-}"
    local label="${method} ${path}"
    local out status body
    if [[ -n "$data" ]]; then
        out=$(curl -sS -o /tmp/es_body.$$ -w '%{http_code}' \
              -X "$method" -H 'Content-Type: application/json' \
              --data "$data" "${ES}${path}" || true)
    else
        out=$(curl -sS -o /tmp/es_body.$$ -w '%{http_code}' \
              -X "$method" "${ES}${path}" || true)
    fi
    status="$out"
    body=$(head -c 400 /tmp/es_body.$$ 2>/dev/null || echo "")
    rm -f /tmp/es_body.$$
    printf '\n=== %s -> %s ===\n%s\n' "$label" "$status" "$body"
}

probe GET "/"
probe GET "/_nodes/http"
probe GET "/_cat/indices?h=index&format=json&s=index:asc"
probe GET "/_aliases"
probe GET "/${INDEX}/_mapping"
probe GET "/${INDEX}/_search_shards"
probe POST "/${INDEX}/_search?scroll=1m" '{"size":3,"query":{"match_all":{}}}'

# Use the last scroll_id from the previous response (best effort).
SCROLL_ID=$(curl -sS -X POST -H 'Content-Type: application/json' \
    --data '{"size":3,"query":{"match_all":{}}}' \
    "${ES}/${INDEX}/_search?scroll=1m" | python3 -c \
    "import sys,json; print(json.load(sys.stdin).get('_scroll_id',''))" 2>/dev/null || true)

if [[ -n "${SCROLL_ID:-}" ]]; then
    probe POST "/_search/scroll" "{\"scroll\":\"1m\",\"scroll_id\":\"${SCROLL_ID}\"}"
    probe DELETE "/_search/scroll" "{\"scroll_id\":[\"${SCROLL_ID}\"]}"
else
    printf '\n=== /_search/scroll skipped (no scroll_id) ===\n'
fi
