import { describe, expect, test } from "vitest";
import type { TransferTypeConverter } from "nexus-rpc";

import {
  blockTransferTypeConverter,
  categoryTransferTypeConverter,
  getCategoryTreeInputTransferTypeConverter,
  getPageInputTransferTypeConverter,
  pageTransferTypeConverter,
  putBlockOutputTransferTypeConverter,
} from "../kb/index.ts";
import {
  fixtureBytes,
  loadFixture as loadFixtureFrom,
  roundTripFixture,
} from "./json-converter-helper.ts";

const wireFixtureDir = new URL("../../wire/json_schema/kb/", import.meta.url);

function loadFixture<T = unknown>(name: string): T {
  return loadFixtureFrom<T>(wireFixtureDir, name);
}

// Round-trip a fixture through the Temporal data converter (driven by the
// generated converter) and assert the re-serialized JSON is JSON-equal to the
// fixture. TS converters preserve explicit nulls, so all KB fixtures use exact
// JSON-equality (no optional+nullable collapse — unlike Go).
function expectRoundTrip<T>(name: string, converter: TransferTypeConverter<T>): T {
  const { value, serialized } = roundTripFixture(
    converter,
    fixtureBytes(wireFixtureDir, name),
  );
  expect(serialized).toEqual(loadFixture(name));
  return value;
}

describe("json-schema KB generated output", () => {
  test("roundtrips multi-file KB fixtures through the Temporal converter", () => {
    const page = expectRoundTrip("page.json", pageTransferTypeConverter);
    expect(page.pageId).toBe("page-1");
    expect(page.blocks?.[0]?.blockId).toBe("block-1");
    expect(page.blocks?.[0]?.page).toBeNull();
    expect(page.blocks?.[0]?.style?.bold).toBe(true);

    const block = expectRoundTrip("block.json", blockTransferTypeConverter);
    expect(block.blockId).toBe("block-1");
    expect(block.page).toBeNull();

    const category = expectRoundTrip(
      "category-tree.json",
      categoryTransferTypeConverter,
    );
    expect(category.children?.[0]?.id).toBe("child");

    const request = expectRoundTrip(
      "get-page-input.json",
      getPageInputTransferTypeConverter,
    );
    expect(request.pageId).toBe("page-1");

    const categoryRequest = expectRoundTrip(
      "get-category-tree-input.json",
      getCategoryTreeInputTransferTypeConverter,
    );
    expect(categoryRequest.rootId).toBe("root");

    const response = expectRoundTrip(
      "put-block-output.json",
      putBlockOutputTransferTypeConverter,
    );
    expect(response.revision).toBe(7);
  });
});
