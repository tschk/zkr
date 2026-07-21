# kr

Evidence-backed temporal memory for personal agents.

`kr-memory` keeps source evidence authoritative, represents facts as temporal claims, and produces bounded retrieval packs with citations. Embeddings and search indexes are projections that can be rebuilt from the stored evidence.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets --all-features
cargo test --all-features
```

