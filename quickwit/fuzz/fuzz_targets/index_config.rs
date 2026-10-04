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

//! Fuzzes index configuration parsing and validation.
//!
//! Public surface: the body of `POST /api/v1/indexes`, which a user submits as JSON,
//! TOML or YAML. `load_index_config_from_user_config` is the exact call the REST
//! handler makes, so this covers doc mapping construction, merge policy and retention
//! parsing, and the validation that is supposed to reject hostile configs cleanly.

#![no_main]

use std::str::FromStr;
use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use quickwit_common::uri::Uri;
use quickwit_config::{ConfigFormat, load_index_config_from_user_config};

static DEFAULT_INDEX_ROOT_URI: LazyLock<Uri> = LazyLock::new(|| {
    Uri::from_str("ram:///indexes").expect("`ram:///indexes` should be a valid uri")
});

fuzz_target!(|data: &[u8]| {
    // The endpoint picks the format from the request's content type, so every
    // encoding a client may declare is in scope.
    for config_format in [ConfigFormat::Json, ConfigFormat::Toml, ConfigFormat::Yaml] {
        let _ = load_index_config_from_user_config(config_format, data, &DEFAULT_INDEX_ROOT_URI);
    }
});
