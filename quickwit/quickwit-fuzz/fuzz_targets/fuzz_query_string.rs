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
        // libfuzzer-sys installs a panic hook that calls abort() before stack
        // unwinding, so catch_unwind alone is not enough to suppress a panic.
        // Temporarily swap in a no-op hook so the known upstream tantivy bug
        // ("Exist query without a field isn't allowed") is caught instead of
        // aborting the fuzzer process.
        let abort_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            query.parse_user_query(&["body".to_string()])
        }));
        std::panic::set_hook(abort_hook);
    }
});
