# Architecture

zkr separates durable truth from rebuildable retrieval machinery.

## Durable records

1. A source records an observation and keeps append-only revisions.
2. Evidence points to an exact source revision and span.
3. A claim records a fact, profile fact, preference, task, skill, or recommendation with valid time and recorded time.
4. Claim-evidence links say whether evidence supports or contradicts a claim.
5. Profile entries expose one deterministic stable/current projection per scoped claim predicate; their key and value are derived from a live profile-fact claim.
6. Daily Reviews are cited text artifacts, not a second source of truth.

Deleting a source tombstones it. Derived records without remaining support are retracted and are unavailable to retrieval. Explicit bitemporal history exposes prior non-deleted claim states without recreating deleted evidence.
Correcting or superseding a claim closes both of its half-open time ranges using separately supplied valid and recorded timestamps before recording the replacement.
Schema upgrades run as ordered immediate transactions. The v5-to-v6 upgrade rebuilds profile projections from their backing profile-fact claims; it does not alter those claims or their evidence.

## Retrieval

Keyword and vector results are projections over durable records. A retrieval pack is bounded and contains citations plus explicit gaps. Before extraction, a pack can cite a live source or evidence record directly; after an accepted claim has supporting evidence, retrieval returns the claim without also emitting that supporting source. Contradicting evidence remains available as raw evidence until an explicit correction or supersession. A caller can answer from the pack, request more evidence, or say the memory is insufficient.

## Reflection

Reflection reads a bounded cited pack and may suggest claims, corrections, profile changes, review text, or procedural lessons. Suggestions are caller-owned and are not durable zkr records. A caller must explicitly invoke the ordinary cited storage, correction, profile, or review operation; none of those operations can rewrite observations or evidence.

## Boundaries

The Rust crate and CLI remain agent-framework neutral. Native adapters live under `plugins/`; they translate framework tools into the same CLI operations without becoming memory authorities.

The authoritative commit feed was informed by Omi's device-to-cloud integration requirements but remains host-neutral. Each mutation emits ordered, tenant/person-scoped durable records. A caller freezes the first export page's high-water mark and advances its request commit and event cursors until that boundary is complete. The destination stages contiguous event indexes from zero through the declared `event_count - 1`, verifies the count, applies the complete commit atomically, and only then acknowledges it and durably advances its applied cursor. A page boundary may split a commit, so a request cursor is never an applied cursor. Each serialized authoritative record has the same 1 MiB compatibility limit as a CLI request; an oversized write or migration fails explicitly and atomically. Embeddings and FTS data are excluded because they can be rebuilt. The host, not zkr, owns transport, authentication, retries, acknowledgements, scheduling, and destination storage. The v7-to-v8 bootstrap exports the durable state visible during migration, but it cannot reconstruct explicit correction lineage from before the commit feed existed.
