# Embeddings

Embeddings improve recall but never define truth.

Each vector projection records:

- the durable record and revision it represents;
- model, model version, dimension, normalization, and distance metric;
- a hash of the exact embedded input;
- creation time.

Vectors declared as L2-normalized must have unit magnitude. A historical `as_of` search uses lexical retrieval only: current projections are intentionally not treated as evidence of historical embedding state.

A changed source revision, a tier or processing-state change, or a malformed stored vector makes the old vector stale. Rebuilding vectors must not mutate sources, evidence, claims, or reviews. A model/version identifies one dimension, normalization, and distance lane within a tenant and person; use a new version when migrating that configuration.

Lifecycle writes enqueue records in a projection-repair outbox. Run `repair` with a tenant, person, and limit to drain the outbox idempotently: it deletes or revalidates embeddings, FTS rows, and any future graph projections that no longer match the current authoritative state.

Run `projections` with a tenant, person, model, version, and limit to get only stale or missing work. Each item includes the current text, SHA-256 input hash, target revision, and any stored projection metadata. Embed that exact text and submit the returned hash to `embed`; zkr rejects a vector for any other input. This keeps model execution caller supplied while making rebuilds bounded and auditable.

```sh
printf '%s' '{"tenant_id":"local","person_id":"me","model":"provider/model","version":"1","limit":25}' \
  | zkr --db ~/.zkr/memory.db projections
```

Search works without embeddings through SQLite FTS. When real vectors exist, zkr combines lexical and dense candidates into one bounded cited retrieval pack. It does not ship a fake hash embedder or silently mix incompatible vector spaces.

Callers generate the query vector with the same model and version used for stored projections, then pass it to `search`:

```json
{
  "tenant_id": "local",
  "person_id": "me",
  "query": "where I like to work",
  "limit": 5,
  "query_embedding": {
    "vector": [0.12, -0.04, 0.31],
    "model": "provider/model",
    "version": "1"
  }
}
```

zkr scans matching persisted vectors exactly and combines their dense rank with SQLite FTS using deterministic reciprocal-rank fusion. Before scoring, it compares every vector's recorded revision, hash, dimension, normalization, distance, and vector encoding with the current live target and lane, so stale, malformed, mixed, and deleted projections cannot affect retrieval. Live source and evidence projections map to accepted cited claims when available; otherwise they remain directly retrievable as cited raw memory. New writes that would mix configurations within a model version are rejected.

The initial model remains a caller choice. Before selecting a default, compare multilingual recall, latency, binary size, memory use, and platform support on LoCoMo, LongMemEval, and a private correction/deletion corpus.
