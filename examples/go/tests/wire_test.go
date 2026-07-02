package tests

import (
	"testing"
	"time"

	"github.com/stretchr/testify/require"
	activitypb "go.temporal.io/api/activity/v1"
	commonpb "go.temporal.io/api/common/v1"
	"go.temporal.io/sdk/temporal"
	"google.golang.org/protobuf/proto"

	tr "examples/go/typeroundtrip"
)

// TestActivityOptionsWireRoundtrip verifies that a native value survives a full
// protobuf wire round-trip: native -> ToProto -> Marshal -> Unmarshal ->
// FromProto -> native. This guards the wire compatibility of the generated
// proto conversions against the binary format shared with the Python and
// TypeScript bindings.
func TestActivityOptionsWireRoundtrip(t *testing.T) {
	scheduleToClose := 7 * time.Second
	original := tr.ActivityOptions{
		TaskQueue:              ptr("demo-task-queue"),
		RetryPolicy:            temporal.RetryPolicy{MaximumAttempts: 3, BackoffCoefficient: 2.0},
		ScheduleToCloseTimeout: &scheduleToClose,
		Priority: &temporal.Priority{
			PriorityKey:    4,
			FairnessKey:    "tenant-a",
			FairnessWeight: 2.5,
		},
	}

	// native -> proto
	protoMsg := original.ToProto()

	// proto -> wire bytes -> proto
	bytes, err := proto.Marshal(protoMsg)
	require.NoError(t, err)
	var decoded activitypb.ActivityOptions
	require.NoError(t, proto.Unmarshal(bytes, &decoded))

	// proto -> native
	roundtripped := tr.ActivityOptionsFromProto(&decoded)

	require.NotNil(t, roundtripped.TaskQueue)
	require.Equal(t, *original.TaskQueue, *roundtripped.TaskQueue)
	require.Equal(t, original.RetryPolicy.MaximumAttempts, roundtripped.RetryPolicy.MaximumAttempts)
	require.InDelta(t, original.RetryPolicy.BackoffCoefficient, roundtripped.RetryPolicy.BackoffCoefficient, 0.001)
	require.NotNil(t, roundtripped.ScheduleToCloseTimeout)
	require.Equal(t, *original.ScheduleToCloseTimeout, *roundtripped.ScheduleToCloseTimeout)
	require.NotNil(t, roundtripped.Priority)
	require.Equal(t, original.Priority.PriorityKey, roundtripped.Priority.PriorityKey)
	require.Equal(t, original.Priority.FairnessKey, roundtripped.Priority.FairnessKey)
	require.InDelta(t, original.Priority.FairnessWeight, roundtripped.Priority.FairnessWeight, 0.001)
}

// TestActivityOptionsUnsetVsZero verifies the core property of the pointer-based
// converter contract: an unset optional field (nil) is preserved as nil through
// a full wire round-trip, and stays distinct from a field that is present but
// set to its zero value.
func TestActivityOptionsUnsetVsZero(t *testing.T) {
	// Unset optional fields.
	unset := tr.ActivityOptions{
		RetryPolicy: temporal.RetryPolicy{MaximumAttempts: 1},
	}
	unsetBytes, err := proto.Marshal(unset.ToProto())
	require.NoError(t, err)
	var unsetDecoded activitypb.ActivityOptions
	require.NoError(t, proto.Unmarshal(unsetBytes, &unsetDecoded))
	unsetBack := tr.ActivityOptionsFromProto(&unsetDecoded)
	require.Nil(t, unsetBack.ScheduleToCloseTimeout, "unset optional must round-trip as nil")
	require.Nil(t, unsetBack.Priority, "unset optional must round-trip as nil")

	// Present-but-zero optional fields.
	zeroDuration := time.Duration(0)
	present := tr.ActivityOptions{
		RetryPolicy:            temporal.RetryPolicy{MaximumAttempts: 1},
		ScheduleToCloseTimeout: &zeroDuration,
		Priority:               &temporal.Priority{},
	}
	presentBytes, err := proto.Marshal(present.ToProto())
	require.NoError(t, err)
	var presentDecoded activitypb.ActivityOptions
	require.NoError(t, proto.Unmarshal(presentBytes, &presentDecoded))
	presentBack := tr.ActivityOptionsFromProto(&presentDecoded)
	require.NotNil(t, presentBack.ScheduleToCloseTimeout, "present zero optional must stay non-nil")
	require.Equal(t, time.Duration(0), *presentBack.ScheduleToCloseTimeout)
	require.NotNil(t, presentBack.Priority, "present zero optional must stay non-nil")
}

// TestRetryPolicyWireFieldNames verifies that the proto produced by the Go
// conversion uses the canonical protobuf field encoding (verified by decoding
// into the upstream proto type and checking field values), which is what makes
// the payload interoperable with the other language bindings.
func TestRetryPolicyWireFieldNames(t *testing.T) {
	policy := temporal.RetryPolicy{
		InitialInterval:        time.Second,
		BackoffCoefficient:     1.5,
		MaximumInterval:        10 * time.Second,
		MaximumAttempts:        5,
		NonRetryableErrorTypes: []string{"FatalError"},
	}

	protoMsg := tr.ActivityOptions{RetryPolicy: policy}.ToProto().GetRetryPolicy()
	bytes, err := proto.Marshal(protoMsg)
	require.NoError(t, err)

	var decoded commonpb.RetryPolicy
	require.NoError(t, proto.Unmarshal(bytes, &decoded))

	require.Equal(t, time.Second, decoded.GetInitialInterval().AsDuration())
	require.InDelta(t, 1.5, decoded.GetBackoffCoefficient(), 0.001)
	require.Equal(t, 10*time.Second, decoded.GetMaximumInterval().AsDuration())
	require.Equal(t, int32(5), decoded.GetMaximumAttempts())
	require.Equal(t, []string{"FatalError"}, decoded.GetNonRetryableErrorTypes())
}
