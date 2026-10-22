# TrackMesh repository instructions for Claude

Read [AGENTS.md](AGENTS.md) first. It is the canonical repository policy; follow its task routing, crate-local instructions, compatibility/security rules and branch/check/handoff workflow. Use [docs/CODEMAP.md](docs/CODEMAP.md) for narrow file selection and [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for build/static checks. Do not add or run automated tests or benchmarks.

For two-PC setup/documentation work, start with [docs/setup/README.md](docs/setup/README.md). Sender instructions live in [host/README.md](docs/setup/host/README.md); receiver instructions live in [peer/README.md](docs/setup/peer/README.md). These roles describe the first transfer direction, not a central server architecture.

Current Desktop setup is Link folder → Invite collaborator → open or paste a one-read, 15-minute v2 link → automatic signed device request → explicit Approve → automatic first version into an empty, never-received folder → Share current version / Get collaborator's version. The optional per-user Supabase service stores small encrypted invite drops, endpoint records and head hints. Approval automatically queues a membership grant; creator-side Revoke queues a membership revoke without rotating the project key. Removed devices retain files and keys already received.

Invitation files and CLI `join`, safe `status` and `request-pairing` are Advanced paths; the CLI has no link flow. `serve` alone cannot approve enrollment. Manual card return is an explicit legacy option. Document discovery, selected-peer availability and IP-change handling as implemented. A public directory, browser deployment, offline pairing queue, permanent background service, automatic merge, relay fallback and hole punching do not exist. Mailbox and relay client/server code are kept unbuilt in `future/` for post-1.0. Opening the app does not notify an uninvited peer. Established connections are bidirectional; independent reverse reachability is a separate requirement.

Foundation phases 1–8 name the original implementation; Aşama 1 M0–M10 names the direct-P2P plan; #NN names review/UX tasks. Current evidence and unverified integrations belong in `docs/IMPLEMENTATION.md`.

The standalone peer-package agent template is [docs/setup/peer/CLAUDE.md](docs/setup/peer/CLAUDE.md). Copy that template to the package root as `CLAUDE.md`; do not distribute this repository-development entry point in its place. The package template must work without a repo checkout. Keep invitation payloads, keys, secrets, databases and user projects out of agent context, logs and distribution packages; native import accepts only the private invitation file path.

Keep setup instructions in their role folders and shared troubleshooting in the setup entry point. Update links when moving documents. Record actual checks and remaining integration work only in [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md); instructions are not validation evidence.
