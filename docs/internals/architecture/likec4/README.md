# LikeC4 model

This is a [LikeC4](https://likec4.dev) model of the `quickwit/` Cargo
workspace's layered architecture, described in
[`../README.md`](../README.md). It is generated from, and must stay in
sync with, the real `path` dependencies in each crate's `Cargo.toml` (see
[`../08-dependency-rules.md`](../08-dependency-rules.md)).

## Layout

```
likec4/
  specification.c4        element kinds and tags
  model/
    00-context.c4          person, external systems, the `quickwit` system shell
    01-foundation.c4        layer 0
    02-protocol.c4           layer 1
    03-domain.c4              layer 2
    04-storage.c4              layer 3
    05-dataplane.c4             layer 4
    06-platform.c4                layer 5
    07-entrypoints.c4               layer 6
    08-testing.c4                    cross-cutting test infrastructure
  views.c4                 the `index`, `layers`, and `<layer>Detail` views
```

Each `model/NN-*.c4` file `extend`s the `quickwit` system with one layer
and declares that layer's crates' outgoing dependency edges (to any
layer, including forward references to layers defined in later files —
LikeC4 resolves the whole workspace before checking references, so file
order doesn't matter).

## Viewing the diagrams

No local install is required; use `npx`:

```bash
# Live preview with hot reload, from the repo root
npx likec4@latest start docs/internals/architecture/likec4

# Validate syntax and semantics (fast, no browser needed) — CI-friendly
npx likec4@latest validate docs/internals/architecture/likec4

# Export every view to PNG (requires a Chromium binary Playwright can find)
npx likec4@latest export png docs/internals/architecture/likec4 -o /tmp/quickwit-arch
```

The [Chromium/Chrome headless-shell build LikeC4's exporter drives may
need `npx playwright install chromium`] the first time you export images
on a machine that doesn't already have it; `validate` never needs a
browser.

## Views

- `index` — system landscape: Quickwit as one box plus the external
  systems it talks to (object storage, PostgreSQL, message queues, OTel
  clients, AWS Lambda).
- `layers` — the 8 layers as boxes, with edges aggregated from every
  underlying crate-to-crate dependency.
- `foundationDetail`, `protocolDetail`, `domainDetail`, `storageDetail`,
  `dataplaneDetail`, `platformDetail`, `entrypointsDetail`,
  `testingDetail` — one layer's crates expanded, with the layers/external
  systems it depends on collapsed to single boxes.

## Keeping this in sync

When a `Cargo.toml` dependency changes:

1. Update the `model/NN-*.c4` file for the crate's layer (add/remove the
   relationship line).
2. If the crate moved to a different layer, move its `crate` declaration
   to the other layer's file and update both files' relationships.
3. Update the matching prose doc in `../` (the crate table and any
   "depends on (internal)" column).
4. Run `npx likec4@latest validate docs/internals/architecture/likec4`
   before committing.
