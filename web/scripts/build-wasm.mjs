import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(webRoot, "..");

run("cargo", [
  "build",
  "-p",
  "depthdeck-wasm",
  "--release",
  "--target",
  "wasm32-unknown-unknown",
]);
run("wasm-bindgen", [
  path.join(repoRoot, "target/wasm32-unknown-unknown/release/depthdeck_wasm.wasm"),
  "--target",
  "web",
  "--out-dir",
  path.join(webRoot, "src/wasm"),
]);

function run(command, args) {
  const result = spawnSync(command, args, { cwd: repoRoot, stdio: "inherit", shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
