# Roadmap

zkr stays a neutral Rust crate and CLI. Agent-specific behavior remains in `plugins/`, and model runtimes remain caller supplied.

## Shipped foundation

- Evidence-backed temporal claims, corrections, deletion propagation, and cited daily reviews.
- Tenant-scoped SQLite FTS plus persisted real embedding projections.
- Exact dense retrieval and deterministic FTS+dense reciprocal-rank fusion.
- Native OpenClaw and Hermes adapters over the same CLI contract.

## Next, in order

1. **Embedding lifecycle integrity:** record source revision and creation time, identify stale projections, and make rebuilds auditable.
2. **Versioned schema migrations:** replace create-if-missing setup with ordered, rollback-safe upgrades and fixtures from every released schema.
3. **Scoped export and import:** round-trip one tenant/person with evidence, history, tombstones, and projection metadata without leaking another scope.
4. **Frozen retrieval benchmark:** check FTS-only, dense-only, and hybrid retrieval against corrections, deletion, multilingual paraphrases, abstention, and tenant isolation.
5. **Evidence-backed graph expansion:** add typed adjacency only if the frozen benchmark proves a repeatable gain over hybrid retrieval.

Approximate vector indexes, hosted services, schedulers, and bundled embedding models remain deferred until exact scan or caller-managed inference is measurably insufficient.
