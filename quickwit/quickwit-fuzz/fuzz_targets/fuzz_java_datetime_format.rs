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

// Fuzz Java SimpleDateFormat and strptime *format-string* parsing.
//
// The existing `fuzz_datetime` target tests parsing date strings with fixed
// known formats.  This target covers the orthogonal path: parsing the format
// specification itself, which uses a recursive grammar for optional groups
// `[...]` and resolves named aliases (e.g. `date_optional_time`).
//
// Elasticsearch range queries accept an untrusted `"format"` field that flows
// through `StrptimeParser::from_java_datetime_format`, making this an external
// input surface beyond just configuration.
#![no_main]
use libfuzzer_sys::fuzz_target;
use quickwit_datetime::StrptimeParser;

fuzz_target!(|data: &[u8]| {
    if let Ok(format_str) = std::str::from_utf8(data) {
        let _ = StrptimeParser::from_java_datetime_format(format_str);
        let _ = StrptimeParser::from_strptime(format_str);
    }
});
