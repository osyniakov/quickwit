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

#![no_main]
use libfuzzer_sys::fuzz_target;
use quickwit_query::BooleanOperand;
use quickwit_query::query_ast::UserInputQuery;

fuzz_target!(|data: &[u8]| {
    if let Ok(query_str) = std::str::from_utf8(data) {
        let query = UserInputQuery {
            user_text: query_str.to_string(),
            default_fields: Some(vec!["body".to_string()]),
            default_operator: BooleanOperand::Or,
            lenient: true,
        };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            query.parse_user_query(&["body".to_string()])
        }));
    }
});
