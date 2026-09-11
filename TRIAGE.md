# Open PR triage — tschk/zkr

Status snapshot as of 2026-09-11. Read-only review of the listed open PRs. No PRs were merged or closed. twenify was skipped (out of scope).

All nine PRs are Jules-generated, target `main` at `f8f7e80`, and current `main` is `f8e6cb8` (five later commits, no expected hard conflicts on the real hunks). Most also carry the same unrelated boilerplate: `schema.rs` import wrap and a `plugins/openclaw/cli.ts` indent fix. That boilerplate will conflict if several land in sequence; resolve by keeping one copy.

CI jobs: `rust` (fmt + clippy `-D warnings` + test), `openclaw`, `hermes`. Combined commit-status API reports `pending` for all of these (no commit statuses); classifications use check runs.

## Summary

| PR | Title | Verdict | CI (latest head) |
|----|-------|---------|------------------|
| [#75](https://github.com/tschk/zkr/pull/75) | Refactor `correct` in `lifecycle.rs` | **MERGE_READY** | rust/openclaw/hermes green |
| [#74](https://github.com/tschk/zkr/pull/74) | Bulk retrieval-target N+1 | **NEEDS_WORK** | rust fail (clippy); openclaw flaky |
| [#73](https://github.com/tschk/zkr/pull/73) | `store_review` evidence N+1 | **MERGE_READY** | green (re-run 2026-09-11) |
| [#71](https://github.com/tschk/zkr/pull/71) | `validate_evidence` N+1 (summaries) | **MERGE_READY** | green |
| [#70](https://github.com/tschk/zkr/pull/70) | Export profile-existence N+1 | **MERGE_READY** | green |
| [#69](https://github.com/tschk/zkr/pull/69) | Parameterize `pragma_table_info` | **CLOSE_CANDIDATE** | green |
| [#65](https://github.com/tschk/zkr/pull/65) | Static SQL in `projection_input_from` | **MERGE_READY** | green |
| [#64](https://github.com/tschk/zkr/pull/64) | `augment` empty-lessons test | **NEEDS_WORK** | rust/hermes green; latest openclaw EPIPE fail |
| [#63](https://github.com/tschk/zkr/pull/63) | Parameterize `pragma_table_info` | **MERGE_READY** | green |

Suggested land order if taking the ready set: **#63 → #65 → #70 → #71 → #73 → #75**. Close **#69** as a duplicate of #63. Hold **#74**. Re-run **#64** CI before merge.

## Overlap

| Cluster | PRs | Notes |
|---------|-----|-------|
| `ensure_column` / `pragma_table_info` | #63, #69 | Same schema fix. #69 also adds an out-of-scope OpenClaw stdin EPIPE handler. Keep #63. |
| Embeddings SQL literals | #65 | Independent of #63/#69. False-positive “SQL injection”; still a clean hygiene change. |
| Evidence N+1 | #73, #71 | Same `json_each` idea, different modules (`store_review` vs `validate_evidence`). Not duplicates. Both `Cargo.toml` benches must be kept. |
| `lifecycle.rs` | #75, #73, #70 | Different functions (`correct`, `store_review`, `build_deletion_records`). Should auto-merge. |
| Retrieval bulk SQL | #74 | Isolated Rust change; only shares boilerplate files. |
| OpenClaw EPIPE | #69, #73, #70 | Same stdin `error` swallow. Land once. Latest #64/#74 openclaw failures look like this flake. |

---

## #75 — MERGE_READY

https://github.com/tschk/zkr/pull/75 · `refactor-correct-10844817778989965663` · `64683cbc`

**What.** Extracts `MemoryDb::correct` into helpers (`find_correction_targets`, `validate_correction_time`, `insert_correction_source_and_evidence`, `supersede_stale_claim`, `apply_correction_claim`, `build_correction_records`) plus an `OldClaim` struct.

**Correctness.** Pipeline order, SQL, and export-record shape match the pre-refactor path. Covered by existing lifecycle/export/summary tests.

**Overlap.** Same file as #73/#70 but different functions. Boilerplate `schema.rs` / `cli.ts` only.

**Security.** None claimed.

**CI.** All checks green.

---

## #74 — NEEDS_WORK

https://github.com/tschk/zkr/pull/74 · `jules-fix-embedding-nplus1-8633657681382795521` · `d6087cf9`

**What.** Replaces per-candidate `retrieval_targets_for_embedding` with `retrieval_targets_for_embeddings_bulk` (`json_each` + `UNION ALL`) and two-phase dense scoring.

**Correctness.** Score aggregation (`max` per target) is fine. Source fallback is not: the old path required a live-evidence join before emitting `RetrievalTarget::Source`; the bulk fallback emits Source whenever the source exists and has no supporting claims, including sources with zero live evidence. Downstream `retrieval_item` can then `NotFound` and abort dense search. An earlier commit on the branch (`bc71d6c`) added the live-evidence join and a regression test; a later commit dropped both. Bugbot flagged the same issue.

**Overlap.** Isolated from the other N+1 PRs except boilerplate.

**Security.** Not a security PR. JSON input is escaped via `serde_json::to_string`.

**CI.** `rust` fails on both head runs: `clippy::map_entry` at `src/store/retrieval.rs` (`contains_key` + `insert` for empty vecs). One `openclaw` run failed (EPIPE flake). Tests still pass locally because the regression test was deleted.

**Needed before merge.** Restore the live-evidence join and `dense_source_without_live_evidence_does_not_abort_search`; fix `map_entry`. That is more than a tiny fix-up, so this branch was left untouched.

---

## #73 — MERGE_READY

https://github.com/tschk/zkr/pull/73 · `perf/evidence-validation-nplus1-11381485382194887014` · `a0d2438f`

**What.** `store_review` validates `evidence_ids` with one `json_each EXCEPT` query instead of per-ID `EXISTS`. Adds `store_review_evidence_nplus1` bench.

**Correctness.** Tenant/person/`deleted_at IS NULL` filters match the old loop. Error string is unchanged. `EXCEPT` result order is undefined, so if several IDs are missing the reported ID may differ from input order. Typical single-miss path is identical.

**Overlap.** Not a duplicate of #71. Conflicts with #71 on `Cargo.toml` (keep both benches) and with #70/#69 on the `cli.ts` EPIPE handler.

**Security.** None.

**CI.** Green on 2026-09-11 re-run.

---

## #71 — MERGE_READY

https://github.com/tschk/zkr/pull/71 · `perf/optimize-validate-evidence-3504700469850662475` · `f0c636be`

**What.** Error path in `summaries::validate_evidence`: after a count mismatch, find the first missing ID with one `json_each` + `NOT EXISTS` instead of a per-ID loop. Adds `validate_evidence` bench.

**Correctness.** Happy path unchanged (still the bulk `COUNT`). First-missing-ID order is preserved (`json_each` walks the array; `LIMIT 1`). Same live-evidence scope.

**Overlap.** Complementary to #73, not redundant.

**Security.** None.

**CI.** Green.

---

## #70 — MERGE_READY

https://github.com/tschk/zkr/pull/70 · `optimize-profile-export-nplus1-17247301270499260556` · `99915920`

**What.** `build_deletion_records` loads existing `profile_entries` IDs in one `json_each` query and emits deletions for IDs not in that set.

**Correctness.** Tenant/person scoping matches the old per-ID `EXISTS`. Neither old nor new filters `deleted_at` on profile entries. Semantics unchanged.

**Overlap.** Different function from #73/#75. Shares EPIPE handler with #73/#69.

**Security.** None.

**CI.** Green.

---

## #69 — CLOSE_CANDIDATE

https://github.com/tschk/zkr/pull/69 · `fix-sql-injection-ensure-column-5465577183885091728` · `6880a48d`

**What.** Same `ensure_column` change as #63, plus an unrelated OpenClaw stdin EPIPE handler.

**Correctness.** `pragma_table_info(?1)` with bound table/column is valid in rusqlite/SQLite and equivalent to the interpolated form.

**Overlap.** Duplicate of #63. Prefer #63 (tighter scope, fewer commits).

**Security.** False positive for exploitability. `ensure_column` is private, call sites pass hardcoded identifiers, and `is_valid_identifier` already rejects anything outside `[A-Za-z0-9_]`. Defense-in-depth only. `ALTER TABLE` still uses `format!` after the same check (DDL identifiers cannot be bound).

**CI.** Green.

---

## #65 — MERGE_READY

https://github.com/tschk/zkr/pull/65 · `fix/sql-injection-embeddings-6605308632245666886` · `95db4a73`

**What.** Replaces `format!` over `EmbeddingTarget` fragments in `projection_input_from` with three static `SELECT` strings.

**Correctness.** Table/column/`WHERE` fragments were already compile-time literals; IDs were already bound as `?1`–`?3`. Behavior and `NotFound` handling are unchanged.

**Overlap.** Independent of #63/#69. Only shares boilerplate files.

**Security.** False positive. Closed enum, no user-controlled SQL fragments. Fine as SAST hygiene; not a reachable injection.

**CI.** Green.

---

## #64 — NEEDS_WORK

https://github.com/tschk/zkr/pull/64 · `add-self-improve-edge-case-test-229032979685625290` · `ab46a891`

**What.** Test-only: `augment_with_no_lessons_returns_base` asserts `SelfImprove::augment` returns the base string when `lessons()` is empty.

**Correctness.** Matches the early return in `augment`. Complements `augment_injects_lessons_into_prompt`. No production change.

**Overlap.** Real hunk is only `src/self_improve.rs`. Boilerplate `schema.rs` / `cli.ts` will churn with the others.

**Security.** None.

**CI.** `rust` and `hermes` green. Latest `openclaw` failed on `rejects malformed successful CLI output` with `EPIPE` (`index.test.ts`). Same flake appears on #74; #75’s identical indent-only `cli.ts` change is green. Re-run CI (or cherry-pick the stdin EPIPE handler from #73/#70 once). Not a tiny production fix-up, so the branch was left as-is.

---

## #63 — MERGE_READY

https://github.com/tschk/zkr/pull/63 · `fix-schema-sql-injection-17164826925547437694` · `ae1c4f2d`

**What.** Binds table and column in `SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)`. Also the usual `cli.ts` indent (no EPIPE handler).

**Correctness.** Same as #69; migration tests cover this path.

**Overlap.** Winner vs #69.

**Security.** Same false-positive / defense-in-depth as #69. Worth landing once so the duplicate can be closed.

**CI.** Green.

---

## Actions not taken

- Did not merge or close any PR.
- Did not push fix-ups. The only correctness+CI blocker (#74 source fallback + clippy) is not a tiny change.
- Did not review twenify.
