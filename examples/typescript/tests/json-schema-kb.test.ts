import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import * as nexus from "nexus-rpc";

import {
  BlockMapper,
  CategoryMapper,
  GetCategoryTreeInputMapper,
  GetPageInputMapper,
  PageMapper,
  PutBlockOutputMapper,
} from "../json_schema/definitions/kb/index.ts";
import {
  knowledgeBaseService,
  type Category,
  type Page,
  type PutBlockOutput,
} from "../json_schema/api/kb/index.ts";
import { executeWorkflowWithNexus, withWorkflowEnvironment } from "./helpers.ts";
import {
  fixtureBytes,
  loadFixture as loadFixtureFrom,
  roundTripFixture,
  type IntermediateMapper,
} from "./json-converter-helper.ts";

const wireFixtureDir = new URL("../../wire/json_schema/kb/", import.meta.url);
const workflowsPath = fileURLToPath(
  new URL("./workflows/json-schema-kb.ts", import.meta.url),
);

function loadFixture<T = unknown>(name: string): T {
  return loadFixtureFrom<T>(wireFixtureDir, name);
}

// Round-trip a fixture through the Temporal data converter (driven by the
// generated mapper) and assert the re-serialized JSON is JSON-equal to the
// fixture. TS mappers preserve explicit nulls, so all KB fixtures use exact
// JSON-equality (no optional+nullable collapse — unlike Go).
function expectRoundTrip<T>(name: string, mapper: IntermediateMapper<T>): T {
  const { value, serialized } = roundTripFixture(
    mapper,
    fixtureBytes(wireFixtureDir, name),
  );
  expect(serialized).toEqual(loadFixture(name));
  return value;
}

describe("json-schema KB generated output", () => {
  test("roundtrips multi-file KB fixtures through the Temporal converter", () => {
    const page = expectRoundTrip("page.json", new PageMapper());
    expect(page.pageId).toBe("page-1");
    expect(page.blocks?.[0]?.blockId).toBe("block-1");
    expect(page.blocks?.[0]?.page).toBeNull();
    expect(page.blocks?.[0]?.style?.bold).toBe(true);

    const block = expectRoundTrip("block.json", new BlockMapper());
    expect(block.blockId).toBe("block-1");
    expect(block.page).toBeNull();

    const category = expectRoundTrip("category-tree.json", new CategoryMapper());
    expect(category.children?.[0]?.id).toBe("child");

    const request = expectRoundTrip("get-page-input.json", new GetPageInputMapper());
    expect(request.pageId).toBe("page-1");

    const categoryRequest = expectRoundTrip(
      "get-category-tree-input.json",
      new GetCategoryTreeInputMapper(),
    );
    expect(categoryRequest.rootId).toBe("root");

    const response = expectRoundTrip(
      "put-block-output.json",
      new PutBlockOutputMapper(),
    );
    expect(response.revision).toBe(7);
  });

  test("uses all generated KB operations through a real Nexus client", async () => {
    await withWorkflowEnvironment(async (env) => {
      const calls: Array<[string, unknown]> = [];
      const handler = nexus.serviceHandler(knowledgeBaseService, {
        async getPage(_ctx, input) {
          calls.push(["GetPage", input]);
          expect(input).toEqual({ pageId: "page-1" });
          return loadFixture<Page>("page.json");
        },
        async putBlock(_ctx, input) {
          calls.push(["PutBlock", input]);
          expect(input).toMatchObject({
            blockId: "block-1",
            order: 0,
            style: { bold: true, indent: 1 },
          });
          return loadFixture<PutBlockOutput>("put-block-output.json");
        },
        async getCategoryTree(_ctx, input) {
          calls.push(["GetCategoryTree", input]);
          expect(input).toEqual({ rootId: "root" });
          return loadFixture<Category>("category-tree.json");
        },
      });

      const result = await executeWorkflowWithNexus<{
        blockId: string;
        categoryChildId: string | undefined;
        pageId: string;
        revision: number;
      }>(env, {
        endpoint: "knowledge-base",
        nexusServices: [handler],
        workflowType: "jsonSchemaKbCaller",
        workflowsPath,
      });

      expect(result).toEqual({
        blockId: "block-1",
        categoryChildId: "child",
        pageId: "page-1",
        revision: 7,
      });
      expect(calls.map(([operation]) => operation)).toEqual([
        "GetPage",
        "PutBlock",
        "GetCategoryTree",
      ]);
    });
  });
});
