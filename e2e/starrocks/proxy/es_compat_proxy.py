#!/usr/bin/env python3
"""
ES-compatibility shim that fronts Quickwit on behalf of StarRocks.

Two responsibilities:

1. Path translation: standard Elasticsearch URLs (e.g. /, /<index>/_search,
   /_search/scroll) are rewritten to Quickwit's /api/v1/_elastic/... routes.

2. Body shaping for known incompatibilities (today, just one):
     - Quickwit's /<index>/_search_shards omits the `state` field that
       StarRocks's EsShardPartitions parser requires.
   The shim parses each /_search_shards response and inserts
   `"state": "STARTED"` on every shard entry. It also injects a synthetic
   `nodes` map so StarRocks has something to look up node metadata in
   (only meaningful when es.nodes.wan.only=true, which we set).

Once the upstream Quickwit fixes (described in REPORT.md) ship, this shim
only needs to do path translation.
"""
from __future__ import annotations

import http.client
import json
import os
import socketserver
import sys
from http.server import BaseHTTPRequestHandler
from urllib.parse import urlsplit

UPSTREAM = os.environ.get("UPSTREAM", "http://quickwit:7280")
PREFIX = "/api/v1/_elastic"
HOP_BY_HOP = {
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "content-length",
    "content-encoding",
}


def _maybe_rewrite(path: str, payload: bytes) -> bytes:
    if path.endswith("/_search_shards"):
        return _rewrite_search_shards(payload)
    if path == "/_nodes/http":
        return _rewrite_nodes_http(payload)
    return payload


def _node_stub(node_id: str) -> dict:
    return {
        "name": node_id,
        "version": "7.10.2",
        "transport_address": "127.0.0.1:9300",
        "http_address": "127.0.0.1:9200",
        "attributes": {},
        "roles": ["data"],
    }


def _rewrite_search_shards(payload: bytes) -> bytes:
    try:
        doc = json.loads(payload)
    except Exception:
        return payload
    shards = doc.get("shards")
    if not isinstance(shards, list):
        return payload
    seen_nodes = {}
    for shard_group in shards:
        if not isinstance(shard_group, list):
            continue
        for shard in shard_group:
            if isinstance(shard, dict):
                shard.setdefault("state", "STARTED")
                node_id = shard.get("node")
                if node_id and node_id not in seen_nodes:
                    seen_nodes[node_id] = _node_stub(node_id)
    doc.setdefault("nodes", seen_nodes)
    doc.setdefault("indices", {})
    return json.dumps(doc).encode()


def _rewrite_nodes_http(payload: bytes) -> bytes:
    # StarRocks parses `nodes.<id>.version`; Quickwit omits it. Inject a stub.
    try:
        doc = json.loads(payload)
    except Exception:
        return payload
    nodes = doc.get("nodes")
    if not isinstance(nodes, dict):
        return payload
    for node_id, node in nodes.items():
        if not isinstance(node, dict):
            continue
        node.setdefault("name", node_id)
        node.setdefault("version", "7.10.2")
        node.setdefault("attributes", {})
        # Quickwit publishes http.publish_address; mirror it as
        # http_address so EsNodeInfo's parser is happy either way.
        http = node.get("http") or {}
        if isinstance(http, dict) and "publish_address" in http:
            node.setdefault("http_address", http["publish_address"])
    return json.dumps(doc).encode()


class Handler(BaseHTTPRequestHandler):
    server_version = "es_compat_proxy/0.1"

    def _proxy(self) -> None:
        url = urlsplit(UPSTREAM)
        host, port = url.hostname, url.port or 80
        path = self.path
        path_only = path.split("?", 1)[0]
        upstream_path = PREFIX if path_only == "/" else PREFIX + path
        body_len = int(self.headers.get("Content-Length", 0) or 0)
        body = self.rfile.read(body_len) if body_len else b""

        fwd_headers = {
            k: v for k, v in self.headers.items() if k.lower() != "host"
        }
        fwd_headers.pop("Content-Length", None)

        conn = http.client.HTTPConnection(host, port, timeout=120)
        try:
            conn.request(self.command, upstream_path, body=body, headers=fwd_headers)
            up = conn.getresponse()
            data = up.read()
            data = _maybe_rewrite(path_only, data)

            self.send_response(up.status)
            for header, value in up.getheaders():
                if header.lower() in HOP_BY_HOP:
                    continue
                self.send_header(header, value)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        finally:
            conn.close()

    do_GET = _proxy
    do_POST = _proxy
    do_PUT = _proxy
    do_DELETE = _proxy
    do_HEAD = _proxy

    def log_message(self, fmt, *args) -> None:
        sys.stderr.write("%s - - %s\n" % (self.address_string(), fmt % args))


class ThreadedServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = True


if __name__ == "__main__":
    addr = ("0.0.0.0", 9200)
    with ThreadedServer(addr, Handler) as srv:
        sys.stderr.write(f"es_compat_proxy listening on {addr}, upstream {UPSTREAM}\n")
        srv.serve_forever()
