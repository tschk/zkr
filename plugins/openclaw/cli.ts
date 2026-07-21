import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

export type ZkrCommand =
  | "remember"
  | "search"
  | "get"
  | "correct"
  | "delete"
  | "review";

export type ZkrOptions = {
  command?: string;
  database?: string;
  tenant?: string;
  person?: string;
};

export const ZKR_COMMAND_FAILED = "zkr command failed";
const MAX_ZKR_OUTPUT_BYTES = 1024 * 1024;

export async function runZkr(
  operation: ZkrCommand,
  input: unknown,
  options: ZkrOptions = {},
): Promise<unknown> {
  const executable = options.command ?? "zkr";
  const database = options.database ?? join(homedir(), ".zkr", "memory.db");
  mkdirSync(dirname(database), { recursive: true });

  return new Promise((resolve, reject) => {
    const child = spawn(executable, ["--db", database, operation], {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    const output = { stdout: [] as Buffer[], stderr: [] as Buffer[] };
    let outputBytes = 0;
    let settled = false;
    const timeout = setTimeout(() => fail(), 30_000);
    const fail = (kill = true) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      if (kill) child.kill();
      reject(new Error(ZKR_COMMAND_FAILED));
    };
    const capture = (stream: Buffer[], chunk: Buffer) => {
      if (settled) return;
      outputBytes += chunk.length;
      if (outputBytes > MAX_ZKR_OUTPUT_BYTES) {
        fail();
        return;
      }
      stream.push(chunk);
    };

    child.stdout.on("data", (chunk: Buffer) => capture(output.stdout, chunk));
    child.stderr.on("data", (chunk: Buffer) => capture(output.stderr, chunk));
    child.on("error", () => {
      fail(false);
    });
    child.on("close", (code) => {
      if (settled) return;
      clearTimeout(timeout);
      if (code !== 0) {
        fail(false);
        return;
      }
      try {
        const parsed = JSON.parse(
          Buffer.concat(output.stdout).toString("utf8"),
        );
        settled = true;
        resolve(parsed);
      } catch {
        fail(false);
      }
    });
    child.stdin.end(JSON.stringify(input));
  });
}
