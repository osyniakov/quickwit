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

//! Fuzzes the Elasticsearch-compatible query DSL.
//!
//! Public surface: the `query` object in the body of `POST /_elastic/{index}/_search`.
//! Deserialization alone is not the interesting part; the lowering to Quickwit's own
//! `QueryAst` is where recursion depth, field resolution and range coercion happen.

#![no_main]

use libfuzzer_sys::fuzz_target;
use quickwit_query::ElasticQueryDsl;
use quickwit_query::query_ast::QueryAst;

fuzz_target!(|data: &[u8]| {
    let Ok(elastic_query_dsl) = serde_json::from_slice::<ElasticQueryDsl>(data) else {
        return;
    };
    let _ = QueryAst::try_from(elastic_query_dsl);
});
