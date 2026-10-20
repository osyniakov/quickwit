#!/usr/bin/env bash
# Probe Quickwit's ES-compat surface *directly* (no shim), at both the
# root path a stock ES client uses and Quickwit's `/api/v1/_elastic`
# prefix. Prints one status line per request plus a body snippet, so the
# raw wire format can be compared against what StarRocks parses.
#
# Usage: QW=http://127.0.0.1:7280 INDEX=events bash scripts/probe_direct.sh

set -u

QW="${QW:-http://127.0.0.1:7280}"
INDEX="${INDEX:-events}"
TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

probe() {
    local method="$1" url="$2" data="${3:-}"
    local status
    if [[ -n "$data" ]]; then
        status=$(curl -sS -o "$TMP" -w '%{http_code}' -X "$method" \
            -H 'Content-Type: application/json' --data "$data" "$url" || true)
    else
        status=$(curl -sS -o "$TMP" -w '%{http_code}' -X "$method" "$url" || true)
    fi
    printf '\n=== %s %s -> %s ===\n%s\n' "$method" "${url#"$QW"}" "$status" "$(head -c 600 "$TMP")"
}

for prefix in "" "/api/v1/_elastic"; do
    printf '\n################ prefix: "%s" ################\n' "$prefix"
    probe GET "$QW$prefix/"
    probe GET "$QW$prefix/_nodes/http"
    probe GET "$QW$prefix/_cat/indices?h=index&format=json&s=index:asc"
    probe GET "$QW$prefix/_aliases"
    probe GET "$QW$prefix/$INDEX/_mapping"
    probe GET "$QW$prefix/$INDEX/_search_shards"
    probe POST "$QW$prefix/$INDEX/_search?scroll=1m&preference=_shards:0" \
        '{"size":3,"query":{"match_all":{}},"sort":["_doc"]}'
    scroll_id=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1])).get('_scroll_id',''))" "$TMP" 2>/dev/null || true)
    if [[ -n "$scroll_id" ]]; then
        probe POST "$QW$prefix/_search/scroll" "{\"scroll\":\"1m\",\"scroll_id\":\"$scroll_id\"}"
        probe DELETE "$QW$prefix/_search/scroll" "{\"scroll_id\":[\"$scroll_id\"]}"
    fi
done
