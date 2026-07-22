package tests

import (
	"testing"

	apikb "examples/go/json_schema/api/kb"

	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/converter"
	"go.temporal.io/sdk/workflow"
)

// TestJSONSchemaKBRuntime round-trips every KB wire fixture through the Temporal
// default data converter and asserts JSON-equality against the canonical
// fixtures, mirroring the Python and Java suites.
//
// Exception (see json-schema/nullability.md): optional+nullable fields collapse
// in Go — an explicit wire `null` on such a field round-trips to absent. The
// fixtures carrying such nulls (page.json and block.json both hold `page: null`,
// which is optional+nullable) are therefore verified by deserialization + field
// checks rather than exact JSON-equality, matching the Java test.
func TestJSONSchemaKBRuntime(t *testing.T) {
	dc := converter.GetDefaultDataConverter()

	// page.json carries a nested block with page: null (optional+nullable),
	// which Go collapses on serialize, so verify by deserialization only.
	page := decodeFixture[apikb.Page](t, dc, "kb", "page.json")
	require.Equal(t, "page-1", page.PageId)
	require.Equal(t, "nex-gen", page.Meta.Author)
	require.Len(t, page.Blocks, 1)
	require.Equal(t, "block-1", page.Blocks[0].BlockId)
	require.Nil(t, page.Blocks[0].Page)
	require.NotNil(t, page.Blocks[0].Style)
	require.NotNil(t, page.Blocks[0].Style.Bold)
	require.True(t, *page.Blocks[0].Style.Bold)

	// block.json carries page: null (optional+nullable) — deserialization only.
	block := decodeFixture[apikb.Block](t, dc, "kb", "block.json")
	require.Equal(t, "block-1", block.BlockId)
	require.Equal(t, int64(0), block.Order)
	require.Nil(t, block.Page)
	require.NotNil(t, block.Style)
	require.NotNil(t, block.Style.Bold)
	require.True(t, *block.Style.Bold)

	category := roundTripJSONEq[apikb.Category](t, dc, "kb", "category-tree.json")
	require.Equal(t, "root", category.Id)
	require.Len(t, category.Children, 1)
	require.Equal(t, "child", category.Children[0].Id)

	getPage := roundTripJSONEq[apikb.GetPageInput](t, dc, "kb", "get-page-input.json")
	require.Equal(t, "page-1", getPage.PageId)

	getTree := roundTripJSONEq[apikb.GetCategoryTreeInput](t, dc, "kb", "get-category-tree-input.json")
	require.Equal(t, "root", getTree.RootId)

	putBlock := roundTripJSONEq[apikb.PutBlockOutput](t, dc, "kb", "put-block-output.json")
	require.Equal(t, "block-1", putBlock.BlockId)
	require.Equal(t, int64(7), putBlock.Revision)

	require.Equal(t, "example.kb.v1.KnowledgeBaseService", apikb.KnowledgeBaseService.ServiceName)
	require.Equal(t, "GetPage", apikb.KnowledgeBaseService.GetPage.Name())
	require.Equal(t, "PutBlock", apikb.KnowledgeBaseService.PutBlock.Name())
	require.Equal(t, "GetCategoryTree", apikb.KnowledgeBaseService.GetCategoryTree.Name())
	require.NotNil(t, apikb.NewKnowledgeBaseServiceClient("kb-endpoint"))
}

var _ func(*apikb.KnowledgeBaseServiceClient, workflow.Context, apikb.GetPageInput) workflow.Future = (*apikb.KnowledgeBaseServiceClient).GetPage
var _ func(*apikb.KnowledgeBaseServiceClient, workflow.Context, apikb.Block) workflow.Future = (*apikb.KnowledgeBaseServiceClient).PutBlock
var _ func(*apikb.KnowledgeBaseServiceClient, workflow.Context, apikb.GetCategoryTreeInput) workflow.Future = (*apikb.KnowledgeBaseServiceClient).GetCategoryTree
