import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import * as nexus from "nexus-rpc";

import { knowledgeBaseService } from "../kb/kb/services.ts";
import { BlockMapper } from "../kb/content/block/models.ts";
import type { Block } from "../kb/content/block/models.ts";
import { CategoryMapper } from "../kb/tree/category/models.ts";
import type { Category } from "../kb/tree/category/models.ts";
import { PageMapper } from "../kb/content/page/models.ts";
import type { Page } from "../kb/content/page/models.ts";
import {
  GetCategoryTreeInputMapper,
  GetPageInputMapper,
  PutBlockOutputMapper,
} from "../kb/kb/models.ts";
import type { PutBlockOutput } from "../kb/kb/models.ts";
import { loadFixture as loadFixtureFrom } from "./json-converter-helper.ts";
import { executeWorkflowWithNexus, withWorkflowEnvironment } from "./helpers.ts";

const workflowsPath = fileURLToPath(
  new URL("./workflows/json-schema-kb.ts", import.meta.url),
);
const wireFixtureDir = new URL("../../wire/json_schema/kb/", import.meta.url);

function loadFixture<T = unknown>(name: string): T {
  return loadFixtureFrom<T>(wireFixtureDir, name);
}

describe("json-schema KB generated Nexus service", () => {
  test("exposes generated service + operation definitions", () => {
    expect(knowledgeBaseService.name).toBe("example.kb.v1.KnowledgeBaseService");
    expect(knowledgeBaseService.operations.getPage.name).toBe("GetPage");
    expect(knowledgeBaseService.operations.putBlock.name).toBe("PutBlock");
    expect(knowledgeBaseService.operations.getCategoryTree.name).toBe(
      "GetCategoryTree",
    );
  });

  // Register a handler bound to the generated service definition, then run a
  // workflow that calls every operation through the SDK's Nexus client. Both
  // sides bridge the Nexus wire payloads through the generated model mappers:
  // the handler validates each request with `fromIntermediate` and projects
  // each response with `toIntermediate`, exercising the generated
  // service/operation definitions end-to-end over a real Temporal + Nexus
  // endpoint.
  test("drives every operation through a real Nexus client", async () => {
    await withWorkflowEnvironment(async (env) => {
      const calls: Array<[string, unknown]> = [];
      const handler = nexus.serviceHandler(knowledgeBaseService, {
        async getPage(_ctx, input) {
          calls.push(["GetPage", new GetPageInputMapper().fromIntermediate(input)]);
          return new PageMapper().toIntermediate(
            loadFixture<Page>("page.json"),
          ) as Page;
        },
        async putBlock(_ctx, input) {
          calls.push(["PutBlock", new BlockMapper().fromIntermediate(input)]);
          return new PutBlockOutputMapper().toIntermediate(
            loadFixture<PutBlockOutput>("put-block-output.json"),
          ) as PutBlockOutput;
        },
        async getCategoryTree(_ctx, input) {
          calls.push([
            "GetCategoryTree",
            new GetCategoryTreeInputMapper().fromIntermediate(input),
          ]);
          return new CategoryMapper().toIntermediate(
            loadFixture<Category>("category-tree.json"),
          ) as Category;
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
      expect((calls[0]?.[1] as { pageId: string }).pageId).toBe("page-1");
      const putBlockInput = calls[1]?.[1] as Block;
      expect(putBlockInput.blockId).toBe("block-1");
      expect(putBlockInput.style?.bold).toBe(true);
      expect((calls[2]?.[1] as { rootId: string }).rootId).toBe("root");
    });
  });
});
