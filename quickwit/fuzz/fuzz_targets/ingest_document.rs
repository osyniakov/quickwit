// Copyright 2021-Present Datadog, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Fuzzes document parsing and mapping at ingest time.
//!
//! Public surface: the body of `POST /api/v1/{index}/ingest` and of the
//! Elasticsearch-compatible `_bulk` endpoint, one JSON document per line.
//!
//! The doc mapping below is deliberately wide: driving datetime, IP, bytes and JSON
//! values through the public ingest entry point covers the same value parsers that a
//! direct `quickwit-datetime` target would, without reaching into an internal crate.

#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use quickwit_doc_mapper::{DocMapper, DocMapperBuilder};

const DOC_MAPPING_JSON: &str = r#"{
    "mode": "dynamic",
    "timestamp_field": "timestamp",
    "index_field_presence": true,
    "store_source": true,
    "default_search_fields": ["body", "severity"],
    "field_mappings": [
        {
            "name": "timestamp",
            "type": "datetime",
            "input_formats": [
                "rfc3339",
                "iso8601",
                "rfc2822",
                "unix_timestamp",
                "%Y-%m-%d %H:%M:%S"
            ],
            "fast": true
        },
        { "name": "severity", "type": "text", "tokenizer": "raw", "fast": true },
        { "name": "body", "type": "text", "tokenizer": "default", "record": "position" },
        { "name": "count", "type": "i64", "fast": true },
        { "name": "ratio", "type": "f64" },
        { "name": "ok", "type": "bool" },
        { "name": "host_ip", "type": "ip", "fast": true },
        { "name": "payload", "type": "bytes" },
        { "name": "attributes", "type": "json", "tokenizer": "default" },
        {
            "name": "resource",
            "type": "object",
            "field_mappings": [
                { "name": "service_name", "type": "text", "tokenizer": "raw" }
            ]
        }
    ]
}"#;

/// Built once: re-deriving the tantivy schema on every iteration would dominate the
/// runtime and starve the part of the code we actually want to explore.
static DOC_MAPPER: LazyLock<DocMapper> = LazyLock::new(|| {
    let doc_mapper_builder: DocMapperBuilder = serde_json::from_str(DOC_MAPPING_JSON)
        .expect("fuzz doc mapping should be a valid doc mapper builder");
    doc_mapper_builder
        .try_build()
        .expect("fuzz doc mapping should build a valid doc mapper")
});

fuzz_target!(|data: &[u8]| {
    let Ok(json_str) = std::str::from_utf8(data) else {
        return;
    };
    let _ = DOC_MAPPER.doc_from_json_str(json_str);
});
