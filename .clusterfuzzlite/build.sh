#!/bin/bash -eu
cd "$SRC/quickwit/quickwit"
RUSTFLAGS="--cfg tokio_unstable" cargo +nightly fuzz build
for fuzz_target in fuzz_query_dsl fuzz_query_string fuzz_datetime fuzz_doc_mapper; do
    cp "fuzz/target/x86_64-unknown-linux-gnu/release/$fuzz_target" "$OUT/"
done
