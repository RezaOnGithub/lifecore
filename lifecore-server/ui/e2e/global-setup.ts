// Builds the `lifecore` binary (unless LIFECORE_BIN names one), initializes a
// scratch database, starts `lifecore serve` on it, and hands the URL it prints
// on standard output, launch token included, to the tests. The returned
// function stops the server and removes the database.
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const repository = fileURLToPath(new URL("../../../", import.meta.url));

function build(): string {
  const output = execFileSync(
    "cargo",
    ["build", "-p", "lifecore-cli", "--message-format=json-render-diagnostics"],
    { cwd: repository, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
  );
  for (const line of output.split("\n")) {
    if (!line.startsWith("{")) continue;
    const message = JSON.parse(line);
    if (
      message.reason === "compiler-artifact" &&
      message.target?.name === "lifecore" &&
      message.executable
    ) {
      return message.executable;
    }
  }
  throw new Error("cargo build did not report the lifecore executable");
}

function printedUrl(server: ChildProcess): Promise<string> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error("lifecore serve printed no URL within 30 s")),
      30_000,
    );
    if (server.stdout === null) {
      reject(new Error("lifecore serve has no standard output"));
      return;
    }
    createInterface({ input: server.stdout }).once("line", (line) => {
      clearTimeout(timer);
      resolve(line.trim());
    });
    server.once("exit", (code) => {
      clearTimeout(timer);
      reject(
        new Error(`lifecore serve exited with ${code} before printing a URL`),
      );
    });
  });
}

export default async function globalSetup() {
  const binary = process.env.LIFECORE_BIN ?? build();
  const scratch = mkdtempSync(path.join(tmpdir(), "lifecore-e2e-"));
  const env = {
    ...process.env,
    LIFECORE_STATE_FILE_PATH: path.join(scratch, "LifeCoreState.json"),
  };
  const initialized = JSON.parse(
    execFileSync(binary, ["init"], { env, encoding: "utf8" }),
  );
  const server = spawn(binary, ["serve"], {
    env,
    stdio: ["ignore", "pipe", "inherit"],
  });
  const url = await printedUrl(server);
  if (!/^http:\/\/127\.0\.0\.1:\d+\/#token=[0-9a-f]{64}$/.test(url)) {
    server.kill("SIGTERM");
    throw new Error(`unexpected URL from lifecore serve: ${url}`);
  }
  process.env.LIFECORE_URL = url;
  process.env.LIFECORE_DATABASE = initialized.database;
  return async () => {
    const exited = new Promise((resolve) => server.once("exit", resolve));
    server.kill("SIGTERM");
    await exited;
    rmSync(scratch, { recursive: true, force: true });
  };
}
