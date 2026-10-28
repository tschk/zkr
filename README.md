# zkr

Evidence-backed temporal memory for personal agents.

`zkr` keeps source evidence authoritative, represents facts as temporal claims, and produces bounded retrieval packs with citations. Raw captures remain searchable before a claim is extracted. Embeddings, search indexes, summary trees, and retrieval statistics are local projections that can be rebuilt from the stored evidence.

`MemoryMode::{Coding, Personal, Research, Agent}` provides scoped presets without changing the storage engine. Each preset selects a workspace, person, project, or agent boundary; an existing memory tier; bounded retrieval defaults; allowed source kinds; and profile eligibility. All modes require evidence citations and tenant/person privacy isolation while preserving correction history.

## Principles

- Sources and evidence are authoritative; indexes are disposable.
- Claims keep both when they were true and when they were recorded.
- Corrections supersede history instead of silently rewriting it.
- Retrieval is bounded, tenant-scoped, and cited.
- Search reranks lexical and dense candidates with scoped recency and exposure statistics; those scores never affect durable truth.
- Accepted claims replace their supporting raw capture in results instead of duplicating it.
- Reflection suggestions stay outside durable memory until a caller explicitly stores cited evidence, a claim, a correction, a profile projection, or a review.

See [the architecture](docs/architecture.md), [embedding design](docs/embeddings.md), and [memory-system research](docs/research.md).

Transcript captures can include an optional `locator` with `device_id`, `provider`, `stream_id`, `segment_id`, `start_ms`, and `end_ms`. The locator remains attached to its evidence citation and can be retrieved with the `locator` CLI command. Library callers can use `MemoryDb::remember_with_locator` without changing existing `RememberInput` code.

Library callers that retry `remember` with wall-clock timestamps can call `MemoryDb::ingestion_timestamps` to reuse the `captured_at`, `recorded_at`, and claim `valid_from` stored for an ingestion key, so an idempotent replay does not trip the ingestion key conflict check.

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

`correct` requires separate `valid_at` and `recorded_at` values. Add an `as_of` object with those same keys to `search` only when bitemporal history is required; ordinary retrieval returns current supported claims. Optional `aliases` on `remember` are extra FTS tokens only — they do not change source text, excerpts, or the commit feed. Run `zkr --help` for `wake`, `zoom`, `nap`, `merge`, `rebuild`, `link`, `promote`, `archive`, `profile`, `profiles`, `profile-pager`, `delete`, `repair`, `review`, `reviews`, `projections`, `embed`, `export`, and `apply`. `link` records supporting or contradicting evidence without changing a claim. `promote` moves a processed `short_term` claim to `long_term`; `archive` moves a processed claim out of live retrieval. `repair` processes bounded projection-repair outbox records and reports stale summary nodes. `profile` derives its key and value from a live `profile_fact` claim and keeps one current projection per scoped key. `projections` returns bounded stale or missing embedding inputs with the exact text, revision, and SHA-256 hash required by `embed`. Retrieval excerpts are capped at 4096 UTF-8 bytes while retaining their evidence citation.

## Agent session workflow

`nap` stores one cited leaf summary. `merge` combines two adjacent summaries, `zoom` opens a parent, and `wake` returns raw cited matches plus the newest-first detail cover that fits `max_bytes`. Summary nodes are local rebuildable projections: source deletion or claim correction marks them stale, and `rebuild` replaces a stale node with cited current inputs. `search` remains raw FTS/dense retrieval and `review` remains a separate durable cited artifact.

```sh
printf '%s' '{"tenant_id":"local","person_id":"me","query":"current rollout","limit":8}' \
  | zkr --db ~/.zkr/memory.db wake

printf '%s' '{"tenant_id":"local","person_id":"me","summary":"Rollout is Friday.","evidence_ids":["evidence-id"],"recorded_at":1785062400}' \
  | zkr --db ~/.zkr/memory.db nap
```

`export` returns a bounded, tenant/person-scoped page of authoritative commits. Send `export_format: 1` and start with `after_commit: 0`; keep the first page's `high_water_mark` fixed while requesting later pages with `next_after_commit` and `next_after_event_index` so one export observes a stable boundary. A host must stage contiguous event indexes from zero through `event_count - 1` for each commit, verify the declared count, atomically apply the complete commit, and only then acknowledge and durably advance its applied cursor. Request cursors may move while staging, but they are not proof that a commit was applied. Each serialized authoritative record shares the CLI request boundary's 1 MiB compatibility limit; an oversized write or migration fails explicitly and atomically. The feed includes durable sources, evidence, claims (including tier and processing-state updates), links, corrections, deletions, profiles, and reviews. It excludes embeddings, FTS indexes, summary trees, and retrieval statistics because they are rebuildable projections. The host owns authentication, transport, retries, acknowledgement state, and destination policy. Migration from schema v7 bootstraps the durable rows visible at upgrade time but cannot reconstruct explicit correction lineage that predates the v8 commit feed.

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

