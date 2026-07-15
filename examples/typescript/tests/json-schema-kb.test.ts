import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import * as nexus from "nexus-rpc";

import {
  BlockMapper,
} from "../json_schema/definitions/kb/content/block/models.ts";
import {
  PageMapper,
} from "../json_schema/definitions/kb/content/page/models.ts";
import {
  CategoryMapper,
} from "../json_schema/definitions/kb/tree/category/models.ts";
import {
  GetCategoryTreeInputMapper,
  GetPageInputMapper,
  PutBlockOutputMapper,
} from "../json_schema/definitions/kb/kb/models.ts";
import type { Page } from "../json_schema/api/kb/content/page/models.ts";
import type { PutBlockOutput } from "../json_schema/api/kb/kb/models.ts";
import type { Category } from "../json_schema/api/kb/tree/category/models.ts";
import { knowledgeBaseService } from "../json_schema/api/kb/kb/services.ts";
import { executeWorkflowWithNexus, withWorkflowEnvironment } from "./helpers.ts";

const wireFixtureDir = new URL("../../wire/json_schema/kb/", import.meta.url);
const workflowsPath = fileURLToPath(
  new URL("./workflows/json-schema-kb.ts", import.meta.url),
);

function loadFixture<T = unknown>(name: string): T {
  return JSON.parse(readFileSync(new URL(name, wireFixtureDir), "utf8")) as T;
}

function expectRoundTrip<T>(
  name: string,
  parse: (raw: unknown) => T,
  serialize: (value: T) => unknown,
): T {
  const wire = loadFixture(name);
  const value = parse(wire);
  expect(serialize(value)).toEqual(wire);
  return value;
}

describe("json-schema KB generated output", () => {
  test("roundtrips multi-file KB fixtures through mapper helpers", () => {
    const pageMapper = new PageMapper();
    const page = expectRoundTrip(
      "page.json",
      (raw) => pageMapper.fromIntermediate(raw),
      (value) => pageMapper.toIntermediate(value),
    );
    expect(page.pageId).toBe("page-1");
    expect(page.blocks?.[0]?.blockId).toBe("block-1");
    expect(page.blocks?.[0]?.page).toBeNull();
    expect(page.blocks?.[0]?.style?.bold).toBe(true);

    const blockMapper = new BlockMapper();
    const block = expectRoundTrip(
      "block.json",
      (raw) => blockMapper.fromIntermediate(raw),
      (value) => blockMapper.toIntermediate(value),
    );
    expect(block.blockId).toBe("block-1");
    expect(block.page).toBeNull();

    const categoryMapper = new CategoryMapper();
    const category = expectRoundTrip(
      "category-tree.json",
      (raw) => categoryMapper.fromIntermediate(raw),
      (value) => categoryMapper.toIntermediate(value),
    );
    expect(category.children?.[0]?.id).toBe("child");

    const getPageInputMapper = new GetPageInputMapper();
    const request = expectRoundTrip(
      "get-page-input.json",
      (raw) => getPageInputMapper.fromIntermediate(raw),
      (value) => getPageInputMapper.toIntermediate(value),
    );
    expect(request.pageId).toBe("page-1");

    const getCategoryTreeInputMapper = new GetCategoryTreeInputMapper();
    const categoryRequest = expectRoundTrip(
      "get-category-tree-input.json",
      (raw) => getCategoryTreeInputMapper.fromIntermediate(raw),
      (value) => getCategoryTreeInputMapper.toIntermediate(value),
    );
    expect(categoryRequest.rootId).toBe("root");

    const putBlockOutputMapper = new PutBlockOutputMapper();
    const response = expectRoundTrip(
      "put-block-output.json",
      (raw) => putBlockOutputMapper.fromIntermediate(raw),
      (value) => putBlockOutputMapper.toIntermediate(value),
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
