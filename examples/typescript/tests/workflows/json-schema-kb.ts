import { KnowledgeBaseServiceClient } from "../../json_schema/api/kb/kb/index.ts";

export async function jsonSchemaKbCaller(): Promise<{
  blockId: string;
  categoryChildId: string | undefined;
  pageId: string;
  revision: number;
}> {
  const service = new KnowledgeBaseServiceClient("knowledge-base");

  const pageHandle = await service.getPage({ pageId: "page-1" });
  const page = await pageHandle.result();
  const block = page.blocks?.[0];
  if (block == null) {
    throw new Error("expected page block");
  }

  const putBlockHandle = await service.putBlock(block);
  const putBlockOutput = await putBlockHandle.result();

  const categoryHandle = await service.getCategoryTree({ rootId: "root" });
  const category = await categoryHandle.result();

  return {
    blockId: putBlockOutput.blockId,
    categoryChildId: category.children?.[0]?.id,
    pageId: page.pageId,
    revision: putBlockOutput.revision,
  };
}
