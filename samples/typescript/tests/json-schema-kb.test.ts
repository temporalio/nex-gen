import { describe, expect, test } from "vitest";

import {
  BlockMapper,
  CategoryMapper,
  GetCategoryTreeInputMapper,
  GetPageInputMapper,
  PageMapper,
  PutBlockOutputMapper,
} from "../kb/index.ts";
import {
  fixtureBytes,
  loadFixture as loadFixtureFrom,
  roundTripFixture,
  type IntermediateMapper,
} from "./json-converter-helper.ts";

const wireFixtureDir = new URL("../../wire/json_schema/kb/", import.meta.url);

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
});
