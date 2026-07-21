import { Type } from "typebox";
import { definePluginEntry } from "openclaw/plugin-sdk/plugin-entry";
import type { MemoryPluginRuntime } from "openclaw/plugin-sdk/memory-core";
import type {
  MemorySearchManager,
  MemorySearchResult,
} from "openclaw/plugin-sdk/memory-host-files";
import { runZkr, type ZkrOptions } from "./cli.ts";

const Timestamp = Type.Integer();
const Claim = Type.Object(
  {
    subject: Type.String({ minLength: 1 }),
    predicate: Type.String({ minLength: 1 }),
    value: Type.String({ minLength: 1 }),
    valid_from: Timestamp,
  },
  { additionalProperties: false },
);

function result(value: unknown) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(value) }],
    details: value,
  };
}

type RetrievalItem = {
  memory: { kind: string; id: string };
  excerpt: string;
  relevance_basis_points: number;
  evidence_ids: string[];
};

type RetrievalPack = { items: RetrievalItem[] };

export class ZkrMemoryManager implements MemorySearchManager {
  readonly #agentId: string;
  readonly #options: ZkrOptions;
  readonly #run: typeof runZkr;

  constructor(
    agentId: string,
    options: ZkrOptions,
    run: typeof runZkr = runZkr,
  ) {
    this.#agentId = agentId;
    this.#options = options;
    this.#run = run;
  }

  async search(
    query: string,
    options: {
      maxResults?: number;
      minScore?: number;
      signal?: AbortSignal;
    } = {},
  ): Promise<MemorySearchResult[]> {
    options.signal?.throwIfAborted();
    const pack = (await this.#run(
      "search",
      {
        tenant_id: this.#options.tenant ?? "openclaw",
        person_id: this.#options.person ?? this.#agentId,
        query,
        limit: options.maxResults ?? 10,
      },
      this.#options,
    )) as RetrievalPack;
    const results = pack.items
      .map((item) => {
        const path = `zkr://${item.memory.kind}/${item.memory.id}`;
        const score = item.relevance_basis_points / 10_000;
        return {
          path,
          startLine: 1,
          endLine: 1,
          score,
          snippet: item.excerpt,
          source: "memory" as const,
          citation: item.evidence_ids.join(", "),
        };
      })
      .filter((item) => item.score >= (options.minScore ?? 0));
    return results;
  }

  async readFile(params: { relPath: string; from?: number; lines?: number }) {
    if (
      (params.from !== undefined &&
        (!Number.isInteger(params.from) || params.from < 1)) ||
      (params.lines !== undefined &&
        (!Number.isInteger(params.lines) || params.lines < 1))
    ) {
      throw new Error("from and lines must be positive integers");
    }
    const match = /^zkr:\/\/(source|evidence|claim)\/([^/]+)$/u.exec(
      params.relPath,
    );
    if (!match) throw new Error("invalid zkr memory path");
    const item = (await this.#run(
      "get",
      {
        tenant_id: this.#options.tenant ?? "openclaw",
        person_id: this.#options.person ?? this.#agentId,
        target: { kind: match[1], id: match[2] },
      },
      this.#options,
    )) as RetrievalItem;
    const allLines = item.excerpt.split(/\r?\n/u);
    const from = params.from ?? 1;
    const lines = params.lines ?? 50;
    const selected = allLines.slice(from - 1, from - 1 + lines);
    const nextFrom = from + selected.length;
    return {
      text: selected.join("\n"),
      path: params.relPath,
      from,
      lines: selected.length,
      truncated: nextFrom <= allLines.length,
      ...(nextFrom <= allLines.length ? { nextFrom } : {}),
    };
  }

  status() {
    return {
      backend: "builtin" as const,
      provider: "zkr",
      dbPath: this.#options.database,
      sources: ["memory" as const],
      fts: { enabled: true, available: true },
      vector: { enabled: false, available: false },
    };
  }

  async probeEmbeddingAvailability() {
    return { ok: false, checked: true };
  }

  async probeVectorAvailability() {
    return false;
  }
}

export class ZkrMemoryHost {
  readonly #options: ZkrOptions;
  readonly #run: typeof runZkr;
  readonly #managers = new Map<string, ZkrMemoryManager>();

  constructor(options: ZkrOptions, run: typeof runZkr = runZkr) {
    this.#options = options;
    this.#run = run;
  }

