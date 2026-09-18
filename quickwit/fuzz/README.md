# Fuzzing Quickwit

Coverage-guided fuzzing for the inputs an untrusted client can actually send to a
Quickwit node.

## Scope: public APIs only

Every target here drives an entry point that is reachable from outside the process
— a REST body, a query parameter, or an OTLP payload — through the same function
the server calls. Internal crates are deliberately *not* fuzzed directly.

The distinction matters because a fuzzer pointed at an internal decoder mostly
finds "bugs" that require an attacker to already control data the engine wrote
itself. Those reports cost review time and fix nothing. A panic in the list below,
by contrast, is a crash a stranger can trigger against a running node.

So: when the value being parsed comes from a user, fuzz it *through* the endpoint
that accepts it, not through the helper that happens to parse it. Datetime parsing
is covered by `ingest_document`, whose doc mapping declares a datetime field, and
not by a target that calls `quickwit-datetime` directly.

| Target | Entry point | Reachable from |
| --- | --- | --- |
| `search_query_string` | `UserInputQuery::parse_user_query` | `GET /api/v1/{index}/search?query=`, `_search?q=` |
| `elastic_query_dsl` | `QueryAst::try_from(ElasticQueryDsl)` | `POST /_elastic/{index}/_search` body |
| `ingest_document` | `DocMapper::doc_from_json_str` | `POST /api/v1/{index}/ingest`, `_bulk` |
| `index_config` | `load_index_config_from_user_config` | `POST /api/v1/indexes` |
| `otlp_logs_json` | `parse_otlp_logs_json` | `POST /otlp/v1/logs` |
| `otlp_logs_protobuf` | `parse_otlp_logs_protobuf` | OTLP gRPC, port 4317 |
| `otlp_traces_json` | `parse_otlp_spans_json` | `POST /otlp/v1/traces` |
| `otlp_traces_protobuf` | `parse_otlp_spans_protobuf` | OTLP gRPC, port 4317 |

Adding a target? Name the endpoint it is reachable from. If you cannot, it probably
belongs in a unit or property test instead.

## Running

Requires a nightly toolchain and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz):

```bash
rustup toolchain install nightly
cargo +nightly install cargo-fuzz --locked
```

`RUSTFLAGS` must carry `--cfg tokio_unstable`. Quickwit sets it in
`quickwit/.cargo/config.toml`, but cargo-fuzz builds its own `RUSTFLAGS` and that
config value is dropped, so pass it explicitly. `make fuzz-build` and
`make fuzz-run` wrap this.

```bash
cd quickwit

# Build every target.
RUSTFLAGS="--cfg tokio_unstable" cargo +nightly fuzz build

# Run one, seeded from the committed corpus.
RUSTFLAGS="--cfg tokio_unstable" cargo +nightly fuzz run search_query_string \
    fuzz/corpus/search_query_string fuzz/seeds/search_query_string

# Time-box a run (seconds) — what CI does on a pull request.
RUSTFLAGS="--cfg tokio_unstable" cargo +nightly fuzz run ingest_document -- -max_total_time=300
```

The fuzz crate is its own cargo workspace, so it does not slow down a normal
`cargo build` and is not swept up by `cargo clippy --workspace`. Its `Cargo.lock` is
checked in: a crash found today has to still reproduce tomorrow, and resolving the
dependency graph fresh on every CI run has already broken this build once.

## Corpora

`fuzz/seeds/<target>/` holds the committed starting inputs; `fuzz/corpus/<target>/`
is the working corpus cargo-fuzz grows and is gitignored. CI seeds each run from
`fuzz/seeds/` and carries the accumulated corpus between runs.

The two protobuf corpora are generated, because a valid OTLP protobuf payload is
not something to hand-write into a file:

```bash
python3 fuzz/scripts/generate_protobuf_seeds.py
```

Rerun it if the OTLP protos are ever renumbered.

## Findings

The first runs found three crashes across two distinct bugs. Both are fixed. The
reproducers are described here rather than added to `seeds/`, because a seed that
crashes on load makes a target useless.

**1. Underflow on an inverted span duration** — `otlp_traces_json` and
`otlp_traces_protobuf`, first hit after 6k executions. Fixed in
`quickwit-opentelemetry/src/otlp/traces.rs`, covered by
`test_span_from_otlp_clamps_inverted_duration`.

`span.end_time_unix_nano - span.start_time_unix_nano` on two `u64`s underflowed when
a span ended before it started: a panic under debug assertions, and in a release
build without overflow checks a wrap to a ~584-year duration that quietly corrupts
any duration aggregation over the index. A sender whose clock stepped backwards
mid-span produces one, so it never required a hostile client.

The duration is now clamped to zero and the anomaly logged, rather than rejected:
`parse_otlp_spans` propagates the first error, so failing the span would have
discarded every other span in the same export batch. The received timestamps are
still indexed as sent, so the skew stays visible.

**2. Panic on a query string of `*` followed by a control character** —
`search_query_string`, first hit after 170k executions. Minimal reproducer is two
bytes, `0x2A 0x0C` (`*` then a form feed), i.e.
`GET /api/v1/{index}/search?query=*%0C`. Fixed upstream; picked up here by moving the
tantivy pin to `20d7f72f`.

`*` parses to `UserInputLeaf::All`, which `set_default_field` rewrites into an
`Exists` leaf, and a later `set_field(None)` hit an `expect` in
`query-grammar/src/user_input_ast.rs`. Quickwit could not guard it from the outside,
since the panic happened inside `parse_query`; upstream now folds that case back to
`UserInputLeaf::All`.

### Where the targets stand

After both fixes, 120 seconds each unless noted:

| Target | Executions | Result |
| --- | --- | --- |
| `otlp_traces_json` | 8.9M (240s) | clean |
| `otlp_traces_protobuf` | 8.9M (240s) | clean |
| `elastic_query_dsl` | 6.1M | clean |
| `otlp_logs_json` | 5.3M | clean |
| `otlp_logs_protobuf` | 5.0M | clean |
| `ingest_document` | 3.5M | clean |
| `search_query_string` | 1.3M | clean |
| `index_config` | 506k | clean |

`index_config` is slowest because every input is parsed three times and a clean parse
then builds a doc mapper. `otlp_traces_protobuf` went from 43k executions to 8.9M
once the underflow was fixed: a crashing target stops exploring, so its pre-fix
numbers said little about the code behind the crash.

Minutes are still a smoke run, not a campaign. "Clean" means no shallow crash, not no
bug; the batch workflow is what actually explores these.

## Reproducing a crash

cargo-fuzz writes the offending input to `fuzz/artifacts/<target>/`. CI attaches the
same file to the failed run.

```bash
# Replay it.
RUSTFLAGS="--cfg tokio_unstable" cargo +nightly fuzz run search_query_string \
    fuzz/artifacts/search_query_string/crash-<hash>

# Shrink it to the smallest input that still crashes.
RUSTFLAGS="--cfg tokio_unstable" cargo +nightly fuzz tmin search_query_string \
    fuzz/artifacts/search_query_string/crash-<hash>
```

Once minimized, turn it into a regression test in the crate that owns the bug and
fix it there. Do not silence a crash inside the fuzz target: a target that swallows
panics reports nothing, which is worse than not having the target at all.
