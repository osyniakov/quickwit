SET CATALOG qw_es;
SHOW DATABASES;

USE default_db;
SHOW TABLES;

-- Schema discovery via _mapping. Exercises:
--   GET /_nodes/http       (StarRocks needs nodes[*].version → patched by shim)
--   GET /<index>/_mapping  (Quickwit-native, OK once index has a split)
--   GET /<index>/_search_shards (StarRocks needs shard.state and nodes map →
--                               patched by shim)
DESC events;

-- Full table read via scroll API. Requires a healthy BE.
SELECT COUNT(*) AS total_rows FROM events;

-- Predicate pushdown to Quickwit (term query on `level`).
SELECT level, COUNT(*) AS n FROM events GROUP BY level ORDER BY level;

-- Numeric range pushdown.
SELECT service, AVG(latency_ms) AS avg_latency
FROM events
WHERE status >= 500
GROUP BY service
ORDER BY service;

-- Top-N with secondary sort.
SELECT ts, level, service, message, latency_ms
FROM events
ORDER BY latency_ms DESC
LIMIT 5;
