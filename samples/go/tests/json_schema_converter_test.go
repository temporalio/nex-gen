package tests

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	commonpb "go.temporal.io/api/common/v1"
	"go.temporal.io/sdk/converter"
	"go.temporal.io/sdk/temporal"

	"github.com/stretchr/testify/require"
)

// decodeValidation bypasses the current SDK JSON converter's legacy `%v`
// wrapper so negative tests can inspect the ApplicationError detail directly.
// Positive round trips still exercise the default data converter above.
func decodeValidation(payload *commonpb.Payload, valuePtr any) error {
	return json.Unmarshal(payload.GetData(), valuePtr)
}

// validationText exposes a locally-created payload-validation error's first
// detail for assertions. Details performs a cheap assignment here; it does not
// serialize the generated violation slice.
func validationText(err error) string {
	var applicationError *temporal.ApplicationError
	if errors.As(err, &applicationError) && applicationError.Type() == "PayloadValidationError" {
		var details any
		if applicationError.Details(&details) == nil {
			return fmt.Sprint(details)
		}
	}
	return err.Error()
}

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
