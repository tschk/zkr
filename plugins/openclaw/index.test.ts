import { describe, expect, test } from "bun:test";
import { runZkr, ZKR_COMMAND_FAILED } from "./cli.ts";
import { tools, ZkrMemoryHost, zkrPlugin } from "./index.ts";

describe("zkr OpenClaw tools", () => {
  test("redacts local CLI failures", async () => {
    const command = "zkr-command-that-must-not-exist";
    const failure = await runZkr(
      "search",
      { tenant_id: "tenant", person_id: "person", query: "memory" },
      { command },
    ).catch((error: unknown) => error);

    expect(String(failure)).toBe(`Error: ${ZKR_COMMAND_FAILED}`);
    expect(String(failure)).not.toContain(command);
  });

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

  test("rejects invalid runtime read windows", async () => {
    const manager = new ZkrMemoryHost({}, async () => ({}) as never).manager(
      "agent-1",
    );
    await expect(
      manager.readFile({ relPath: "zkr://source/s-1", from: 0 }),
    ).rejects.toThrow("positive integers");
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
