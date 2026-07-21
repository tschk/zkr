# Embeddings

Embeddings improve recall but never define truth.

Each vector projection records:

- the durable record and revision it represents;
- model, model version, dimension, normalization, and distance metric;
- a hash of the exact embedded input;
- creation time.

A changed source revision or embedding configuration makes the old vector stale. Rebuilding vectors must not mutate sources, evidence, claims, or reviews.

Search works without embeddings through SQLite FTS. When real vectors exist, zkr combines lexical and dense candidates into one bounded cited retrieval pack. It does not ship a fake hash embedder or silently mix incompatible vector spaces.

The initial model remains a caller choice. Before selecting a default, compare multilingual recall, latency, binary size, memory use, and platform support on LoCoMo, LongMemEval, and a private correction/deletion corpus.
