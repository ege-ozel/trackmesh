# Phase 3: encrypted mailbox and private infrastructure

> Historical. The mailbox and relay services moved to `future/` and are no longer built, so the commands below no longer work as written. The per-user Supabase service replaced the mailbox for version notices.

The agent publishes encrypted signed announcements to an Axum/SQLite mailbox. A separate libp2p process supplies allow-listed rendezvous and circuit relay. Neither service receives a project key. Project manifests and chunks remain on authorized collaborator PCs and travel through the existing peer transfer protocol.

Current default pairing is the native invitation → automatic device request → explicit sender approval workflow in [setup/README.md](setup/README.md). Direct operation does not require this infrastructure. The Desktop UI is a local Windows application; deploying it as a browser site is not part of this service setup. Supabase is not integrated. Local client SQLite and the mailbox's separate SQLite serve different purposes; neither makes a private/CGNAT address reachable by itself.

With prepared infrastructure, an online peer reconnects to the pinned stable service, renews rendezvous registration and establishes a relay reservation; trusted peers can discover it and try direct/circuit paths. Public deployment and actual IP/hotspot changes remain unverified. In the default direct trial, changed sender public IP/port is handled by a fresh native invitation imported into the same project/profile/folder. Mailbox stores encrypted snapshot announcements, not offline pairing requests or project files; source peers must still be online for Receive.

## Build

Use the Windows developer shell described in [DEVELOPMENT.md](DEVELOPMENT.md):

```powershell
cargo build -p trackmesh-core -p trackmesh-mailbox-service -p trackmesh-relay-service --locked
```

The three executables are `target\debug\trackmesh-agent.exe`, `trackmesh-mailbox-server.exe`, and `trackmesh-relay-server.exe`. Relay identity uses current-user DPAPI: run it under the same Windows account on restart and retain its data directory. The infrastructure is explicitly started; no Windows service or permanent agent daemon is installed.

## Authorize the devices

First exchange invitations privately and pair devices with current [automatic setup](setup/README.md), or explicitly use [legacy CLI file pairing](PHASE2.md). Invitations contain a project key; infrastructure operators receive **public signed device cards**, never invitations. Desktop Approve device changes only that PC's project trust; it does not update service ACLs. A new receiver must already have an operator-provisioned infrastructure grant if its first pairing attempt depends on that relay. Automatic approval is not a replacement for the provisioning steps below.

Export each device card with the agent's `card --output` command. On the infrastructure machine, create a mailbox ACL JSON array with one grant per project/device. For example, using cards already delivered privately:

```powershell
$projectId = 'REPLACE-WITH-PROJECT-UUID'
$cardA = Get-Content -Raw .\a-card.json | ConvertFrom-Json
$cardB = Get-Content -Raw .\b-card.json | ConvertFrom-Json
@(
    @{ project_id = $projectId; device = $cardA }
    @{ project_id = $projectId; device = $cardB }
) | ConvertTo-Json -Depth 32 | Set-Content -Encoding utf8NoBOM .\mailbox-acl.json
@($cardA, $cardB) | ConvertTo-Json -Depth 32 | Set-Content -Encoding utf8NoBOM .\relay-allow.json
```

These file-writing examples use PowerShell 7's `utf8NoBOM`; in Windows PowerShell 5.1, write UTF-8 without a BOM using `System.IO.File.WriteAllText` and `System.Text.UTF8Encoding($false)`. The server expects ordinary JSON bytes.

Mailbox ACL grants are checked against the signed request's project, device UUID, and public key. Device cards must bind the same key to their PeerId. Conflicting bindings and more than three devices in a project are rejected. The relay accepts at most three cards: use a dedicated relay identity/process per private collaboration group. Rendezvous registrations are visible to devices authorized on that instance; they are not an authorization boundary for project files. Clients still filter results through their own project trust.

The mailbox rereads its bounded ACL for each authenticated request. Relay reloads its allow-file every six seconds using a blocking worker outside the swarm. Missing/invalid documents disable access. Remove a revoked device from the appropriate service ACL as well as running the agent's `revoke` command. Replace ACL files atomically to avoid an intermediate invalid file.

## Run the services

```powershell
target\debug\trackmesh-mailbox-server.exe --database D:\TrackMeshInfrastructure\mailbox.db --acl D:\TrackMeshInfrastructure\mailbox-acl.json
target\debug\trackmesh-relay-server.exe --data-dir D:\TrackMeshInfrastructure\relay --allow-file D:\TrackMeshInfrastructure\relay-allow.json --port 4001 --external-address /dns4/relay.example.com/udp/4001/quic-v1 --external-address /dns4/relay.example.com/tcp/4001
```

