/**
 * Import smoke test for the probe matrix: does each generated TypeScript
 * package *evaluate*?
 *
 * `tsc --noEmit` is necessary but not sufficient — a pinned `pattern` that is
 * legal for Rust's `regex` and illegal for ECMA-262-with-`u` type-checks fine
 * and throws `SyntaxError` from `new RegExp` at module import. Only running the
 * module finds it.
 */
import { writeFileSync } from "node:fs";
import { test } from "vitest";
// eslint-disable-next-line import/no-unresolved -- written by the Rust driver
import { REGISTRY } from "./registry";

test("generated modules import", async () => {
  const results: Record<string, string> = {};
  for (const [id, entry] of Object.entries(REGISTRY)) {
    try {
      await entry.load();
      results[id] = "ok";
    } catch (error) {
      results[id] = `${(error as Error).name}: ${(error as Error).message}`;
    }
  }
  writeFileSync(
    process.env.NEXGEN_CONFORMANCE_RESULT!,
    JSON.stringify(results, null, 1),
  );
});