  manager(agentId: string) {
    let manager = this.#managers.get(agentId);
    if (!manager) {
      manager = new ZkrMemoryManager(agentId, this.#options, this.#run);
      this.#managers.set(agentId, manager);
    }
    return manager;
  }

  runtime(): MemoryPluginRuntime {
    return {
      getMemorySearchManager: async ({ agentId }) => ({
        manager: this.manager(agentId),
      }),
      resolveMemoryBackendConfig: () => ({ backend: "builtin" }),
      closeMemorySearchManager: async ({ agentId }) => {
        this.#managers.delete(agentId);
      },
      closeAllMemorySearchManagers: async () => {
        this.#managers.clear();
      },
    };
  }
}

function nativeMemoryTools(manager: ZkrMemoryManager) {
  return [
    {
      name: "memory_search",
      label: "Memory Search",
      description: "Search cited zkr memory.",
      parameters: Type.Object(
        {
          query: Type.String({ minLength: 1 }),
          maxResults: Type.Optional(Type.Integer({ minimum: 1, maximum: 100 })),
          minScore: Type.Optional(Type.Number({ minimum: 0, maximum: 1 })),
        },
        { additionalProperties: false },
      ),
      async execute(
        _id: string,
        params: { query: string; maxResults?: number; minScore?: number },
      ) {
        return result(await manager.search(params.query, params));
      },
    },
    {
      name: "memory_get",
      label: "Memory Get",
      description: "Read an exact zkr memory returned by memory_search.",
      parameters: Type.Object(
        {
          path: Type.String({ minLength: 1 }),
          from: Type.Optional(Type.Integer({ minimum: 1 })),
          lines: Type.Optional(Type.Integer({ minimum: 1 })),
        },
        { additionalProperties: false },
      ),
      async execute(
        _id: string,
        params: { path: string; from?: number; lines?: number },
      ) {
        return result(
          await manager.readFile({
            relPath: params.path,
            from: params.from,
            lines: params.lines,
          }),
        );
      },
    },
  ];
}

export function tools(
  options: ZkrOptions,
  run: typeof runZkr = runZkr,
  agentId = "default",
) {
  const scoped = (params: unknown) => ({
    ...(params as Record<string, unknown>),
    tenant_id: options.tenant ?? "openclaw",
    person_id: options.person ?? agentId,
  });
  return [
    {
      name: "zkr_store",
      label: "Store zkr memory",
      description:
        "Store source evidence and an optional temporal claim in zkr memory",
      parameters: Type.Object(
        {
          kind: Type.Union([
            Type.Literal("conversation"),
            Type.Literal("screen"),
            Type.Literal("audio"),
            Type.Literal("document"),
            Type.Literal("integration"),
            Type.Literal("user_correction"),
          ]),
          text: Type.String({ minLength: 1 }),
          captured_at: Timestamp,
          claim: Type.Optional(Claim),
        },
        { additionalProperties: false },
      ),
      async execute(_id: string, params: unknown) {
        return result(await run("remember", scoped(params), options));
      },
    },
    {
      name: "zkr_search",
      label: "Search zkr memory",
      description:
        "Search zkr memory and return bounded results with evidence citations",
      parameters: Type.Object(
        {
          query: Type.String({ minLength: 1 }),
          limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 100 })),
        },
        { additionalProperties: false },
      ),
      async execute(_id: string, params: unknown) {
        return result(await run("search", scoped(params), options));
      },
    },
    {
      name: "zkr_correct",
      label: "Correct zkr memory",
      description:
        "Correct an accepted zkr claim while retaining its prior evidence and history",
      parameters: Type.Object(
        {
          claim_id: Type.String({ minLength: 1 }),
          text: Type.String({ minLength: 1 }),
          value: Type.String({ minLength: 1 }),
          occurred_at: Timestamp,
        },
        { additionalProperties: false },
      ),
      async execute(_id: string, params: unknown) {
        return result(await run("correct", scoped(params), options));
      },
    },
    {
      name: "zkr_delete",
      label: "Delete zkr source",
      description:
        "Tombstone a zkr source and remove its evidence from retrieval",
      parameters: Type.Object(
        {
          source_id: Type.String({ minLength: 1 }),
          deleted_at: Timestamp,
        },
        { additionalProperties: false },
      ),
      async execute(_id: string, params: unknown) {
        return result(await run("delete", scoped(params), options));
      },
    },
    {
      name: "zkr_reflect",
      label: "Reflect into zkr memory",
      description:
        "Save a cited daily reflection in zkr without changing source evidence",
      parameters: Type.Object(
        {
          day: Type.String({ minLength: 1 }),
          summary: Type.String({ minLength: 1 }),
          evidence_ids: Type.Array(Type.String({ minLength: 1 })),
          recorded_at: Timestamp,
        },
        { additionalProperties: false },
      ),
      async execute(_id: string, params: unknown) {
        return result(await run("review", scoped(params), options));
      },
    },
  ];
}

export const zkrPlugin = definePluginEntry({
  id: "zkr",
  name: "zkr Memory",
  description: "Evidence-backed temporal memory tools powered by zkr",
  register(api) {
    const options = api.pluginConfig as ZkrOptions | undefined;
    const host = new ZkrMemoryHost(options ?? {});
    api.registerMemoryCapability({
      promptBuilder: ({ availableTools }) =>
        availableTools.has("memory_search")
          ? [
              "Search zkr memory before answering questions about prior user context.",
            ]
          : [],
      runtime: host.runtime(),
    });
    api.registerTool(
      (context) =>
        nativeMemoryTools(host.manager(context.agentId ?? "default")),
      { names: ["memory_search", "memory_get"] },
    );
    api.registerTool(
      (context) => tools(options ?? {}, runZkr, context.agentId ?? "default"),
      {
        names: [
          "zkr_store",
          "zkr_search",
          "zkr_correct",
          "zkr_delete",
          "zkr_reflect",
        ],
      },
    );
  },
});

export default zkrPlugin;
