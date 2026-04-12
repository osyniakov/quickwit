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

// Fuzz OTLP trace parsing (protobuf and JSON paths).
//
// The internal `otlp_value_to_json_value` helper processes OTLP `AnyValue`
// recursively (arrays and key-value lists of itself) with no depth limit.
// A crafted payload with deep nesting can cause a stack overflow — this target
// exercises that code path from both wire formats.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(iter) = quickwit_opentelemetry::otlp::parse_otlp_spans_protobuf(data) {
        for _ in iter {}
    }
    if let Ok(iter) = quickwit_opentelemetry::otlp::parse_otlp_spans_json(data) {
        for _ in iter {}
    }
});
