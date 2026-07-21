# Architecture

zkr separates durable truth from rebuildable retrieval machinery.

## Durable records

1. A source records what was observed and keeps append-only revisions.
2. Evidence points to an exact source revision and span.
3. A claim records subject, predicate, value, confidence, valid time, and recorded time.
4. Claim-evidence links say whether evidence supports or contradicts a claim.
5. Profile entries expose a small editable stable/current view backed by claims.
6. Daily Reviews are cited text artifacts, not a second source of truth.

Deleting a source tombstones it. Derived records without remaining support are retracted; history remains inspectable.

## Retrieval

Keyword, vector, graph, and recency results are projections over durable records. A retrieval pack is bounded and contains citations plus explicit gaps. A caller can answer from the pack, request more evidence, or say the memory is insufficient.

## Reflection

Reflection reads a bounded cited pack and emits proposals: claims, corrections, profile changes, review text, or procedural lessons. Deterministic validation and normal lifecycle rules apply before anything becomes durable.

## Boundaries

The Rust crate and CLI remain agent-framework neutral. Native adapters live under `plugins/`; they translate framework tools into the same CLI operations without becoming memory authorities.
