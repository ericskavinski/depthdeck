import { existsSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(webRoot, "..");
const output = path.join(webRoot, "public/demo.ddt");

if (!existsSync(output)) {
  await mkdir(path.dirname(output), { recursive: true });
  const result = spawnSync(
    "cargo",
    [
      "run",
      "--quiet",
      "-p",
      "depthdeck",
      "--",
      "generate-demo",
      output,
      "--duration",
      "90",
      "--rate",
      "100",
    ],
    { cwd: repoRoot, stdio: "inherit", shell: false },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
