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

## Known findings

The first run of these targets found three crashes. They are recorded here rather
than added to `seeds/`, because a seed that crashes on load makes a target useless.
Until they are fixed, CI will flag them.

**1. Underflow on an inverted span duration** — `otlp_traces_json`,
`otlp_traces_protobuf`, found after 6k executions.

`quickwit-opentelemetry/src/otlp/traces.rs:270` computes

```rust
let span_duration_nanos = span.end_time_unix_nano - span.start_time_unix_nano;
```

Both sides are `u64`, so a span whose end precedes its start underflows. With debug
assertions on, the OTLP ingest task panics; in a release build without overflow
checks it wraps instead, and the span is indexed with a duration of roughly 584
years, which quietly corrupts any duration aggregation over that index. An OTLP
client only needs a clock that stepped backwards mid-span to produce one, so this
does not require a hostile sender.

**2. Panic on a query string of `*` followed by a control character** —
`search_query_string`, found after 170k executions. Minimal reproducer is two bytes:
`0x2A 0x0C` (`*` then a form feed), i.e. `GET /api/v1/{index}/search?query=*%0C`.

The panic is upstream, at `query-grammar/src/user_input_ast.rs:51` in the pinned
tantivy revision:

```rust
UserInputLeaf::Exists { field: _ } => UserInputLeaf::Exists {
    field: field.expect("Exist query without a field isn't allowed"),
},
```

`*` parses to `UserInputLeaf::All`, which `set_default_field` rewrites into an
`Exists` leaf, and a later `set_field(None)` then hits the `expect`. Quickwit cannot
guard this from the outside, since the panic happens inside `parse_query`; the fix
belongs in tantivy.

Targets that came back clean, at 45 seconds each: `otlp_logs_protobuf` (3.1M
executions), `elastic_query_dsl` (3.0M), `otlp_logs_json` (2.8M), `ingest_document`
(1.4M) and `index_config` (193k — the slowest target, since every input is parsed
three times and a clean parse then builds a doc mapper).

Forty-five seconds is a smoke run, not a campaign. "Clean" above means no shallow
crash, not no bug; the batch workflow is what actually explores these.

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