## Project categories, structured connections, and associative dreaming

`remember-project` groups an ordinary cited fact/task under an optional `project`
(defaulting to `"general"`) and one or more caller-defined category labels (`category`
and/or `categories`). Labels are caller-defined strings: an AI can create categories as
needed without changing a built-in taxonomy. Every stored project memory or structured
record is automatically vectorized with built-in projection embeddings (`zkr-auto`), or
callers can attach an inline `embedding` (`{ vector, model?, version?, normalization?, distance? }`)
on `remember-project` and `remember-project-record` in a single call.

`project-memories` retrieves current, processed, non-archived facts with optional
`project`, `projects`, `category`, `categories` (match any), `all_categories` (match all),
`predicate`, `record_type`, `relation`, `target`, `where_fields` (exact or array-membership
matching on custom JSON record fields), bidirectional multi-hop graph traversal
(`connected_to`, `max_hops`), and semantic vector ranking via `vector_query` or
`query_embedding` (`{ vector, model, version }`).

```sh
printf '%s' '{"tenant_id":"local","person_id":"me","project":"Tofu","categories":["launch","work"],"kind":"conversation","text":"Launch is planned","captured_at":10,"recorded_at":10,"claim":{"predicate":"task:launch:status","value":"planned","kind":"task","valid_from":10}}' \
  | zkr --db ./memory.db remember-project
printf '%s' '{"tenant_id":"local","person_id":"me","projects":["Tofu"],"all_categories":["launch"],"vector_query":"planned rollout","limit":20}' \
  | zkr --db ./memory.db project-memories
```

`remember-project-record` adds a named JSON record with optional `categories`, arbitrary
`fields`, and weighted/annotated `connections` (`weight_basis_points`, `note`).
`claim.subject` and `claim.value` are filled automatically:

```json
{
  "record_type": "work_item",
  "categories": ["launch", "milestone"],
  "fields": { "state": "blocked", "priority": 2, "tags": ["connector"] },
  "connections": [
    {
      "relation": "blocked_by",
      "target": "cupboard:pairing",
      "weight_basis_points": 9000,
      "note": "Waiting on pairing flow"
    }
  ]
}
```

The record replaces `claim.value` with its complete serialized JSON value; the
claim's predicate remains its stable logical record ID. `ProjectFact` includes
`project`, `category`, `categories`, the raw `value` string, the parsed `record`
(`ProjectRecord`), and optional `similarity_basis_points` when ranked by
`vector_query` or `query_embedding`. Use `correct` and new evidence to update that
claim (pass the entire serialized record for structured records).

`dream` inspects live embeddings and structured connections to return an emergent
organization and maintenance pack containing:
- `categories`: emergent category summaries with vectorized `centroid` prototypes,
- `category_suggestions`: vector-prototype category suggestions for categorizing related claims,
- `schemas`: emergent `record_type` schemas (observed field names, relations, and categories),
- `hubs`: high-degree graph hubs across incoming and outgoing connections,
- `associations`: cross-category vector similarity, triadic closure (`2-hop` bridges), and shared-target associations,
- `unresolved_targets`: connection targets that do not yet resolve to a live claim,
- `contradictions`: conflicting live claims sharing a subject and predicate,
- `short_term_claims`: `short_term` claims ready for `promote`,
- `unsummarized_evidence_ids`: evidence ready for `nap`, and
- `stale_summary_ids`: summary nodes marked stale and ready for `rebuild`.

Model-made categories and connections remain caller-supplied claims, not
owner-confirmed facts, a schema migration, generated executable code, or execution
authority. Connection targets are exact named subjects and may be unresolved; they
are not foreign keys and never fetch another owner's memory. The source claim's
normal scope, citation, feature gates, correction, deletion and replication apply.
Records are limited to 16 KiB, 64 fields and 64 connections; reads retain ZKR's
usual count and byte bounds. No schema migration or new dependency is required. The
APIs organize memory; they do not schedule or execute tasks.
