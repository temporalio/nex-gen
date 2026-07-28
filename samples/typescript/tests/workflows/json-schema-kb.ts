import * as workflow from "@temporalio/workflow";

import { knowledgeBaseService } from "../../kb/kb/services.ts";
import { BlockMapper } from "../../kb/content/block/models.ts";
import type { Block } from "../../kb/content/block/models.ts";
import { PageMapper } from "../../kb/content/page/models.ts";
import { CategoryMapper } from "../../kb/tree/category/models.ts";
import {
  GetCategoryTreeInputMapper,
  GetPageInputMapper,
  PutBlockOutputMapper,
} from "../../kb/kb/models.ts";
import type { GetCategoryTreeInput, GetPageInput } from "../../kb/kb/models.ts";

// Drive the generated Nexus service *definition* through the Temporal SDK's
// built-in Nexus client — no generated API client. Every request and response
// crossing the Nexus boundary goes through the generated model mappers:
// `toIntermediate` projects a typed model to its plain wire form on the way
// out, and `fromIntermediate` validates/parses the plain wire form back into a
// typed model on the way in. The generated operations are typed with the model,
// so the intentionally-untyped wire value from `toIntermediate` is asserted
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
    new GetPageInputMapper().toIntermediate({ pageId: "page-1" }) as GetPageInput,
  );
  const page = new PageMapper().fromIntermediate(await pageHandle.result());
  const block = page.blocks?.[0];
  if (block == null) {
    throw new Error("expected page block");
  }

  const putBlockHandle = await client.startOperation(
    knowledgeBaseService.operations.putBlock,
    new BlockMapper().toIntermediate(block) as Block,
  );
  const putBlockOutput = new PutBlockOutputMapper().fromIntermediate(
    await putBlockHandle.result(),
  );

  const categoryHandle = await client.startOperation(
    knowledgeBaseService.operations.getCategoryTree,
    new GetCategoryTreeInputMapper().toIntermediate({
      rootId: "root",
    }) as GetCategoryTreeInput,
  );
  const category = new CategoryMapper().fromIntermediate(await categoryHandle.result());

  return {
    blockId: putBlockOutput.blockId,
    categoryChildId: category.children?.[0]?.id,
    pageId: page.pageId,
    revision: putBlockOutput.revision,
  };
}
