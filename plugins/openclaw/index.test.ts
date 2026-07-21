import { describe, expect, test } from "bun:test";
import { tools } from "./index.ts";

describe("zkr OpenClaw tools", () => {
  test("maps native tools to the zkr CLI contract", async () => {
    const calls: unknown[][] = [];
    const run = async (...args: unknown[]) => {
      calls.push(args);
      return { ok: true };
    };
    const registered = tools({ database: "test.db" }, run as never);
    const search = registered.find((tool) => tool.name === "zkr_search");

    expect(search).toBeDefined();
    await search!.execute("call-1", {
      tenant_id: "tenant",
      person_id: "person",
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
        { database: "test.db" },
      ],
    ]);
  });
});
