# Layer 3: Storage & Metadata

Durable state: split bytes, index metadata, and cluster membership. This
is the layer where Quickwit starts talking to the outside world (cloud
object storage, PostgreSQL, other nodes).

## Crates

| Crate | Responsibility | Depends on (internal) | Talks to |
|-------|-----------------|------------------------|----------|
| `quickwit-storage` | Multi-cloud storage abstraction (S3, Azure, GCS, local file, RAM) behind a single `Storage` trait. | `quickwit-aws`, `quickwit-common`, `quickwit-metrics`, `quickwit-config`, `quickwit-proto` | Cloud object storage |
| `quickwit-metastore` | Index metadata storage: file-backed for dev, PostgreSQL for production. | `quickwit-storage`, `quickwit-config`, `quickwit-doc-mapper`, `quickwit-query`, `quickwit-parquet-engine`, `quickwit-common`, `quickwit-metrics`, `quickwit-proto` | PostgreSQL (production) |
| `quickwit-cluster` | Cluster membership and failure detection via the [Chitchat](https://github.com/quickwit-oss/chitchat) gossip protocol. | `quickwit-config`, `quickwit-transport`, `quickwit-common`, `quickwit-metrics`, `quickwit-proto` | Other Quickwit nodes |

## Depended on by

Data-Plane Engines (layer 4), Platform Services (layer 5), and the API
surface — everything that reads or writes splits or index metadata.

## Guidance for agents

- `quickwit-storage`'s `Storage` trait is the seam for adding a new cloud
  backend; new backends are feature-gated (`azure`, `gcs`) per
  `AGENTS.md`. Keep new feature flags minimal and additive.
- `quickwit-metastore` has two backends with different consistency and
  latency characteristics (file-backed vs. PostgreSQL via `sqlx`). A
  change to a metastore operation's semantics must be implemented and
  tested against both, not just whichever one you have running locally
  — `make test-all` starts the PostgreSQL container for this reason.
- `quickwit-cluster` failures manifest as split-brain or missed
  scheduling events several layers up (in `quickwit-control-plane`); if
  you're chasing a scheduling bug, rule out gossip/membership issues here
  first.
