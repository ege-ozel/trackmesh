# Network changes

Read root `AGENTS.md`. The actor is the only swarm owner. Application callers use `Network`, not libp2p internals.

- `src/behaviour.rs`: transports/behaviors, protocol strings, limits, listening, advertised addresses.
- `src/actor.rs`: serialized commands/events, authorization, preferred connections, shutdown/error status.
- `src/discovery.rs`: allow-list refresh, direct retry ordering, learned-address bounds.
- `src/handle.rs`: bounded command/RPC interface; `src/codec.rs`: bounded JSON and raw chunk frames.
- `src/identity.rs`: signed application key ↔ PeerId; `src/status.rs`: online/route/transport reporting; `src/config.rs`: startup settings.

Preserve QUIC preference, TCP + Noise/Yamux fallback, and six-second retry/refresh behavior. QUIC replaces direct TCP. There is no relay client, DCUtR or rendezvous; that code is kept unbuilt in `future/relay-client`. Keep existing timeout/stream/window/connection limits unless explicitly tuning them.

Paired peers may connect; `src/gate.rs` adds a bounded, explicit 10-minute inbound enrollment window and `codec::PairingCodec` an isolated pairing channel. Unknown enrollment peers must not populate discovery, and ordinary transfer requests remain denied until core/user approval; project request authorization is still required. Validate signed cards and key-derived PeerId. No public DHT, mDNS or swarms.

The actor must not hash/scan large files or perform blocking disk work. Responders in `core/src/peer.rs` delegate those operations to blocking workers. Fatal actor exits resolve pending requests and report offline/error status.

Keep codec tags, size bounds, protocol strings, and signed types stable when moving code. The Phase 3 relay service is kept, unbuilt, in `future/relay-service`; real separate-network NAT validation remains pending. No automated tests/benchmarks.
