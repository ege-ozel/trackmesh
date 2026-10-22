# Phase 4 Windows desktop operation guide

The native Tauri 2 / React / TypeScript desktop calls the existing Rust core. It owns an explicit window session with one peer runtime and the existing 30-second metadata worker. Phase evidence and unfinished integrations belong in [IMPLEMENTATION.md](IMPLEMENTATION.md).

## Build and launch

Use Windows 10/11 x64, the x64 Visual Studio C++ developer shell, Windows SDK, WebView2, Rust 1.98.1 (the known working toolchain), and a supported Node.js runtime. The desktop package declares Rust 1.90; other crates' declared 1.85 minimum remains unverified. This phase used Node 24.19. Vite requires Node 20.19+ or 22.12+; do not use the machine's older Node 22.11 for the desktop build.

Build static assets before workspace checks/native builds. No development server or filesystem watcher is needed:

```powershell
Set-Location D:\CodeSpace\track-mesh\apps\desktop
npm ci
npm run typecheck
npm run build
Set-Location D:\CodeSpace\track-mesh
cargo build -p trackmesh-desktop --locked --offline
target\debug\trackmesh-desktop.exe
```

Use a supported Node/npm installation (see the [development guide](DEVELOPMENT.md)). Initial dependency download may require network access; keep lockfiles and use the development guide's prescribed static checks.

The output is `target\debug\trackmesh-desktop.exe` with bundled frontend assets. No installer, code-signing certificate, autostart, updater or permanent Windows service is installed. [Phase 5](PHASE5.md) additionally packages desktop/agent executables beside the VST3 binary and exposes its local plugin pipe.

Default data stays `%LOCALAPPDATA%\TrackMesh`; `--data-dir` or `TRACKMESH_DATA_DIR` selects another profile. The desktop default listener is port **43101**. It accepts `--device-name`, `--port`, repeated `--external-address` flags, and the same explicit literal-loopback `--allow-loopback-http` development option. An existing DPAPI identity retains its name/key. For example:

```powershell
target\debug\trackmesh-desktop.exe --device-name "Studio A" --port 43101 --external-address /ip4/192.168.1.10/udp/43101/quic-v1 --external-address /ip4/192.168.1.10/tcp/43101
```

Advertise only addresses collaborators can reach; production mailbox URLs use valid HTTPS and relay addresses include the pinned infrastructure PeerId. A second desktop launch focuses the existing window. A peer-runtime file lease prevents CLI `serve`/`check`/`receive` and desktop from running separate peers against the same data directory. Read-only CLI status and project operations retain their existing project/SQLite safeguards. Use a different device profile for a second peer in a one-PC manual fixture.

## Link and pair

1. Select **Link folder**, enter a project name and its permanent UUID, and choose the actual project folder. **New UUID** creates an initial identifier; retain it across collaborators and future VST state. Agent data and project folders must be separate.
2. Configure the optional HTTPS mailbox and pinned relay address when linking. Relinking uses the same UUID/folder; retain endpoint values because omitted options clear them, as with the CLI.
3. Expand **Collaboration & connection details → Pair collaborator**. For a LAN invitation use **Export invitation with current addresses**; for Internet setup open **Connection setup** and use **Export invitation with public addresses** after configuring forwarding/firewall. Follow [the two-PC trial steps](setup/README.md) for the complete sequence. Exchange this capability-bearing file privately. The desktop never places its payload/project key in React, the clipboard, or logs.
4. Export opens a 15-minute pairing window for that project. The collaborator selects **Accept invitation → Accept invitation & connect…** with their local folder. Native import is followed by an automatic enrollment request over the same encrypted peer transport; no return card file is required.
5. The inviter verifies the expected device/PeerId and chooses **Pair collaborator → Approve device** or **Reject**. Until approval, transfer authorization remains denied. The receiver polls approval for up to 60 seconds; if approval comes later, **Retry automatic pairing** obtains the result. At most two other devices may be paired per project.

If the inviter is offline or enrollment cannot complete, the imported project remains intact. The inviter uses **Open pairing window**, the receiver **Retry automatic pairing**. Closing/expiry/restart clears unapproved requests; windows do not revoke invitation capabilities or already trusted peers. Old signed cards are supported under **Legacy file pairing**. CLI adds `request-pairing` while retaining existing flags/output contracts. [Enrollment security](setup/PAIRING_SECURITY.md) describes the additive protocol and bounds. Both sides need an automatic-pairing build: the `20261007` package lacks it, `20261008` AutoPairing contains it, and `20261009` reuses those executables with refreshed instructions. Earlier ZIPs remain unchanged.

