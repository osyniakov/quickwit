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

//! Fuzzes OTLP/HTTP JSON log ingestion.
//!
//! Public surface: `POST /otlp/v1/logs` with a JSON payload. The iterator is drained
//! because the handler drains it too: the per-record lowering to a Quickwit document
//! is as reachable as the initial decode.

#![no_main]

use libfuzzer_sys::fuzz_target;
use quickwit_opentelemetry::otlp::parse_otlp_logs_json;

fuzz_target!(|data: &[u8]| {
    let Ok(json_log_iterator) = parse_otlp_logs_json(data) else {
        return;
    };
    for (json_log, num_bytes) in json_log_iterator {
        std::hint::black_box((json_log, num_bytes));
    }
});
