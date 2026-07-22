package tests

import (
	"testing"

	apichat "examples/go/json_schema/api/chat"

	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/converter"
	"go.temporal.io/sdk/workflow"
)

// TestJSONSchemaChatRuntime round-trips every chat wire fixture through the
// Temporal default data converter and asserts JSON-equality against the
// canonical fixtures, mirroring the Python and Java suites.
//
// Exception (see json-schema/nullability.md): optional+nullable fields collapse
// in Go. message-full.json carries replyToId: null (optional+nullable), which
// Go collapses on serialize, so it is verified by deserialization + field checks
// rather than exact JSON-equality, matching the Java test. (room-open.json's
// topic is required-nullable, so its explicit null survives the round-trip and
// is checked via JSON-equality.)
func TestJSONSchemaChatRuntime(t *testing.T) {
	dc := converter.GetDefaultDataConverter()

	minimal := roundTripJSONEq[apichat.Message](t, dc, "chat", "message-minimal.json")
	require.Equal(t, apichat.MessageKindText, minimal.Kind)
	require.Equal(t, "hi", minimal.Body)
	require.Nil(t, minimal.ReplyToId)
	require.Nil(t, minimal.Priority)
	require.Equal(t, int64(0), minimal.PriorityOrDefault())

	// message-full carries replyToId: null (optional+nullable) — deserialization only.
	full := decodeFixture[apichat.Message](t, dc, "chat", "message-full.json")
	require.Nil(t, full.ReplyToId)
	require.NotNil(t, full.Priority)
	require.Equal(t, int64(7), *full.Priority)

	room := roundTripJSONEq[apichat.Room](t, dc, "chat", "room-open.json")
	require.Equal(t, "r1", room.RoomId)
	require.Nil(t, room.Topic)
	require.Equal(t, []string{"a"}, room.Members)
	require.Contains(t, room.AdditionalProperties, "x-extra")

	labels := roundTripJSONEq[apichat.Labels](t, dc, "chat", "labels.json")
	require.Equal(t, "prod", labels.AdditionalProperties["env"])
	require.Equal(t, "core", labels.AdditionalProperties["team"])

	input := roundTripJSONEq[apichat.SendMessageInput](t, dc, "chat", "send-message-input.json")
	require.Equal(t, "r1", input.RoomId)
	require.Equal(t, "hi", input.Message.Body)

	output := roundTripJSONEq[apichat.SendMessageOutput](t, dc, "chat", "send-message-output.json")
	require.Equal(t, "m1", output.MessageId)

	require.Equal(t, "example.chat.v1.ChatService", apichat.ChatService.ServiceName)
	require.Equal(t, "SendMessage", apichat.ChatService.SendMessage.Name())
	require.NotNil(t, apichat.NewChatServiceClient("chat-endpoint"))
}

var _ func(*apichat.ChatServiceClient, workflow.Context, apichat.SendMessageInput) workflow.Future = (*apichat.ChatServiceClient).SendMessage
var _ func(*apichat.ChatServiceClient, workflow.Context, apichat.GetRoomInput) workflow.Future = (*apichat.ChatServiceClient).GetRoom
var _ func(*apichat.ChatServiceClient, workflow.Context) workflow.Future = (*apichat.ChatServiceClient).Ping
