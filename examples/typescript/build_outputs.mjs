import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

function main() {
  const appRoot = dirname(fileURLToPath(import.meta.url));
  const repoRoot = resolve(appRoot, "..", "..");
  const command = generatorCommand();
  const result = spawnSync(
    command[0],
    [...command.slice(1), ...process.argv.slice(2)],
    {
      cwd: repoRoot,
      stdio: "inherit",
    },
  );

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function generatorCommand() {
  if (process.env.NEXUS_API_GEN_BIN) {
    return [process.env.NEXUS_API_GEN_BIN, "build-examples", "--lang", "typescript"];
  }

  return ["cargo", "build-examples", "--lang", "typescript"];
}

main();
