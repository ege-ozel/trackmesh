# Private relay and rendezvous

Read root `AGENTS.md`. One actor owns this infrastructure swarm. It serves relay/rendezvous protocols only, never project data requests.

- `src/main.rs`: explicit listeners, external addresses, DPAPI identity lifetime.
- `src/actor.rs`: bounded allow-list reload outside the swarm, fail-closed revocation, event ownership.
- `src/behaviour.rs`: QUIC and authenticated TCP, connection/relay/registration limits.

Public signed device cards authorize connections; clients still enforce project trust and pin the infrastructure PeerId. No DHT/mDNS/public discovery. Preserve client protocols and prefer direct connections. Do not claim real NAT/DCUtR validation from loopback checks. No tests/benchmarks.
