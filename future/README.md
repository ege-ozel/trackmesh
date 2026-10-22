# Future work (not built)

Code kept as notes for after 1.0. It is outside the Cargo workspace, so it is not compiled, checked or shipped, and it may no longer build as it is.

- `mailbox-service/` — the self-hosted metadata service. It stored encrypted version notices for a project. The per-user Supabase service now does this job.
- `mailbox-client/` — the app side of that service (core, storage, crypto and HTTP pieces), removed in d447de1.
- `relay-service/` — a private relay/rendezvous server for when two PCs cannot connect directly. Relay fallback is not part of 1.0.
- `relay-client/` — the app side: libp2p relay client, DCUtR hole punching and rendezvous discovery, removed from `crates/network`.

Each `*-client/` folder has its own README saying where the pieces lived and what returning them needs.

To bring one back, move it into the workspace members in the root `Cargo.toml` and bring it up to date with the current crates.
