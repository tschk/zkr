# zkr

Evidence-backed temporal memory for personal agents.

`zkr` keeps source evidence authoritative, represents facts as temporal claims, and produces bounded retrieval packs with citations. Embeddings and search indexes are projections that can be rebuilt from the stored evidence.

## Principles

- Sources and evidence are authoritative; indexes are disposable.
- Claims keep both when they were true and when they were recorded.
- Corrections supersede history instead of silently rewriting it.
- Retrieval is bounded, tenant-scoped, and cited.
- Reflection proposes durable changes; it does not bypass evidence.

See [the architecture](docs/architecture.md), [embedding design](docs/embeddings.md), and [memory-system research](docs/research.md).

## Install

```sh
cargo install --git https://github.com/tschk/zkr
```

The library is consumed as the `zkr` crate. The CLI reads one JSON object from stdin and writes one JSON object to stdout.

```sh
printf '%s' '{"tenant_id":"local","person_id":"me","kind":"conversation","text":"I prefer short plans.","captured_at":1784615483,"claim":{"subject":"me","predicate":"prefers","value":"short plans","valid_from":1784615483}}' \
  | zkr --db ~/.zkr/memory.db remember

printf '%s' '{"tenant_id":"local","person_id":"me","query":"plans","limit":5}' \
  | zkr --db ~/.zkr/memory.db search
```

Run `zkr --help` for `correct`, `delete`, `review`, `reviews`, and `embed`.

## Agent plugins

### OpenClaw

Link or copy `plugins/openclaw` into an OpenClaw extension location, install it with Bun, then enable the `zkr` plugin. Its optional `command` and `database` settings default to `zkr` and `~/.zkr/memory.db`.

### Hermes Agent

Link or copy `plugins/hermes` to `$HERMES_HOME/plugins/zkr`, then set:

```yaml
memory:
  provider: zkr
```

Both plugins expose store, search, correction, deletion, and cited reflection through the neutral CLI. They do not add framework dependencies to the Rust crate.

## Development

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
