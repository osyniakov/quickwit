-- Drop any prior run.
DROP CATALOG IF EXISTS qw_es;

-- Create an Elasticsearch catalog pointing at the nginx proxy in front of
-- Quickwit. `es.nodes.wan.only=true` is critical: otherwise StarRocks would
-- try to read shard locations from /_search_shards and connect directly to
-- those node addresses, which won't be reachable for Quickwit.
CREATE EXTERNAL CATALOG qw_es PROPERTIES (
    "type" = "es",
    "hosts" = "http://es_proxy:9200",
    "user" = "",
    "password" = "",
    "es.net.ssl" = "false",
    "enable_docvalue_scan" = "true",
    "enable_keyword_sniff" = "true",
    "es.nodes.wan.only" = "true"
);

SHOW CATALOGS;
