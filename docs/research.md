# Memory-system research

zkr should keep durable, cited records as truth and treat every index, summary, graph edge, and embedding as a rebuildable projection. The Rust crate and CLI remain framework-neutral; integrations belong in `plugins/`.

## Decisions

| System | Relevant evidence | Decision for zkr |
| --- | --- | --- |
| [Omi](https://github.com/BasedHardware/omi) | Its canonical memory work separates short-, long-, and archive-tier items; retains source evidence; versions ledger mutations; invalidates superseded facts; and purges vectors and graph citations when sources are retracted. | **Borrow:** source-backed claims, explicit lifecycle states, deterministic commits, projection-repair outbox, and fail-closed deletion. **Avoid:** coupling the neutral crate to Firebase, Pinecone, or Omi policy. |
| [rs_gbrain](https://github.com/undivisible/rs_gbrain) | Demonstrates a small Rust design around SQLite FTS, typed links, injected embeddings, and bounded retrieval packs. | **Borrow:** practical SQLite/FTS patterns and a caller-supplied embedder boundary. **Avoid:** hash-derived pseudo-embeddings and heuristic links presented as semantic truth. |
| [Mem0](https://github.com/mem0ai/mem0) | Its current design combines semantic, BM25, entity, and temporal signals; its published benchmark harness is separate from the managed implementation. | **Borrow:** multi-signal candidate fusion and reproducible evaluation. **Avoid:** copying self-reported scores or ADD-only accumulation without testing correction and deletion behavior. |
| [Graphiti](https://github.com/getzep/graphiti) / [Zep paper](https://arxiv.org/abs/2501.13956) | Facts retain provenance to episodes and validity windows; retrieval combines semantic, keyword, and graph traversal. | **Borrow:** valid-time intervals, recorded-time history, source lineage, and graph-assisted multi-hop retrieval. **Defer:** a mandatory graph database until SQLite adjacency proves insufficient. |
| [HippoRAG 2](https://github.com/OSU-NLP-Group/HippoRAG) | Uses graph structure for associativity and multi-hop retrieval while preserving ordinary factual retrieval. | **Borrow:** graph expansion as an optional reranking signal. **Defer:** LLM-heavy offline graph construction until it beats lexical+dense retrieval on zkr data. |
| [A-MEM](https://github.com/WujiangXu/A-mem) | Generates structured notes, links related memories, and evolves organization through reflection. | **Borrow:** bounded reflection that proposes links and refinements. **Avoid:** letting model-authored rewrites replace source evidence or bypass deterministic validation. |
| [MemOS](https://github.com/MemTensor/MemOS) | Exposes add, retrieve, edit, and delete over inspectable graph memory; supports correction and isolated memory collections. | **Borrow:** explicit feedback/correction operations and scoped collections. **Defer:** schedulers, multimodal pipelines, and memory-cube orchestration to host applications. |
| [Hindsight](https://github.com/vectorize-io/hindsight) | Separates factual information, experiences, and opinions and uses retain, recall, and reflect operations. | **Borrow:** reflection as a distinct cited operation and explicit memory categories. **Avoid:** requiring a server or provider-specific model stack in the core crate. |
| [Letta / MemGPT](https://arxiv.org/abs/2310.08560) | Treats context as tiered memory managed under a finite prompt budget. | **Borrow:** small bounded retrieval packs and an editable hot profile. **Avoid:** embedding agent runtime, prompt paging, or tool orchestration in zkr. |
| [LangGraph memory](https://docs.langchain.com/oss/python/langgraph/memory) | Separates semantic facts, episodic experience, and procedural instructions; scopes long-term storage by application-defined namespaces; leaves hot-path versus background writes to the host. | **Borrow:** explicit categories and caller-owned namespaces. **Avoid:** checkpointers, graph execution, and background-memory scheduling in zkr. |
| [Cognee](https://github.com/topoteretes/cognee) | Combines embeddings, knowledge graphs, and ontology generation behind remember/recall/forget/improve operations; its agent integrations own session lifecycle and background synchronization. | **Borrow:** inspectable deletion and category-aware retrieval. **Avoid:** mandatory LLM, graph, service, telemetry, or lifecycle dependencies in the neutral core. |
| [Supermemory](https://github.com/supermemoryai/supermemory) | Maintains user profiles, handles updates and contradictions, and combines memory with hybrid document search. | **Borrow:** separate stable profile facts from current activity and expire projections deliberately. **Avoid:** adopting benchmark claims without a reproducible, equivalent configuration. |
| [EverOS](https://github.com/EverMind-AI/EverMemOS) | Keeps readable source material distinct from SQLite and vector indexes and scopes retrieval by user, agent, app, project, and session. | **Borrow:** orthogonal tenancy keys and rebuildable indexes. **Defer:** Markdown as an additional authority; zkr already has an append-only record model. |

## Required invariants

| Concern | zkr rule |
| --- | --- |
| Evidence | Every derived claim and review cites exact source revisions and spans. Retrieval returns citations and explicit gaps. |
| Corrections | Claims have valid time and recorded time. A correction closes or contradicts prior validity; it does not overwrite history. |
| Embeddings | A vector stores record ID, source revision, model ID and revision, dimension, normalization, distance metric, input hash, and creation time. Vectors never grant truth status. |
| Index lifecycle | Changed text or configuration marks prior vectors stale. Reindexing is idempotent. Mixed vector spaces are queried separately or rejected, never silently compared. |
| Hybrid retrieval | FTS and dense search generate candidates. Optional graph and recency signals rerank them. Reciprocal-rank fusion is the first implementation because it does not require score calibration. |
| Reflection | A model emits cited proposals. Deterministic checks and normal lifecycle operations decide whether they become durable. |
| Deletion | Source tombstones synchronously hide affected facts from reads and enqueue idempotent FTS, vector, graph, and summary cleanup. A stale projection cannot resurrect deleted data. |
| Tenancy | Tenant and subject scope are mandatory in durable rows, indexes, cache keys, retrieval, export, and deletion. Cross-tenant retrieval is a failed invariant, not a filter applied later. |

## Embedding benchmark

No default model should be selected before measurement. Compare these documented candidates using the same normalized inputs and retrieval pipeline:

| Candidate | Why include it | Target lane |
| --- | --- | --- |
| [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) | Small 384-dimensional English baseline with broad runtime support. | Mobile and desktop latency floor |
| [multilingual-e5-small](https://huggingface.co/intfloat/multilingual-e5-small) | Compact 384-dimensional multilingual retrieval model. | Mobile, desktop, multilingual baseline |
| [EmbeddingGemma](https://huggingface.co/google/embeddinggemma-300m) | On-device-oriented multilingual model with Matryoshka dimensions. | Newer mobile and desktop devices |
| [nomic-embed-text-v1.5](https://huggingface.co/nomic-ai/nomic-embed-text-v1.5) | Longer-context model with Matryoshka dimension choices. | Desktop and cloud |
| [BGE-M3](https://huggingface.co/BAAI/bge-m3) | Multilingual, long-context dense/sparse/multi-vector reference. | Cloud quality ceiling |

Run one locked matrix:

1. Use [LoCoMo](https://github.com/snap-research/locomo) for conversational recall and multi-hop questions, [LongMemEval](https://github.com/xiaowu0162/LongMemEval) for extraction, multi-session reasoning, updates, temporal reasoning, and abstention, and [MemoryAgentBench](https://github.com/HUST-AI-HYZ/MemoryAgentBench) for retrieval, test-time learning, long-range understanding, and conflict resolution.
2. Add a zkr corpus with multilingual paraphrases, current-versus-former facts, negation, repeated corrections, source deletion, and two tenants containing intentionally similar facts. Each query has allowed evidence IDs and an abstain expectation.
3. Compare FTS-only, dense-only, FTS+dense RRF, and FTS+dense+graph RRF at fixed candidate budgets. Measure Recall@5/10/20, MRR@10, nDCG@10, citation precision, current-fact accuracy, contradiction accuracy, deletion leakage, tenant leakage, and abstention F1.
4. On representative Android, iOS, macOS, Windows, and Cloudflare-compatible service hardware, record cold start, p50/p95 embedding latency, p50/p95 query latency, peak RSS, model and index bytes, energy per 100 embeddings where available, and cost per million embedded tokens for remote models.
5. Run full precision and supported quantizations at each advertised Matryoshka dimension. Reject configurations that alter normalization or truncate inputs without recording that choice. Repeat each performance run at least five times and publish medians plus raw results.

Selection is by lane, not one global winner: choose the smallest configuration meeting a predeclared retrieval-quality floor on mobile, the best quality-per-latency configuration on desktop, and the best quality-per-dollar configuration in cloud. Keep FTS available when no embedding runtime is present.

## Evaluation cadence

- Every change: zkr correction/deletion/tenancy corpus and FTS-only regression.
- Before release: frozen embedding matrix and LongMemEval subset, with model and dataset revisions pinned.
- Nightly: full LongMemEval, MemoryAgentBench, and [EverMemBench](https://github.com/EverMind-AI/EverMemBench); track quality, latency, index size, and token cost separately.
- Before adopting graph expansion or reflection: require a statistically repeatable gain over FTS+dense RRF without regression in deletion, tenancy, or citation precision.

## Scope boundary

The first implementation needs SQLite FTS, an embedding import/export contract, exact vector scan for small local stores, reciprocal-rank fusion, citations, and lifecycle tests. Approximate nearest-neighbor indexes, graph databases, bundled model runtimes, background schedulers, and hosted services wait for benchmark evidence.
