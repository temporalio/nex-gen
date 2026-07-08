package tests

import (
	"encoding/json"
	"testing"

	apichat "examples/go/json_schema/api/chat"

	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/workflow"
)

func TestJSONSchemaChatRuntime(t *testing.T) {
	topic := "general"
	room := apichat.Room{
		RoomId:      "room-1",
		DisplayName: "General",
		Topic:       &topic,
		Members:     []string{"user-1"},
		Labels: &apichat.Labels{
			AdditionalProperties: map[string]string{"team": "sdk"},
		},
		AdditionalProperties: map[string]json.RawMessage{
			"extra": json.RawMessage(`{"kept":true}`),
		},
	}

	encoded, err := json.Marshal(room)
	require.NoError(t, err)
	require.Contains(t, string(encoded), `"extra":{"kept":true}`)

	var decoded apichat.Room
	require.NoError(t, json.Unmarshal(encoded, &decoded))
	require.Equal(t, "room-1", decoded.RoomId)
	require.NotNil(t, decoded.Topic)
	require.Equal(t, "general", *decoded.Topic)
	require.Equal(t, json.RawMessage(`{"kept":true}`), decoded.AdditionalProperties["extra"])

	decoded.Topic = nil
	encoded, err = json.Marshal(decoded)
	require.NoError(t, err)
	require.Contains(t, string(encoded), `"topic":null`)

	messageBytes := []byte(`{"kind":"text","body":"hello","priority":2}`)
	var message apichat.Message
	require.NoError(t, json.Unmarshal(messageBytes, &message))
	require.Equal(t, int64(2), message.PriorityOrDefault())

	err = json.Unmarshal([]byte(`{"kind":"image","body":"nope"}`), &message)
	require.Error(t, err)
	require.Contains(t, err.Error(), "kind")
	require.Contains(t, err.Error(), "const: must equal")

	require.Equal(t, "example.chat.v1.ChatService", apichat.ChatService.ServiceName)
	require.Equal(t, "SendMessage", apichat.ChatService.SendMessage.Name())
	require.NotNil(t, apichat.NewChatServiceClient("chat-endpoint"))
}

var _ func(*apichat.ChatServiceClient, workflow.Context, apichat.SendMessageInput) workflow.NexusOperationFuture = (*apichat.ChatServiceClient).SendMessage
var _ func(*apichat.ChatServiceClient, workflow.Context, apichat.GetRoomInput) workflow.NexusOperationFuture = (*apichat.ChatServiceClient).GetRoom
var _ func(*apichat.ChatServiceClient, workflow.Context) workflow.NexusOperationFuture = (*apichat.ChatServiceClient).Ping
