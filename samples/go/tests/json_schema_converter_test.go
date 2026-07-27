package tests

import (
	"os"
	"path/filepath"
	"testing"

	commonpb "go.temporal.io/api/common/v1"
	"go.temporal.io/sdk/converter"

	"github.com/stretchr/testify/require"
)

// jsonSchemaFixtureBytes reads a canonical wire fixture shared across languages.
// Go test working directory is the package dir (samples/go/tests), so the
// fixtures live two levels up under samples/wire/json_schema.
func jsonSchemaFixtureBytes(t *testing.T, suite, name string) []byte {
	t.Helper()
	path := filepath.Join("..", "..", "wire", "json_schema", suite, name)
	data, err := os.ReadFile(path)
	require.NoError(t, err)
	return data
}

// jsonPayload wraps raw fixture bytes as a json/plain Temporal payload, matching
// how a real Nexus/workflow payload arrives on the wire.
func jsonPayload(data []byte) *commonpb.Payload {
	return &commonpb.Payload{
		Metadata: map[string][]byte{"encoding": []byte("json/plain")},
		Data:     data,
	}
}

// decodeFixture deserializes a fixture into the generated model T *through the
// Temporal data converter* (its json/plain path invokes the model's custom
// UnmarshalJSON via encoding/json).
func decodeFixture[T any](t *testing.T, dc converter.DataConverter, suite, name string) T {
	t.Helper()
	var out T
	require.NoError(t, dc.FromPayload(jsonPayload(jsonSchemaFixtureBytes(t, suite, name)), &out))
	return out
}

// reencode serializes a model back through the data converter and returns the
// raw payload bytes.
func reencode[T any](t *testing.T, dc converter.DataConverter, value T) []byte {
	t.Helper()
	payload, err := dc.ToPayload(value)
	require.NoError(t, err)
	return payload.GetData()
}

// roundTripJSONEq decodes a fixture, re-encodes it, and asserts the re-serialized
// JSON is semantically equal to the fixture (JSON-equal, not byte-equal: the
// converter emits compact JSON while fixtures are pretty-printed).
func roundTripJSONEq[T any](t *testing.T, dc converter.DataConverter, suite, name string) T {
	t.Helper()
	value := decodeFixture[T](t, dc, suite, name)
	got := reencode(t, dc, value)
	require.JSONEq(t, string(jsonSchemaFixtureBytes(t, suite, name)), string(got), name)
	return value
}
