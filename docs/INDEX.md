# Documentation index

Choose the page for the current task; these pages are not a required reading sequence.

Terminology: **Foundation phases 1–8** are the 2026-09/10 build described in `PHASE*.md`. **Aşama 1 M0–M10** is the direct-P2P plan. **#NN** identifies review/UX tasks. These numbering systems describe different work; validation evidence belongs in [IMPLEMENTATION.md](IMPLEMENTATION.md).

| Page | Purpose |
| --- | --- |
| [Root README](../README.md) | Product status and quick build entry point |
| [Two-PC setup entry point (Türkçe)](setup/README.md) | Current invite-link flow, optional Supabase, shared connection troubleshooting and manual router/firewall guidance |
| [Sender / host setup (Türkçe)](setup/host/README.md) | First sender: current-source build, folder link, Invite collaborator, Approve and Share current version |
| [Receiving peer setup (Türkçe)](setup/peer/README.md) | First receiver: open/paste link, Accept, automatic first version and later Get collaborator's version; Advanced CLI route |
| [Automatic pairing security (Türkçe)](setup/PAIRING_SECURITY.md) | Enrollment challenge, explicit approval, transport/authorization separation, bounds and sniffing limits |
| [Peer package CLAUDE.md (Türkçe)](setup/peer/CLAUDE.md) | Standalone distribution-package agent instructions; invitation payload stays outside agent context |
| [Agent entry point](../AGENTS.md) | Repository rules and task routing |
| [Code map](CODEMAP.md) | Exact file ownership, public entry points, and targeted searches |
| [Architecture](ARCHITECTURE.md) | Dependency boundaries, durable flows, and security/recovery invariants |
| [Development](DEVELOPMENT.md) | Build/static checks, Windows prerequisites, local data, and change discipline |
| [MVP contract](MVP.md) | Accepted scope and requirements for unfinished phases |
| [Aşama 1 M0–M10 direct P2P plan](plans/phase1-direct-p2p.md) | Milestone decisions for UPnP, endpoint updates, one-read links and direct acceptance; implementation evidence is separate |
| [Aşama 1 M5/M6 endpoint and invitation design](plans/phase1-m5-endpoint-design.md) | Signed endpoint and invitation design; consult IMPLEMENTATION.md for delivered scope and validation |
| [Supabase backend](../services/supabase/README.md) | Encrypted invite/endpoint/head service setup, signed membership and deployment commands |
| [Deferred until after 1.0.0](plans/post-1.0.md) | Plan only: relay, content-defined chunking, mailbox close lifetime |
| [Implementation progress](IMPLEMENTATION.md) | Current phase status, historical manual evidence, and integration gaps |
| [Foundation phase 2 operation guide](PHASE2.md) | Advanced CLI request-pairing, legacy card recipe, serve/check/receive/resume/recovery |
| [Foundation phase 3 infrastructure guide](PHASE3.md) | Separate self-hosted publication/refresh and operator ACLs; legacy relay/rendezvous code |
| [Foundation phase 4 desktop guide](PHASE4.md) | Original Desktop implementation and native operation; current setup is in the role guides |
| [Foundation phase 5 VST3/IPC guide](PHASE5.md) | Official SDK build, UUID state, native controls, private pipe and owner lifetime |

Crate-local `AGENTS.md` files add focused guidance for core, network, storage, and transfer changes. Source and schema define exact current behavior; progress claims belong in `IMPLEMENTATION.md`.
