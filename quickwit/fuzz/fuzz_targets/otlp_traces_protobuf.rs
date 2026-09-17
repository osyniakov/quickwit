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

//! Fuzzes OTLP trace ingestion over protobuf.
//!
//! Public surface: the OTLP gRPC endpoint on port 4317 and `POST /otlp/v1/traces` with
//! a protobuf payload. Kept separate from the JSON target so the two corpora, one
//! binary and one textual, do not pollute each other.

#![no_main]

use libfuzzer_sys::fuzz_target;
use quickwit_opentelemetry::otlp::parse_otlp_spans_protobuf;

fuzz_target!(|data: &[u8]| {
    let Ok(json_span_iterator) = parse_otlp_spans_protobuf(data) else {
        return;
    };
    for (json_span, num_bytes) in json_span_iterator {
        std::hint::black_box((json_span, num_bytes));
    }
});
