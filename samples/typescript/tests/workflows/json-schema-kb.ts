import * as workflow from "@temporalio/workflow";

import { knowledgeBaseService } from "../../kb/kb/services.ts";
import { blockTransferTypeConverter } from "../../kb/content/block/models.ts";
import type { Block } from "../../kb/content/block/models.ts";
import { pageTransferTypeConverter } from "../../kb/content/page/models.ts";
import { categoryTransferTypeConverter } from "../../kb/tree/category/models.ts";
import {
  getCategoryTreeInputTransferTypeConverter,
  getPageInputTransferTypeConverter,
  putBlockOutputTransferTypeConverter,
} from "../../kb/kb/models.ts";
import type { GetCategoryTreeInput, GetPageInput } from "../../kb/kb/models.ts";

// Drive the generated Nexus service *definition* through the Temporal SDK's
// built-in Nexus client — no generated API client. Every request and response
// crossing the Nexus boundary goes through the generated transfer type
// converters:
// `toTransferType` projects a typed model to its plain wire form on the way
// out, and `fromTransferType` validates/parses the plain wire form back into a
// typed model on the way in. The generated operations are typed with the model,
// so the intentionally-untyped wire value from `toTransferType` is asserted
// back to the model type at the send boundary.
export async function jsonSchemaKbCaller(): Promise<{
  blockId: string;
  categoryChildId: string | undefined;
  pageId: string;
  revision: number;
}> {
  const client = workflow.createNexusServiceClient({
    service: knowledgeBaseService,
    endpoint: "knowledge-base",
  });

  const pageHandle = await client.startOperation(
    knowledgeBaseService.operations.getPage,
    getPageInputTransferTypeConverter.toTransferType({
      pageId: "page-1",
    }) as GetPageInput,
  );
  const page = pageTransferTypeConverter.fromTransferType(await pageHandle.result());
  const block = page.blocks?.[0];
  if (block == null) {
    throw new Error("expected page block");
  }

  const putBlockHandle = await client.startOperation(
    knowledgeBaseService.operations.putBlock,
    blockTransferTypeConverter.toTransferType(block) as Block,
  );
  const putBlockOutput = putBlockOutputTransferTypeConverter.fromTransferType(
    await putBlockHandle.result(),
  );

  const categoryHandle = await client.startOperation(
    knowledgeBaseService.operations.getCategoryTree,
    getCategoryTreeInputTransferTypeConverter.toTransferType({
      rootId: "root",
    }) as GetCategoryTreeInput,
  );
  const category = categoryTransferTypeConverter.fromTransferType(
    await categoryHandle.result(),
  );

  return {
    blockId: putBlockOutput.blockId,
    categoryChildId: category.children?.[0]?.id,
    pageId: page.pageId,
    revision: putBlockOutput.revision,
  };
}
