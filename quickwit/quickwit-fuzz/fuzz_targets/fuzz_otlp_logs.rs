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

// Fuzz OTLP log parsing (protobuf and JSON paths).
//
// Log records from external collectors (Fluentd, Vector, etc.) arrive at the
// gRPC boundary as raw bytes.  `parse_otlp_logs_protobuf/json` deserialise the
// payload and process each LogRecord's body/attributes, where recursive
// `AnyValue` nesting has no depth guard.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(iter) = quickwit_opentelemetry::otlp::parse_otlp_logs_protobuf(data) {
        for _ in iter {}
    }
    if let Ok(iter) = quickwit_opentelemetry::otlp::parse_otlp_logs_json(data) {
        for _ in iter {}
    }
});
