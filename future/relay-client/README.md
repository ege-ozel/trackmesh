# Relay client (not built)

The app side of the private relay in `../relay-service/`. When two PCs could not connect directly, each one reserved a slot on a pinned relay and dialed the other through a `/p2p-circuit` address. Rendezvous on the same relay shared listen addresses inside a project-key-derived namespace, and DCUtR tried to turn a relayed connection into a direct one by hole punching. It only ran for projects linked with a relay address (`--relay-address` or an invitation carrying `relay_address`). Relay fallback is deferred to post-1.0, so this was removed from `crates/network` and its callers.

Nothing here is compiled.

## Files

| File | What it is |
| --- | --- |
| `network_behaviour.rs`, `network_discovery.rs`, `network_actor.rs`, `network_status.rs` | Full copies of `crates/network/src/` before removal |
| `removal.diff` | The code diff that removed it (network, core, CLI, storage, Desktop, `Cargo.toml`). `git apply -R` on a matching tree reverses it |

What the removed code did:

- `behaviour.rs`: `relay::client`, `dcutr` and `rendezvous::client` behaviours; `.with_relay_client(...)` transport.
- `discovery.rs`: `infrastructure_id` (pinned relay PeerId), allow-listing the relay, relay reservation (`listen_on .../p2p-circuit`), rendezvous register/discover, circuit dial after two failed direct rounds, the relay's observed address as an external address.
- `actor.rs`/`status.rs`: a `relayed` flag per connection, route `RELAYED`, direct connections closing relayed ones, endpoint updates refused over a relay.
- core: `Engine::namespaces()` (relay address + `trackmesh-private-rendezvous-v1` namespace per project), `link(..., relay)`, CLI `link --relay-address`, Desktop `Action::Link { relay_address }`.
- `Cargo.toml`: libp2p features `relay`, `dcutr`, `rendezvous`.

## Still in the product, for compatibility

- `ProjectInvite.relay_address` in the signed invitation: written empty, ignored when read.
- SQLite `projects.relay_address` column: kept, not read or written.
- Plugin status `connection` field: same field, now only `DIRECT` or `OFFLINE`.

## To bring it back

1. Re-enable the three libp2p features and add the behaviours and relay transport back in `behaviour.rs`.
2. Restore `NetworkConfig.rendezvous`, `Network::reload(namespaces)` and the discovery code; keep the relay PeerId pinned and the allow-list fail-closed.
3. Restore the `relayed` flag and the `RELAYED` route; keep endpoint updates direct-only.
4. Restore `Engine::namespaces()`, the `relay_address` read in storage, and the link inputs (CLI, Desktop, invitation import/export).
5. Bring `../relay-service/` back into the workspace and test on two separate networks with real NAT.
