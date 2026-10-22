# Storage changes

Read root `AGENTS.md`. One `Store` owns the SQLite mutex/connection; domain files add methods to it. No generic repository framework is needed.

| File | Ownership |
| --- | --- |
| `src/lib.rs`, `src/schema.sql` | Open, pragmas, version/migration, connection ownership |
| `src/models.rs` | Public records and row decoding |
| `src/transfer_state.rs` | Six typed session labels and SQLite/JSON conversion |
| `src/projects.rs`, `src/trust.rs` | Folder links, device bindings, project authorization |
| `src/snapshots.rs` | Immutable commit, file/chunk index/catalog, lineage, verification timestamp |
| `src/transfers.rs` | Durable session/progress/apply markers |

- Preserve schema v2 for structural refactors. Do not change supported state labels or silently reinterpret stored values.
- `commit_local`: one transaction for snapshot/files/index/catalog/head; expected-head check and immutable-hash collision checks remain inside it.
- `begin_apply`: recovery flag and `APPLYING` commit together. `finish_apply`: recovery clear, complete state, and progress commit together.
- Chunk authorization joins project + snapshot + LOCAL location + hash; possession of a cached hash is insufficient authorization.
- Keep identity bindings immutable and the project collaborator limit enforced. Revocation removes that project's association; discovery/requests must respect it.
- `PENDING`/`PUBLISHED` are legacy metadata values; new authored snapshots stay `PENDING`. Received snapshots carry `REMOTE`.
- The `projects.mailbox_url`/`relay_address` columns and `mailbox_state` table stay in the schema for old databases and are not read.
- Do not split transaction steps into callers or introduce a second connection owner during code movement.

Database reads validate typed transfer labels and return errors for unknown values. No automated tests/benchmarks or destructive production-data cleanup.
