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

//! Fuzzes the Quickwit query language.
//!
//! Public surface: the `query` parameter of `GET /api/v1/{index}/search` and the
//! `q` parameter of the Elasticsearch-compatible `_search` endpoint. The string is
//! fully attacker-controlled and reaches the parser before any authorization or
//! schema check, so a panic here is a remotely triggerable crash.

#![no_main]

use libfuzzer_sys::fuzz_target;
use quickwit_query::BooleanOperand;
use quickwit_query::query_ast::UserInputQuery;

/// Stands in for the `default_search_fields` a doc mapping would supply. Two entries
/// keep the "multiple default fields" branches reachable, and the nested name keeps
/// the JSON-field path branches reachable.
const DEFAULT_SEARCH_FIELDS: [&str; 2] = ["body", "attributes.level"];

fuzz_target!(|data: &[u8]| {
    let Ok(user_text) = std::str::from_utf8(data) else {
        return;
    };
    let default_search_fields = DEFAULT_SEARCH_FIELDS.map(str::to_string);
    let user_input_query = UserInputQuery {
        user_text: user_text.to_string(),
        default_fields: None,
        default_operator: BooleanOperand::Or,
        lenient: false,
    };
    let _ = user_input_query.parse_user_query(&default_search_fields);
});
