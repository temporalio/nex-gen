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
	original := tr.ActivityOptions{
		TaskQueue:              "demo-task-queue",
		RetryPolicy:            temporal.RetryPolicy{MaximumAttempts: 3, BackoffCoefficient: 2.0},
		ScheduleToCloseTimeout: 7 * time.Second,
		Priority: temporal.Priority{
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

	require.Equal(t, original.TaskQueue, roundtripped.TaskQueue)
	require.Equal(t, original.RetryPolicy.MaximumAttempts, roundtripped.RetryPolicy.MaximumAttempts)
	require.InDelta(t, original.RetryPolicy.BackoffCoefficient, roundtripped.RetryPolicy.BackoffCoefficient, 0.001)
	require.Equal(t, original.ScheduleToCloseTimeout, roundtripped.ScheduleToCloseTimeout)
	require.Equal(t, original.Priority.PriorityKey, roundtripped.Priority.PriorityKey)
	require.Equal(t, original.Priority.FairnessKey, roundtripped.Priority.FairnessKey)
	require.InDelta(t, original.Priority.FairnessWeight, roundtripped.Priority.FairnessWeight, 0.001)
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

	protoMsg := tr.RetryPolicyToProto(policy)
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
