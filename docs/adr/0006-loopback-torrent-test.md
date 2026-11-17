# ADR 0006 — Offline loopback torrent test

**Status:** accepted (Phase 1)

The brief says a local test swarm needs an ADR first. Reason: the agent's build sandbox can't reach real BitTorrent peers (outbound TCP is proxied), so torrent code would otherwise ship untested.

Scope is one integration test (`crates/features/torrent/tests/loopback.rs`), not tooling: a librqbit seeder on `127.0.0.1` serves a generated file; our engine (DHT and LSD off, `extra_peers` = the seeder) resolves metadata, then head/tail/middle reads are compared byte-for-byte, and cancellation is checked. Runs in ~2 s, no network.

Real-swarm TTFF/seek numbers still have to be measured on a normal network (see PROGRESS.md).
