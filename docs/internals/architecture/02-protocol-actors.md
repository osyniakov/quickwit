# Layer 1: Protocol & Actor Core

The gRPC contracts and the actor runtime that every service and pipeline
in Quickwit is built from.

## Crates

| Crate | Responsibility | Depends on (internal) | Tags |
|-------|-----------------|------------------------|------|
| `quickwit-actors` | Lightweight, custom actor framework: mailboxes, supervision, observability. Underlies the indexing pipeline (`Source -> DocProcessor -> Indexer -> ... -> Publisher`) and several services. | `quickwit-common`, `quickwit-metrics` | |
| `quickwit-proto` | Protobuf-generated gRPC service traits and message types for all inter-service communication. Auto-generated via `quickwit-codegen`. | `quickwit-actors`, `quickwit-codegen`, `quickwit-common` | |

## Depended on by

Nearly everything above this layer: `quickwit-config`, `quickwit-storage`,
`quickwit-doc-mapper`, `quickwit-cluster`, `quickwit-indexing`,
`quickwit-search`, `quickwit-serve`, `quickwit-cli`, and more.

## Guidance for agents

- Changing a message or service definition in `quickwit-proto` is a
  wire-protocol change: check for compatibility across a rolling upgrade
  (mixed old/new nodes in the same cluster) before changing or removing a
  field, not just at compile time.
- `quickwit-actors` failures (a panicking actor, a full mailbox) are the
  most common root cause of hangs in the indexing pipeline. When
  debugging a stuck pipeline, start here rather than in the specific
  actor's business logic.
- This layer must never depend on anything in Domain Primitives (layer 2)
  or above. `quickwit-proto` only knows about wire types, not about
  config parsing, doc mapping, or storage.
