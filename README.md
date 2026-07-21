# zkr

Evidence-backed temporal memory for personal agents.

`zkr` keeps source evidence authoritative, represents facts as temporal claims, and produces bounded retrieval packs with citations. Raw captures remain searchable before a claim is extracted. Embeddings and search indexes are projections that can be rebuilt from the stored evidence.

## Principles

- Sources and evidence are authoritative; indexes are disposable.
- Claims keep both when they were true and when they were recorded.
- Corrections supersede history instead of silently rewriting it.
- Retrieval is bounded, tenant-scoped, and cited.
- Accepted claims replace their supporting raw capture in results instead of duplicating it.
- Reflection proposes durable changes; it does not bypass evidence.

See [the architecture](docs/architecture.md), [embedding design](docs/embeddings.md), and [memory-system research](docs/research.md).

Transcript captures can include an optional `locator` with `device_id`, `provider`, `stream_id`, `segment_id`, `start_ms`, and `end_ms`. The locator remains attached to its evidence citation and can be retrieved with the `locator` CLI command. Library callers can use `MemoryDb::remember_with_locator` without changing existing `RememberInput` code.

## Install

```sh
cargo install zkr
```

The library is consumed as the `zkr` crate. The CLI reads one JSON object from stdin and writes one JSON object to stdout.

```sh
printf '%s' '{"tenant_id":"local","person_id":"me","kind":"conversation","text":"I prefer short plans.","captured_at":1784615483,"claim":{"subject":"me","predicate":"prefers","value":"short plans","valid_from":1784615483}}' \
  | zkr --db ~/.zkr/memory.db remember

printf '%s' '{"tenant_id":"local","person_id":"me","query":"plans","limit":5}' \
  | zkr --db ~/.zkr/memory.db search
```

Run `zkr --help` for `correct`, `delete`, `review`, `reviews`, `projections`, and `embed`. `projections` returns bounded stale or missing work with the exact text, revision, and SHA-256 hash required by `embed`.

## Agent plugins

### OpenClaw

Link or copy `plugins/openclaw` into an OpenClaw extension location, install it with Bun, then enable the `zkr` memory slot. It implements OpenClaw's native memory capability plus `memory_search` and `memory_get`, while preserving the explicit `zkr_*` tools. Its optional `command`, `database`, `tenant`, and `person` settings default to `zkr`, `~/.zkr/memory.db`, `openclaw`, and the active agent ID.

### Hermes Agent

Link or copy `plugins/hermes` to `$HERMES_HOME/plugins/zkr`, then set:

```yaml
memory:
  provider: zkr
```

Both plugins expose store, search, correction, deletion, and cited reflection through the neutral CLI. Hermes persists completed turns to a local write-behind queue before returning, recovers pending turns on startup, flushes on shutdown, and skips non-primary agent contexts. The plugins do not add framework dependencies to the Rust crate.

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
