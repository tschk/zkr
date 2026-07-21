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
    const stdout: Buffer[] = [];
    const timeout = setTimeout(() => child.kill(), 30_000);

    child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
    child.stderr.resume();
    child.on("error", () => {
      clearTimeout(timeout);
      reject(new Error(ZKR_COMMAND_FAILED));
    });
    child.on("close", (code) => {
      clearTimeout(timeout);
      const output = Buffer.concat(stdout).toString("utf8");
      if (code !== 0) {
        reject(new Error(ZKR_COMMAND_FAILED));
        return;
      }
      try {
        resolve(JSON.parse(output));
      } catch {
        reject(new Error(ZKR_COMMAND_FAILED));
      }
    });
    child.stdin.end(JSON.stringify(input));
  });
}