The receiver learns the project name/UUID, signed inviter identity and dial addresses from native invitation import. The inviter appears in the receiver's Collaborator list immediately; the inviter's application being open alone does not send a notification or publish a user directory. A known row does not prove online availability. Current direct setup uses the invitation addresses; changing the inviter's public IP/external port requires a fresh address-bearing invitation imported into the same project/profile/folder. Hotspot/CGNAT can require an operational private relay. Browser deployment, Supabase and offline enrollment storage are not implemented. [Shared setup](setup/README.md) covers these distinctions and bidirectional existing connections.

Document inputs are bounded at 64 KiB. Exports require a new `.json` filename and preserve existing files. Revocation changes local project authorization; the infrastructure operator must also update mailbox/relay ACLs as described in [PHASE3.md](PHASE3.md).

## Controls and status

- **Pair collaborator** shows invitation-verified pending candidates with device name, Device UUID and full Identity/PeerId, plus Approve/Reject. A pending row can remain after disconnection until window closure/expiry; it has no live availability marker. Unimported invitation recipients, every rejected network attempt and persistent connection/audit history are not displayed.
- **Collaborator** selects one locally paired device. Its badge shows `DIRECT`, `RELAYED` or `OFFLINE`; connected transport is shown beside the selector. This is the selected peer's current connection, not an all-users online list or proof of completed Receive. The inviter sees the approved receiver in its list; the receiver sees the imported inviter even while its own access awaits approval.

- **Send Snapshot** scans/hashes and commits locally, then attempts own-author metadata publication. A transport failure leaves the successful local commit visible with `PENDING`. **Retry publication** and the active session worker use the same metadata coordinator; `PUBLISHED` requires a matching durable ACK.
- **Refresh metadata** verifies encrypted signed announcements. It does not download project files or advance the local head. Each collaborator card retains at most eight recent authored snapshots and a bounded last-known lineage. An offline source is not a file-delivery promise; incomplete or deeper ancestry stays `UNKNOWN`.
- **Check peer** obtains signed ancestry and a fresh equality ACK. `UP TO DATE` is shown only for a recent verified check in the current session. Head/known-metadata changes, disconnection, or age invalidate that claim. Persisted verification is shown only for the selected peer and current local head. Known non-equal lineage remains visible while the compared heads/metadata still match.
- **Receive** needs an online selected collaborator and an explicit DAW-closed confirmation. Known divergence disables the control; core independently rejects divergence, unsuitable ancestry, and unsnapshotted folder edits. **Resume receive** targets the existing paused snapshot/session rather than silently selecting a different head.
- **Recover locally** appears when the durable recovery marker is set. Close the DAW first. It uses the existing trusted journal recovery and does not require the source peer to be online.
- Transfer progress shows durable verified bytes; snapshot preparation has a busy label rather than an invented percentage. Cards show the latest transfer's snapshot ID so a historical completion is not confused with the current local head.

Status pages contain at most 32 projects, one local snapshot, two trusted-peer summaries with eight authored snapshots each, and one latest transfer per project. Last-known lineage walks at most 64 ancestors and reports `UNKNOWN` when it cannot prove a relationship. Complete CLI history is unchanged. Foreground jobs are bounded and project operations reject overlapping clicks. Keys, signed serialization, SQLite schema v2, transfer labels and journal phases remain owned by their existing modules.

## Close safely

Since Phase 7, window close and **Close safely** hide the window and release its explicit window session. Active VST sessions, accepted desktop/plugin jobs, peer responses and authored pending publication keep the same owner running. Transfers continue while their source remains available; file installation/recovery retain their existing locks and ordering. Once all work finishes, 60 idle seconds precede clean shutdown and runtime-lease release. Starting desktop again restores the same window. Keep staging/recovery backups intact.

There is no per-transfer Cancel API. [Phase 5](PHASE5.md) describes VST/Named Pipe signals and the shared desktop Engine/network, including Phase 6 settled saves, Phase 7 lifetime and Phase 8 receive handoff. A missing/unavailable mailbox can keep pending work alive indefinitely; reopen desktop to inspect/configure it. Explicit interruption preserves committed pending metadata for restart; forced process termination during installation still requires the existing journal recovery. Native desktop validation on one PC does not establish real HTTPS, firewall/NAT, forced relay or DCUtR behavior.
