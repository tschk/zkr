import { Type } from "typebox";
import { definePluginEntry } from "openclaw/plugin-sdk/plugin-entry";
import { runZkr, type ZkrOptions } from "./cli.ts";

const Scope = {
  tenant_id: Type.String({ minLength: 1 }),
  person_id: Type.String({ minLength: 1 }),
};

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

export function tools(options: ZkrOptions, run: typeof runZkr = runZkr) {
  return [
    {
      name: "zkr_store",
      label: "Store zkr memory",
      description:
        "Store source evidence and an optional temporal claim in zkr memory",
      parameters: Type.Object(
        {
          ...Scope,
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
        return result(await run("remember", params, options));
      },
    },
    {
      name: "zkr_search",
      label: "Search zkr memory",
      description:
        "Search zkr memory and return bounded results with evidence citations",
      parameters: Type.Object(
        {
          ...Scope,
          query: Type.String({ minLength: 1 }),
          limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 100 })),
        },
        { additionalProperties: false },
      ),
      async execute(_id: string, params: unknown) {
        return result(await run("search", params, options));
      },
    },
    {
      name: "zkr_correct",
      label: "Correct zkr memory",
      description:
        "Correct an accepted zkr claim while retaining its prior evidence and history",
      parameters: Type.Object(
        {
          ...Scope,
          claim_id: Type.String({ minLength: 1 }),
          text: Type.String({ minLength: 1 }),
          value: Type.String({ minLength: 1 }),
          occurred_at: Timestamp,
        },
        { additionalProperties: false },
      ),
      async execute(_id: string, params: unknown) {
        return result(await run("correct", params, options));
      },
    },
    {
      name: "zkr_delete",
      label: "Delete zkr source",
      description:
        "Tombstone a zkr source and remove its evidence from retrieval",
      parameters: Type.Object(
        {
          ...Scope,
          source_id: Type.String({ minLength: 1 }),
          deleted_at: Timestamp,
        },
        { additionalProperties: false },
      ),
      async execute(_id: string, params: unknown) {
        return result(await run("delete", params, options));
      },
    },
    {
      name: "zkr_reflect",
      label: "Reflect into zkr memory",
      description:
        "Save a cited daily reflection in zkr without changing source evidence",
      parameters: Type.Object(
        {
          ...Scope,
          day: Type.String({ minLength: 1 }),
          summary: Type.String({ minLength: 1 }),
          evidence_ids: Type.Array(Type.String({ minLength: 1 })),
          recorded_at: Timestamp,
        },
        { additionalProperties: false },
      ),
      async execute(_id: string, params: unknown) {
        return result(await run("review", params, options));
      },
    },
  ];
}

export default definePluginEntry({
  id: "zkr",
  name: "zkr Memory",
  description: "Evidence-backed temporal memory tools powered by zkr",
  kind: "memory",
  register(api) {
    const options = api.pluginConfig as ZkrOptions | undefined;
    for (const tool of tools(options ?? {})) api.registerTool(tool);
  },
});
