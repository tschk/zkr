# Roadmap

zkr stays a neutral Rust crate and CLI. Agent-specific behavior remains in `plugins/`, and model runtimes remain caller supplied.

## Shipped foundation

- Evidence-backed temporal claims, corrections, deletion propagation, and cited daily reviews.
- Canonical memory tiers (`short_term`, `long_term`, `archive`) and processing states (`pending`, `processed`, `blocked`) with a legal-state matrix.
- Ledger-backed commits, an idempotent projection-repair outbox, and `promote`/`archive`/`repair` lifecycle operations.
- Tenant-scoped SQLite FTS plus persisted real embedding projections.
- Exact dense retrieval and deterministic FTS+dense reciprocal-rank fusion.
- Hash- and revision-bound projections with bounded stale/missing rebuild inspection.
- Ordered schema migrations with populated fixtures from every supported historical version.
- Native OpenClaw and Hermes adapters over the same CLI contract.

## Next, in order

1. **Scoped export and import:** round-trip one tenant/person with evidence, history, tombstones, and projection metadata without leaking another scope.
2. **Frozen retrieval benchmark:** check FTS-only, dense-only, and hybrid retrieval against corrections, deletion, multilingual paraphrases, abstention, and tenant isolation.
3. **Evidence-backed graph expansion:** add typed adjacency only if the frozen benchmark proves a repeatable gain over hybrid retrieval.

Approximate vector indexes, hosted services, schedulers, and bundled embedding models remain deferred until exact scan or caller-managed inference is measurably insufficient.
