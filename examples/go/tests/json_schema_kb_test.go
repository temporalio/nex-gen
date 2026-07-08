package tests

import (
	"encoding/json"
	"testing"

	apicontent "examples/go/json_schema/api/kb/content"
	apikb "examples/go/json_schema/api/kb/kb"
	apicategory "examples/go/json_schema/api/kb/tree/category"
	defcontent "examples/go/json_schema/definitions/kb/content"

	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/workflow"
)

func TestJSONSchemaKBRuntime(t *testing.T) {
	bold := true
	indent := int64(2)
	text := "Intro"
	page := apicontent.Page{
		PageId: "page-1",
		Title:  "Getting Started",
		Meta: apicontent.PageMeta{
			Author: "docs",
		},
		Blocks: []apicontent.Block{
			{
				BlockId: "block-1",
				Order:   0,
				Text:    &text,
				Style: &apicontent.BlockStyle{
					Bold:   &bold,
					Indent: &indent,
				},
			},
		},
	}

	encodedPage, err := json.Marshal(page)
	require.NoError(t, err)
	require.Contains(t, string(encodedPage), `"blocks"`)

	var decodedPage apicontent.Page
	require.NoError(t, json.Unmarshal(encodedPage, &decodedPage))
	require.Equal(t, "page-1", decodedPage.PageId)
	require.Len(t, decodedPage.Blocks, 1)
	require.NotNil(t, decodedPage.Blocks[0].Style)
	require.NotNil(t, decodedPage.Blocks[0].Style.Bold)
	require.True(t, *decodedPage.Blocks[0].Style.Bold)

	var block apicontent.Block
	require.NoError(t, json.Unmarshal(
		[]byte(`{"blockId":"block-2","order":1,"style":{"bold":true},"page":null}`),
		&block,
	))
	require.Nil(t, block.Page)
	require.NotNil(t, block.Style)
	require.NotNil(t, block.Style.Bold)
	require.True(t, *block.Style.Bold)

	var category apicategory.Category
	require.NoError(t, json.Unmarshal(
		[]byte(`{"id":"root","name":"Root","children":[{"id":"child","name":"Child"}]}`),
		&category,
	))
	require.Len(t, category.Children, 1)
	require.Equal(t, "child", category.Children[0].Id)

	var definitionsPage defcontent.Page
	require.NoError(t, json.Unmarshal(encodedPage, &definitionsPage))
	require.Equal(t, "Getting Started", definitionsPage.Title)

	err = json.Unmarshal([]byte(`{"bold":"yes"}`), &apicontent.BlockStyle{})
	require.Error(t, err)
	require.Contains(t, err.Error(), "expected boolean")

	require.Equal(t, "example.kb.v1.KnowledgeBaseService", apikb.KnowledgeBaseService.ServiceName)
	require.Equal(t, "GetPage", apikb.KnowledgeBaseService.GetPage.Name())
	require.Equal(t, "PutBlock", apikb.KnowledgeBaseService.PutBlock.Name())
	require.Equal(t, "GetCategoryTree", apikb.KnowledgeBaseService.GetCategoryTree.Name())
	require.NotNil(t, apikb.NewKnowledgeBaseServiceClient("kb-endpoint"))
}

var _ func(*apikb.KnowledgeBaseServiceClient, workflow.Context, apikb.GetPageInput) workflow.NexusOperationFuture = (*apikb.KnowledgeBaseServiceClient).GetPage
var _ func(*apikb.KnowledgeBaseServiceClient, workflow.Context, apicontent.Block) workflow.NexusOperationFuture = (*apikb.KnowledgeBaseServiceClient).PutBlock
var _ func(*apikb.KnowledgeBaseServiceClient, workflow.Context, apikb.GetCategoryTreeInput) workflow.NexusOperationFuture = (*apikb.KnowledgeBaseServiceClient).GetCategoryTree
