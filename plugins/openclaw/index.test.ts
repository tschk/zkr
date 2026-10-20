import { describe, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { resolveZkrExecutable, runZkr, ZKR_COMMAND_FAILED } from "./cli.ts";
import { tools, ZkrMemoryHost, zkrPlugin } from "./index.ts";

async function withTempZkr(
  script: string | null,
  run: (command: string) => Promise<void>,
) {
  const directory = mkdtempSync(join(tmpdir(), "zkr-openclaw-"));
  const command = join(directory, "zkr");
  try {
    if (script !== null) {
      writeFileSync(command, script);
      chmodSync(command, 0o700);
    }
    await run(command);
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
}

describe("zkr OpenClaw tools", () => {
  test("allows PATH and custom zkr install locations", () => {
    expect(resolveZkrExecutable(undefined)).toBe("zkr");
    expect(resolveZkrExecutable("zkr")).toBe("zkr");
    expect(resolveZkrExecutable("zkr.exe")).toBe("zkr.exe");
    expect(resolveZkrExecutable("/usr/local/bin/zkr")).toBe(
      "/usr/local/bin/zkr",
    );
    expect(resolveZkrExecutable("./target/release/zkr")).toBe(
      "./target/release/zkr",
    );
    expect(resolveZkrExecutable("~/my-dir/zkr")).toBe("~/my-dir/zkr");
  });

  test("rejects executables with spaces or shell metacharacters", () => {
    expect(() => resolveZkrExecutable("echo vulnerable && zkr")).toThrow(
      ZKR_COMMAND_FAILED,
    );
    expect(() => resolveZkrExecutable("sh -c 'malicious' # /zkr")).toThrow(
      ZKR_COMMAND_FAILED,
    );
    expect(() => resolveZkrExecutable("/usr/bin/zkr; rm -rf /")).toThrow(
      ZKR_COMMAND_FAILED,
    );
    expect(() => resolveZkrExecutable("zkr --help")).toThrow(
      ZKR_COMMAND_FAILED,
    );
    expect(() => resolveZkrExecutable("zkr\n")).toThrow(ZKR_COMMAND_FAILED);
    expect(() => resolveZkrExecutable("zkr$(echo)")).toThrow(
      ZKR_COMMAND_FAILED,
    );
  });

  test("rejects non-zkr executables without leaking the command", async () => {
    const rejected = [
      "echo",
      "false",
      "python",
      "node",
      "/usr/bin/python",
      "zkr-command-that-must-not-exist",
      join(tmpdir(), "not-zkr"),
      "zkr\0python",
    ];
    for (const command of rejected) {
      const failure = await runZkr("search", {}, { command }).catch(
        (error: unknown) => error,
      );
      expect(String(failure)).toBe(`Error: ${ZKR_COMMAND_FAILED}`);
      expect(String(failure)).not.toContain(command.replaceAll("\0", ""));
    }
  });

  test("redacts local CLI failures", async () => {
    await withTempZkr(null, async (command) => {
      const failure = await runZkr(
        "search",
        { tenant_id: "tenant", person_id: "person", query: "memory" },
        { command },
      ).catch((error: unknown) => error);

      expect(String(failure)).toBe(`Error: ${ZKR_COMMAND_FAILED}`);
      expect(String(failure)).not.toContain(command);
    });
  });

  test("rejects malformed successful CLI output", async () => {
    await withTempZkr(
      `#!/usr/bin/env bun\nconsole.log("invalid json");\n`,
      async (command) => {
        await expect(runZkr("search", {}, { command })).rejects.toThrow(
          ZKR_COMMAND_FAILED,
        );
      },
    );
  });

  test("rejects CLI failure with non-zero exit code", async () => {
    await withTempZkr(
      `#!/usr/bin/env bun\nprocess.exit(1);\n`,
      async (command) => {
        await expect(runZkr("search", {}, { command })).rejects.toThrow(
          ZKR_COMMAND_FAILED,
        );
      },
    );
  });

  for (const stream of ["stdout", "stderr"] as const) {
    test(`rejects oversized ${stream} without leaking it`, async () => {
      const marker = `secret-${stream}-output`;
      await withTempZkr(
        `#!/usr/bin/env bun\nprocess.${stream}.write(${JSON.stringify(marker)}.repeat(65536));\n`,
        async (command) => {
          const failure = await runZkr("search", {}, { command }).catch(
            (error: unknown) => error,
          );

          expect(String(failure)).toBe(`Error: ${ZKR_COMMAND_FAILED}`);
          expect(String(failure)).not.toContain(marker);
        },
      );
    });
  }

  test("maps native tools to the zkr CLI contract", async () => {
    const calls: unknown[][] = [];
    const run = async (...args: unknown[]) => {
      calls.push(args);
      return { ok: true };
    };
    const registered = tools(
      { database: "test.db", tenant: "tenant", person: "person" },
      run as never,
    );
    const search = registered.find((tool) => tool.name === "zkr_search");

    expect(search).toBeDefined();
    await search!.execute("call-1", {
      tenant_id: "other-tenant",
      person_id: "other-person",
      query: "favorite editor",
    });
    expect(calls).toEqual([
      [
        "search",
        {
          tenant_id: "tenant",
          person_id: "person",
          query: "favorite editor",
        },
        { database: "test.db", tenant: "tenant", person: "person" },
      ],
    ]);
    expect(
      (search!.parameters as { properties: object }).properties,
    ).not.toHaveProperty("tenant_id");
  });

  test("requires explicit bitemporal fields in write contracts", () => {
    const registered = tools({});
    const store = registered.find((tool) => tool.name === "zkr_store");
    const correct = registered.find((tool) => tool.name === "zkr_correct");
    const search = registered.find((tool) => tool.name === "zkr_search");

    expect((store!.parameters as { required: string[] }).required).toEqual(
      expect.arrayContaining(["captured_at", "recorded_at"]),
    );
    expect((correct!.parameters as { required: string[] }).required).toEqual(
      expect.arrayContaining(["valid_at", "recorded_at"]),
    );
    expect(
      (search!.parameters as { properties: object }).properties,
    ).toHaveProperty("as_of");
  });

  test("maps project memory, structured record, and dream tools to the zkr CLI contract", async () => {
    const calls: unknown[][] = [];
    const run = async (...args: unknown[]) => {
      calls.push(args);
      return { ok: true };
    };
    const options = { database: "test.db", tenant: "tenant", person: "person" };
    const registered = tools(options, run as never);
    const rememberProject = registered.find(
      (tool) => tool.name === "zkr_remember_project",
    );
    const rememberProjectRecord = registered.find(
      (tool) => tool.name === "zkr_remember_project_record",
    );
    const projectMemories = registered.find(
      (tool) => tool.name === "zkr_project_memories",
    );
    const dream = registered.find((tool) => tool.name === "zkr_dream");

    expect(rememberProject).toBeDefined();
    expect(rememberProjectRecord).toBeDefined();
    expect(projectMemories).toBeDefined();
    expect(dream).toBeDefined();

    await rememberProject!.execute("call-1", {
      project: "Tofu",
      category: "work",
      categories: ["work", "launch"],
      kind: "conversation",
      text: "Launch status: planned",
      captured_at: 10,
      recorded_at: 10,
      claim: {
        predicate: "task:launch:status",
        value: "planned",
        kind: "task",
        valid_from: 10,
      },
      embedding: {
        vector: [0.1, 0.2],
        model: "nomic-embed",
        version: "v1",
        normalization: "l2",
        distance: "cosine",
      },
    });
    await rememberProjectRecord!.execute("call-2", {
      project: "Tofu",
      categories: ["work", "roadmap"],
      kind: "conversation",
      text: "Launch work item",
      captured_at: 10,
      recorded_at: 10,
      claim: {
        predicate: "record:work_item:launch",
        kind: "task",
        valid_from: 10,
      },
      record: {
        record_type: "work_item",
        categories: ["milestone"],
        fields: { title: "Launch", priority: 1 },
        connections: [
          {
            relation: "blocked_by",
            target: "cupboard:pairing",
            weight_basis_points: 9000,
            note: "Requires cupboard pairing first",
          },
        ],
      },
      embedding: {
        vector: [0.3, 0.4],
      },
    });
    await projectMemories!.execute("call-3", {
      project: "Tofu",
      projects: ["Tofu", "Cupboard"],
      category: "work",
      categories: ["work", "roadmap"],
      all_categories: ["work"],
      predicate: "record:work_item:launch",
      record_type: "work_item",
      relation: "blocked_by",
      target: "cupboard:pairing",
      where_fields: { priority: 1 },
      connected_to: "cupboard:pairing",
      max_hops: 2,
      vector_query: "launch work item",
      query_embedding: {
        vector: [0.3, 0.4],
        model: "nomic-embed",
        version: "v1",
      },
      limit: 20,
    });
    await dream!.execute("call-4", {
      model: "nomic-embed",
      version: "v1",
      min_similarity_basis_points: 7500,
      limit: 15,
    });

    expect(
      (rememberProject!.parameters as { properties: object }).properties,
    ).toHaveProperty("categories");
    expect(
      (rememberProject!.parameters as { properties: object }).properties,
    ).toHaveProperty("embedding");
    expect(
      (projectMemories!.parameters as { properties: object }).properties,
    ).toMatchObject({
      projects: expect.any(Object),
      categories: expect.any(Object),
      all_categories: expect.any(Object),
      where_fields: expect.any(Object),
      connected_to: expect.any(Object),
      max_hops: expect.any(Object),
      vector_query: expect.any(Object),
      query_embedding: expect.any(Object),
    });

    expect(calls).toEqual([
      [
        "remember-project",
        {
          tenant_id: "tenant",
          person_id: "person",
          project: "Tofu",
          category: "work",
          categories: ["work", "launch"],
          kind: "conversation",
          text: "Launch status: planned",
          captured_at: 10,
          recorded_at: 10,
          claim: {
            predicate: "task:launch:status",
            value: "planned",
            kind: "task",
            valid_from: 10,
          },
          embedding: {
            vector: [0.1, 0.2],
            model: "nomic-embed",
            version: "v1",
            normalization: "l2",
            distance: "cosine",
          },
        },
        options,
      ],
      [
        "remember-project-record",
        {
          tenant_id: "tenant",
          person_id: "person",
          project: "Tofu",
          categories: ["work", "roadmap"],
          kind: "conversation",
          text: "Launch work item",
          captured_at: 10,
          recorded_at: 10,
          claim: {
            predicate: "record:work_item:launch",
            kind: "task",
            valid_from: 10,
          },
          record: {
            record_type: "work_item",
            categories: ["milestone"],
            fields: { title: "Launch", priority: 1 },
            connections: [
              {
                relation: "blocked_by",
                target: "cupboard:pairing",
                weight_basis_points: 9000,
                note: "Requires cupboard pairing first",
              },
            ],
          },
          embedding: {
            vector: [0.3, 0.4],
          },
        },
        options,
      ],
      [
        "project-memories",
        {
          tenant_id: "tenant",
          person_id: "person",
          project: "Tofu",
          projects: ["Tofu", "Cupboard"],
          category: "work",
          categories: ["work", "roadmap"],
          all_categories: ["work"],
          predicate: "record:work_item:launch",
          record_type: "work_item",
          relation: "blocked_by",
          target: "cupboard:pairing",
          where_fields: { priority: 1 },
          connected_to: "cupboard:pairing",
          max_hops: 2,
          vector_query: "launch work item",
          query_embedding: {
            vector: [0.3, 0.4],
            model: "nomic-embed",
            version: "v1",
          },
          limit: 20,
        },
        options,
      ],
      [
        "dream",
        {
          tenant_id: "tenant",
          person_id: "person",
          model: "nomic-embed",
          version: "v1",
          min_similarity_basis_points: 7500,
          limit: 15,
        },
        options,
      ],
    ]);
  });

  test("rejects invalid runtime read windows", async () => {
    const manager = new ZkrMemoryHost({}, async () => ({}) as never).manager(
      "agent-1",
    );
    await expect(
      manager.readFile({ relPath: "zkr://source/s-1", from: 0 }),
    ).rejects.toThrow("positive integers");
  });

  test("rejects invalid zkr memory paths", async () => {
    const manager = new ZkrMemoryHost({}, async () => ({}) as never).manager(
      "agent-1",
    );
    await expect(manager.readFile({ relPath: "invalid-path" })).rejects.toThrow(
      "invalid zkr memory path",
    );
    await expect(
      manager.readFile({ relPath: "zkr://unknown/123" }),
    ).rejects.toThrow("invalid zkr memory path");
  });

  test("maps exact reads to the zkr CLI contract", async () => {
    const calls: unknown[][] = [];
    const run = async (...args: unknown[]) => {
      calls.push(args);
      return { excerpt: "line1\nline2\nline3" };
    };
    const manager = new ZkrMemoryHost(
      { tenant: "tenant", person: "person", database: "test.db" },
      run as never,
    ).manager("agent-1");

    await manager.readFile({ relPath: "zkr://claim/c-1" });

    expect(calls).toEqual([
      [
        "get",
        {
          tenant_id: "tenant",
          person_id: "person",
          target: { kind: "claim", id: "c-1" },
        },
        { tenant: "tenant", person: "person", database: "test.db" },
      ],
    ]);
  });

  test("slices exact reads using from and lines with pagination logic", async () => {
    const run = async () => ({ excerpt: "line1\nline2\nline3\nline4\nline5" });
    const manager = new ZkrMemoryHost({}, run as never).manager("agent-1");

    // test default behaviour (from 1, lines 50, all fit)
    const resultAll = await manager.readFile({ relPath: "zkr://source/s-1" });
    expect(resultAll).toMatchObject({
      text: "line1\nline2\nline3\nline4\nline5",
      path: "zkr://source/s-1",
      from: 1,
      lines: 5,
      truncated: false,
    });
    expect(resultAll).not.toHaveProperty("nextFrom");

    // test specific slice that hits the end
    const resultEnd = await manager.readFile({
      relPath: "zkr://source/s-1",
      from: 4,
      lines: 2,
    });
    expect(resultEnd).toMatchObject({
      text: "line4\nline5",
      from: 4,
      lines: 2,
      truncated: false,
    });
    expect(resultEnd).not.toHaveProperty("nextFrom");

    // test truncation slice
    const resultTruncated = await manager.readFile({
      relPath: "zkr://source/s-1",
      from: 2,
      lines: 2,
    });
    expect(resultTruncated).toMatchObject({
      text: "line2\nline3",
      from: 2,
      lines: 2,
      truncated: true,
      nextFrom: 4,
    });
  });
});

describe("zkr OpenClaw memory capability", () => {
  test("implements native search and exact reads over the CLI", async () => {
    const item = {
      memory: { kind: "claim", id: "c-1" },
      excerpt: "User prefers concise reports",
      relevance_basis_points: 9000,
      evidence_ids: ["e-1"],
    };
    const run = async (operation: string) =>
      operation === "get" ? item : { items: [item] };
    const manager = new ZkrMemoryHost(
      { database: "test.db" },
      run as never,
    ).manager("agent-1");
    const matches = await manager.search("reports", { minScore: 0.5 });
    const exact = await manager.readFile({ relPath: matches[0]!.path });

    expect(matches[0]).toMatchObject({
      path: "zkr://claim/c-1",
      score: 0.9,
      citation: "e-1",
    });
    expect(exact.text).toBe("User prefers concise reports");
  });

  test("registers the current exclusive memory capability", () => {
    let capability: unknown;
    const toolNames: string[][] = [];
    zkrPlugin.register({
      pluginConfig: {},
      registerMemoryCapability(value: unknown) {
        capability = value;
      },
      registerTool(_tool: unknown, options?: { names?: string[] }) {
        if (options?.names) toolNames.push(options.names);
      },
    } as never);

    expect(capability).toBeDefined();
    expect(toolNames).toContainEqual(["memory_search", "memory_get"]);
  });

  test("scopes explicit tools to the active agent", async () => {
    const calls: unknown[][] = [];
    const run = async (...args: unknown[]) => {
      calls.push(args);
      return { items: [] };
    };
    const agentA = tools({}, run as never, "agent-a");
    const agentB = tools({}, run as never, "agent-b");
    const searchA = agentA.find((tool) => tool.name === "zkr_search");
    const searchB = agentB.find((tool) => tool.name === "zkr_search");
    await searchA!.execute("a", { query: "memory" });
    await searchB!.execute("b", { query: "memory" });
    expect(
      calls.map((call) => (call[1] as { person_id: string }).person_id),
    ).toEqual(["agent-a", "agent-b"]);
  });

  test("does not expose CLI failures through memory tools", async () => {
    const search = tools(
      {},
      (async () => {
        throw new Error(ZKR_COMMAND_FAILED);
      }) as never,
      "agent-1",
    ).find((tool) => tool.name === "zkr_search");

    await expect(search!.execute("call", { query: "memory" })).rejects.toThrow(
      ZKR_COMMAND_FAILED,
    );
  });

  test("keeps the manifest memory ownership and tool contracts aligned", async () => {
    const manifest = (await Bun.file(
      new URL("./openclaw.plugin.json", import.meta.url),
    ).json()) as {
      id: string;
      kind: string;
      contracts: { tools: string[] };
    };
    const registered = new Set<string>();
    zkrPlugin.register({
      pluginConfig: {},
      registerMemoryCapability() {},
      registerTool(
        tool: { name?: string } | ((context: object) => { name: string }[]),
        options?: { names?: string[] },
      ) {
        for (const name of options?.names ?? []) registered.add(name);
        if (typeof tool !== "function" && tool.name) registered.add(tool.name);
      },
    } as never);

    expect(manifest.id).toBe("zkr");
    expect(manifest.kind).toBe("memory");
    expect([...registered].sort()).toEqual(
      [...manifest.contracts.tools].sort(),
    );
  });
});