The mailbox backend binds literal loopback only (default `127.0.0.1:8787`). Expose it through an HTTPS reverse proxy with a valid certificate. [The sample Caddyfile](../future/mailbox-service/Caddyfile.example) uses documented [reverse proxy](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy) and [body-size](https://caddyserver.com/docs/caddyfile/directives/request_body) directives. Replace its hostname with a domain you control, validate the configuration, and configure DNS/firewall/certificate issuance before deployment. Caddy deployment and public certificate issuance are not performed by the repository build.

Allow inbound UDP and TCP on the relay's configured port. Pin the **printed relay PeerId**, obtained through a trusted operator channel, in the agent's complete relay address, for example `/dns4/relay.example.com/udp/4001/quic-v1/p2p/REPLACE-WITH-RELAY-PEER-ID`. Advertise addresses that clients can actually reach. The relay PeerId persists across restarts because its protected identity is retained.

Update an existing project by repeating `link` with its **same UUID and folder**, plus `--relay-address` set to the pinned address. Retain both options when relinking; omitted endpoint options clear their stored values. Export a fresh invitation for a new device, or relink existing collaborators individually. Do not unlink or recreate existing data directories.

## Publish and refresh

```powershell
target\debug\trackmesh-agent.exe snapshot PROJECT-UUID --output .\snapshot.json
target\debug\trackmesh-agent.exe publish PROJECT-UUID
target\debug\trackmesh-agent.exe refresh PROJECT-UUID
target\debug\trackmesh-agent.exe status
target\debug\trackmesh-agent.exe serve
```

`publish` processes up to 64 pending snapshots authored by this device. It validates the signed manifest, creates and persists one encrypted envelope, POSTs it, and marks `PUBLISHED` only after a matching ACK. A received copy is not republished as this device's work. If the request or ACK fails, the envelope remains durable and `PENDING`. A retry sends that same envelope with fresh request authentication. Duplicate identical envelopes receive the original sequence/ACK; a different envelope for the same snapshot UUID produces HTTP 409.

`refresh` fetches ordered pages of at most 32 announcements, up to eight pages per call. It checks project/sequence bounds, trusted author, Ed25519 signature, authenticated decryption, metadata limits, and immutable-history consistency before committing the page and its cursor together. A rejected page leaves that page's cursor unchanged; previously completed pages remain committed. An unpaired/revoked author blocks ingestion rather than silently trusting it. `more: true` means another refresh may be needed. Concurrent refreshes use an expected-cursor check.

`serve` retries publication and refresh every 30 seconds while its explicit session is alive. Failures are logged without sensitive payloads; pending/cursor state stays durable. Ctrl+C stops that worker and the peer runtime. Save settling and DAW/session-aware lifetime belong to later phases.

An announcement appears in `status` history as `REMOTE`; it does not advance the local head, create local chunks, or prove its author is online. Use `check` and `receive` when an authorized source returns online. Fresh peer equality ACKs still own online verification. An untrusted mailbox can omit/delay announcements; cryptographic validation prevents it from inventing trusted content, but does not prove completeness or availability.

## Local development and limits

For separate scratch devices on one Windows PC, link to `http://127.0.0.1:8787` and explicitly pass `--allow-loopback-http` to `publish`, `refresh`, or `serve`. Without that flag, requests require HTTPS. The exception accepts literal loopback IPs only, not arbitrary HTTP hosts. Redirects, URL credentials, query strings, and fragments are rejected; TLS certificate/hostname validation remains enabled.

Signed HTTP requests bind project/device/key, timestamp, random nonce, GET cursor or POST scope, and a BLAKE3 body digest. Clock skew is limited to 120 seconds. The server persists nonce replay protection across restart, retaining nonces for 240 seconds, and limits each device to 120 authenticated requests per minute. Synchronize device/server clocks. HTTP 401 indicates invalid/replayed/expired authentication, 403 missing ACL grant, 409 an immutable collision, 429 rate limiting, and 503 unavailable/invalid ACL or busy workers.

Bounds: 4 KiB auth header, 128 KiB POST, 64 KiB encoded ciphertext, 32 records/5 MiB page, 64 concurrent HTTP requests, 16 blocking HTTP workers, 30-second backend request deadline, and 5-second connect/20-second overall client timeout. Each project has a 10,000-record service quota (HTTP 507 when full); no automatic record pruning occurs. ACL files are at most 1 MiB, with 1,024 mailbox grants. Protect and back up the service database/ACL using operator procedures; they are separate from agent data.

Relay retains standard reservation/circuit rate limiters, with bounded connection/registration/cookie counts. Each circuit permits at most 256 MiB and 120 seconds, enough for the bounded manifests/chunk requests rather than an unlimited tunnel. Project encryption keys determine private rendezvous namespace hashes, and the relay processes only authenticated allow-listed peers.

Actual validation evidence and unresolved deployment/NAT work are recorded only in [IMPLEMENTATION.md](IMPLEMENTATION.md). No automated tests or benchmarks are used.
