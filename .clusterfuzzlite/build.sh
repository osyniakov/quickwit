#!/bin/bash -eu
# Copyright 2021-Present Datadog, Inc.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

cd "$SRC/quickwit/quickwit"

# `rust-toolchain.toml` pins the stable toolchain used to ship quickwit, but
# cargo-fuzz needs the `-Z sanitizer` flags that only nightly accepts.
export RUSTUP_TOOLCHAIN=nightly

# quickwit-common calls tokio's unstable runtime APIs. `.cargo/config.toml` sets this
# cfg for normal builds, but cargo-fuzz composes its own RUSTFLAGS and that config
# value is dropped, so it has to be re-added here.
export RUSTFLAGS="${RUSTFLAGS:-} --cfg tokio_unstable"

cargo fuzz build

FUZZ_TARGET_DIR="fuzz/target/x86_64-unknown-linux-gnu/release"

for fuzz_target in $(cargo fuzz list); do
    cp "$FUZZ_TARGET_DIR/$fuzz_target" "$OUT/"
    if [ -d "fuzz/seeds/$fuzz_target" ]; then
        zip -j "$OUT/${fuzz_target}_seed_corpus.zip" "fuzz/seeds/$fuzz_target"/*
    fi
done
