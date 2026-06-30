#!/bin/bash -eu
cd "$SRC/quickwit/quickwit"
RUSTFLAGS="--cfg tokio_unstable" cargo +nightly fuzz build --fuzz-dir quickwit-fuzz
for fuzz_target in fuzz_query_dsl fuzz_query_string fuzz_datetime fuzz_doc_mapper fuzz_doc_mapper_config fuzz_java_datetime_format fuzz_otlp_logs fuzz_otlp_spans; do
    cp "target/x86_64-unknown-linux-gnu/release/$fuzz_target" "$OUT/"
done
