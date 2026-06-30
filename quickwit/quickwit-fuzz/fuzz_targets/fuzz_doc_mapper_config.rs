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

// Fuzz the doc mapper *configuration* (field mappings, tokenizer definitions,
// index settings) as supplied via the index-creation REST API.
//
// The existing `fuzz_doc_mapper` target exercises document *ingestion* against
// a fixed empty schema.  This target covers the orthogonal path: parsing and
// validating the schema definition itself, including regex tokenizer patterns
// that could cause catastrophic backtracking.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(json_str) = std::str::from_utf8(data)
        && let Ok(builder) = serde_json::from_str::<quickwit_doc_mapper::DocMapperBuilder>(json_str)
    {
        let _ = builder.try_build();
    }
});
