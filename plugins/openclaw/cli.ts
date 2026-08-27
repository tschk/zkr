import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, join } from "node:path";

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

export function resolveZkrExecutable(command: string | undefined): string {
  const executable = command ?? "zkr";
  if (typeof executable !== "string" || executable.includes("\0")) {
    throw new Error(ZKR_COMMAND_FAILED);
  }
  const name = basename(executable);
  const candidate = process.platform === "win32" ? name.toLowerCase() : name;
  if (candidate !== "zkr" && candidate !== "zkr.exe") {
    throw new Error(ZKR_COMMAND_FAILED);
  }
  return executable;
}

export async function runZkr(
  operation: ZkrCommand,
  input: unknown,
  options: ZkrOptions = {},
): Promise<unknown> {
  if (options.command !== undefined && typeof options.command !== "string") {
    throw new Error("zkr command must be a string");
  }
  if (options.database !== undefined && typeof options.database !== "string") {
    throw new Error("zkr database must be a string");
  }

  const executable = resolveZkrExecutable(options.command);
  const database = options.database ?? join(homedir(), ".zkr", "memory.db");

  if (database.includes("\0")) {
    throw new Error("zkr database path must not contain null bytes");
  }

  if (database.startsWith("-")) {
    throw new Error("zkr database path must not start with a hyphen");
  }
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
    child.stdin.on("error", () => {
      // Ignore EPIPE errors which occur when the child process closes its stdin before we finish writing
    });
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
