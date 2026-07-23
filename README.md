# zkr

Evidence-backed temporal memory for personal agents.

`zkr` keeps source evidence authoritative, represents facts as temporal claims, and produces bounded retrieval packs with citations. Raw captures remain searchable before a claim is extracted. Embeddings and search indexes are projections that can be rebuilt from the stored evidence.

## Principles

- Sources and evidence are authoritative; indexes are disposable.
- Claims keep both when they were true and when they were recorded.
- Corrections supersede history instead of silently rewriting it.
- Retrieval is bounded, tenant-scoped, and cited.
- Accepted claims replace their supporting raw capture in results instead of duplicating it.
- Reflection suggestions stay outside durable memory until a caller explicitly stores cited evidence, a claim, a correction, a profile projection, or a review.

See [the architecture](docs/architecture.md), [embedding design](docs/embeddings.md), and [memory-system research](docs/research.md).

Transcript captures can include an optional `locator` with `device_id`, `provider`, `stream_id`, `segment_id`, `start_ms`, and `end_ms`. The locator remains attached to its evidence citation and can be retrieved with the `locator` CLI command. Library callers can use `MemoryDb::remember_with_locator` without changing existing `RememberInput` code.

## Install

```sh
cargo install zkr
```

The library is consumed as the `zkr` crate. The CLI reads one JSON object of at most 1 MiB from stdin and writes one JSON object to stdout.

```sh
printf '%s' '{"tenant_id":"local","person_id":"me","kind":"conversation","text":"I prefer short plans.","captured_at":1784615483,"recorded_at":1784615484,"claim":{"subject":"me","predicate":"prefers","value":"short plans","kind":"preference","valid_from":1784615483}}' \
  | zkr --db ~/.zkr/memory.db remember

printf '%s' '{"tenant_id":"local","person_id":"me","query":"plans","limit":5}' \
  | zkr --db ~/.zkr/memory.db search
```

`correct` requires separate `valid_at` and `recorded_at` values. Add an `as_of` object with those same keys to `search` only when bitemporal history is required; ordinary retrieval returns current supported claims. Run `zkr --help` for `link`, `promote`, `archive`, `profile`, `profiles`, `delete`, `repair`, `review`, `reviews`, `projections`, `embed`, `export`, and `apply`. `link` records supporting or contradicting evidence without changing a claim. `promote` moves a processed `short_term` claim to `long_term`; `archive` moves a processed claim out of live retrieval. `repair` processes bounded projection-repair outbox records. `profile` derives its key and value from a live `profile_fact` claim and keeps one current projection per scoped key. `projections` returns bounded stale or missing work with the exact text, revision, and SHA-256 hash required by `embed`. Retrieval excerpts are capped at 4096 UTF-8 bytes while retaining their evidence citation.

`export` returns a bounded, tenant/person-scoped page of authoritative commits. Send `export_format: 1` and start with `after_commit: 0`; keep the first page's `high_water_mark` fixed while requesting later pages with `next_after_commit` and `next_after_event_index` so one export observes a stable boundary. A host must stage contiguous event indexes from zero through `event_count - 1` for each commit, verify the declared count, atomically apply the complete commit, and only then acknowledge and durably advance its applied cursor. Request cursors may move while staging, but they are not proof that a commit was applied. Each serialized authoritative record shares the CLI request boundary's 1 MiB compatibility limit; an oversized write or migration fails explicitly and atomically. The feed includes durable sources, evidence, claims (including tier and processing-state updates), links, corrections, deletions, profiles, and reviews. It excludes embeddings and FTS indexes because they are rebuildable projections. The host owns authentication, transport, retries, acknowledgement state, and destination policy. Migration from schema v7 bootstraps the durable rows visible at upgrade time but cannot reconstruct explicit correction lineage that predates the v8 commit feed.

`apply` is the inverse of `export`: it materializes authoritative commits authored elsewhere into a local database. Send `export_format: 1`, the destination `tenant_id` and `person_id`, and a `commits` array of complete commits taken from an export page, optionally with the origin's `database_schema_version`. Record identity is caller supplied; `apply` never mints ids, so a cloud-authored record keeps the id its authority assigned. Every commit is applied inside one transaction and every record is deduplicated on its `(record_kind, record_id, payload_hash)` ledger entry, so replaying a page is a no-op that returns `records_skipped` instead of writing again. Records inside one commit are applied in dependency order rather than event order, and a record whose source, evidence, or claim is neither already stored nor present earlier in the batch fails with an explicit error instead of writing a dangling reference. Evidence transcript locators are stored verbatim and never re-derived; a page that contradicts a stored payload, revives a tombstone, or arrives from a newer schema is rejected. Applied records join the local commit feed, so a replica re-exports what it applied. Embeddings are not exported, so applied records start with missing projections that `projections` reports and `embed` fills; FTS rows are rebuilt during apply, so applied sources are searchable immediately.

## Agent plugins

Native plugin sources are available from the repository checkout and are not included in the crates.io package.

### OpenClaw

Link or copy `plugins/openclaw` into an OpenClaw extension location, install it with Bun, then enable the `zkr` memory slot. It implements OpenClaw's native memory capability plus `memory_search` and `memory_get`, while preserving the explicit `zkr_*` tools. Its optional `command`, `database`, `tenant`, and `person` settings default to `zkr`, `~/.zkr/memory.db`, `openclaw`, and the active agent ID. The plugin is tested against OpenClaw `2026.7.1-2` (plugin API and gateway); its Bun ES module sources invoke only the local zkr CLI, so it does not provide an embedding service, telemetry, host scheduling, or agent lifecycle management.

### Hermes Agent

Link or copy `plugins/hermes` to `$HERMES_HOME/plugins/zkr`, then set:

```yaml
memory:
  provider: zkr
```

Both plugins expose store, search, correction, deletion, and cited reflection through the neutral CLI. Hermes supports Hermes Agent 0.19.0 and Python 3.11–3.13 through its native Python `MemoryProvider` contract, keeps `captured_at`, `valid_from`, and `recorded_at` distinct, persists completed turns to a local write-behind queue before returning, recovers pending turns on startup, flushes on shutdown, and skips non-primary agent contexts. Queue records outside the configured tenant and person are quarantined. The plugins do not add framework dependencies to the Rust crate.

## Development

OpenClaw's real loader requires Node's `node:sqlite`, so it cannot run under this repository's Bun-only JavaScript gates. The plugin tests compile against the installed SDK, exercise capability registration and runtime search/read behavior, and build the distributable; run `openclaw plugins inspect zkr --runtime` in a supported OpenClaw Node runtime for the external loader check.

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets --all-features
cargo test --all-features
```

## License

[ISC](LICENSE)

## Acknowledgements

The systems and research that informed zkr are listed in [ACKNOWLEDGEMENTS.md](ACKNOWLEDGEMENTS.md).
