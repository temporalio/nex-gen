package model

import (
	"fmt"
	"reflect"
	"time"

	common "go.temporal.io/api/common/v1"
	deploymentpb "go.temporal.io/api/deployment/v1"
	enums "go.temporal.io/api/enums/v1"
	taskqueue "go.temporal.io/api/taskqueue/v1"
	workflowpb "go.temporal.io/api/workflow/v1"
	"go.temporal.io/sdk/client"
	"go.temporal.io/sdk/converter"
	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"
	"google.golang.org/protobuf/types/known/durationpb"
)

// --- Duration (google.protobuf.Duration) ---

func durationToProto(_ workflow.Context, d *time.Duration) (*durationpb.Duration, error) {
	if d == nil || *d == 0 {
		return nil, nil
	}
	return durationpb.New(*d), nil
}

func durationFromProto(_ workflow.Context, d *durationpb.Duration) (*time.Duration, error) {
	if d == nil {
		return nil, nil
	}
	value := d.AsDuration()
	return &value, nil
}

// --- TaskQueue (temporal.api.taskqueue.v1.TaskQueue) ---

func taskQueueToProto(_ workflow.Context, name *string) (*taskqueue.TaskQueue, error) {
	if name == nil {
		return nil, nil
	}
	return &taskqueue.TaskQueue{Name: *name}, nil
}

func taskQueueFromProto(_ workflow.Context, tq *taskqueue.TaskQueue) (*string, error) {
	if tq == nil {
		return nil, nil
	}
	value := tq.GetName()
	return &value, nil
}

// --- RetryPolicy (temporal.api.common.v1.RetryPolicy) ---

func retryPolicyToProto(_ workflow.Context, p *temporal.RetryPolicy) (*common.RetryPolicy, error) {
	if p == nil || retryPolicyIsZero(*p) {
		return nil, nil
	}
	proto := &common.RetryPolicy{
		BackoffCoefficient:     p.BackoffCoefficient,
		MaximumAttempts:        p.MaximumAttempts,
		NonRetryableErrorTypes: p.NonRetryableErrorTypes,
	}
	if p.InitialInterval != 0 {
		proto.InitialInterval = durationpb.New(p.InitialInterval)
	}
	if p.MaximumInterval != 0 {
		proto.MaximumInterval = durationpb.New(p.MaximumInterval)
	}
	return proto, nil
}

func retryPolicyIsZero(p temporal.RetryPolicy) bool {
	return p.InitialInterval == 0 &&
		p.BackoffCoefficient == 0 &&
		p.MaximumInterval == 0 &&
		p.MaximumAttempts == 0 &&
		len(p.NonRetryableErrorTypes) == 0
}

func retryPolicyFromProto(_ workflow.Context, p *common.RetryPolicy) (*temporal.RetryPolicy, error) {
	if p == nil {
		return nil, nil
	}
	policy := temporal.RetryPolicy{
		BackoffCoefficient:     p.GetBackoffCoefficient(),
		MaximumAttempts:        p.GetMaximumAttempts(),
		NonRetryableErrorTypes: p.GetNonRetryableErrorTypes(),
	}
	if interval := p.GetInitialInterval(); interval != nil {
		policy.InitialInterval = interval.AsDuration()
	}
	if interval := p.GetMaximumInterval(); interval != nil {
		policy.MaximumInterval = interval.AsDuration()
	}
	return &policy, nil
}

// --- Priority (temporal.api.common.v1.Priority) ---

func priorityToProto(_ workflow.Context, p *temporal.Priority) (*common.Priority, error) {
	if p == nil || *p == (temporal.Priority{}) {
		return nil, nil
	}
	return &common.Priority{
		PriorityKey:    int32(p.PriorityKey),
		FairnessKey:    p.FairnessKey,
		FairnessWeight: p.FairnessWeight,
	}, nil
}

func priorityFromProto(_ workflow.Context, p *common.Priority) (*temporal.Priority, error) {
	if p == nil {
		return nil, nil
	}
	return &temporal.Priority{
		PriorityKey:    int(p.GetPriorityKey()),
		FairnessKey:    p.GetFairnessKey(),
		FairnessWeight: p.GetFairnessWeight(),
	}, nil
}

// --- WorkflowType (temporal.api.common.v1.WorkflowType) ---

func workflowTypeToProto(_ workflow.Context, name *string) (*common.WorkflowType, error) {
	if name == nil {
		return nil, nil
	}
	return &common.WorkflowType{Name: *name}, nil
}

func workflowTypeFromProto(_ workflow.Context, t *common.WorkflowType) (*string, error) {
	if t == nil {
		return nil, nil
	}
	value := t.GetName()
	return &value, nil
}

// --- Payload / Payloads (temporal.api.common.v1.Payload[s]) ---
func payloadToProto(ctx workflow.Context, value any) (*common.Payload, error) {
	return getWorkflowDataConverter(ctx).ToPayload(value)
}

func payloadFromProto(ctx workflow.Context, payload *common.Payload) (any, error) {
	if payload == nil {
		return nil, nil
	}
	var value any
	if err := getWorkflowDataConverter(ctx).FromPayload(payload, &value); err != nil {
		return nil, err
	}
	return value, nil
}

func payloadsToProto(ctx workflow.Context, values []any) (*common.Payloads, error) {
	if len(values) == 0 {
		return nil, nil
	}
	payloads, err := getWorkflowDataConverter(ctx).ToPayloads(values...)
	if err != nil {
		return nil, err
	}
	return payloads, nil
}

func payloadsFromProto(ctx workflow.Context, payloads *common.Payloads) ([]any, error) {
	if payloads == nil {
		return nil, nil
	}
	values := make([]any, 0, len(payloads.GetPayloads()))
	for _, payload := range payloads.GetPayloads() {
		value, err := payloadFromProto(ctx, payload)
		if err != nil {
			return nil, err
		}
		values = append(values, value)
	}
	return values, nil
}

func getWorkflowDataConverter(ctx workflow.Context) converter.DataConverter {
	dataConverter := converter.GetDefaultDataConverter()
	if options := ctx.Value("wfEnvOptions"); options != nil {
		optionsValue := reflect.ValueOf(options)
		if optionsValue.Kind() == reflect.Pointer && !optionsValue.IsNil() {
			optionsValue = optionsValue.Elem()
		}
		if optionsValue.Kind() == reflect.Struct {
			field := optionsValue.FieldByName("DataConverter")
			if field.IsValid() && field.CanInterface() && !field.IsNil() {
				if value, ok := field.Interface().(converter.DataConverter); ok {
					dataConverter = value
				}
			}
		}
	}
	if contextAware, ok := dataConverter.(workflow.ContextAware); ok {
		return contextAware.WithWorkflowContext(ctx)
	}
	return dataConverter
}

// --- Memo (temporal.api.common.v1.Memo) ---

func memoToProto(ctx workflow.Context, memo map[string]any) (*common.Memo, error) {
	if memo == nil {
		return nil, nil
	}
	fields := make(map[string]*common.Payload, len(memo))
	for key, value := range memo {
		payload, err := payloadToProto(ctx, value)
		if err != nil {
			return nil, fmt.Errorf("encode workflow memo error: %v", err)
		}
		fields[key] = payload
	}
	return &common.Memo{Fields: fields}, nil
}

func memoFromProto(ctx workflow.Context, memo *common.Memo) (map[string]any, error) {
	if memo == nil {
		return nil, nil
	}
	result := make(map[string]any, len(memo.GetFields()))
	for key, payload := range memo.GetFields() {
		value, err := payloadFromProto(ctx, payload)
		if err != nil {
			return nil, err
		}
		result[key] = value
	}
	return result, nil
}

// --- SearchAttributes (temporal.api.common.v1.SearchAttributes) ---

func searchAttributesToProto(_ workflow.Context, searchAttributes *temporal.SearchAttributes) (*common.SearchAttributes, error) {
	if searchAttributes == nil || searchAttributes.Size() == 0 {
		return nil, nil
	}

	fields := make(map[string]*common.Payload, searchAttributes.Size())
	for key, value := range searchAttributes.GetUntypedValues() {
		payload, err := converter.GetDefaultDataConverter().ToPayload(value)
		if err != nil {
			return nil, fmt.Errorf("encode search attribute [%s] error: %v", key, err)
		}
		if payload.GetData() != nil {
			if payload.Metadata == nil {
				payload.Metadata = map[string][]byte{}
			}
			payload.Metadata["type"] = []byte(key.GetValueType().String())
		}
		fields[key.GetName()] = payload
	}
	return &common.SearchAttributes{IndexedFields: fields}, nil
}

// --- VersioningOverride (temporal.api.workflow.v1.VersioningOverride) ---

func versioningOverrideToProto(_ workflow.Context, versioningOverride *client.VersioningOverride) (*workflowpb.VersioningOverride, error) {
	if versioningOverride == nil || *versioningOverride == nil {
		return nil, nil
	}

	switch v := (*versioningOverride).(type) {
	case *client.PinnedVersioningOverride:
		return &workflowpb.VersioningOverride{
			Behavior:      enums.VERSIONING_BEHAVIOR_PINNED,
			PinnedVersion: v.Version.DeploymentName + "." + v.Version.BuildID,
			Deployment: &deploymentpb.Deployment{
				SeriesName: v.Version.DeploymentName,
				BuildId:    v.Version.BuildID,
			},
			Override: &workflowpb.VersioningOverride_Pinned{
				Pinned: &workflowpb.VersioningOverride_PinnedOverride{
					Behavior: workflowpb.VersioningOverride_PINNED_OVERRIDE_BEHAVIOR_PINNED,
					Version: &deploymentpb.WorkerDeploymentVersion{
						DeploymentName: v.Version.DeploymentName,
						BuildId:        v.Version.BuildID,
					},
				},
			},
		}, nil
	case *client.AutoUpgradeVersioningOverride:
		return &workflowpb.VersioningOverride{
			Behavior: enums.VERSIONING_BEHAVIOR_AUTO_UPGRADE,
			Override: &workflowpb.VersioningOverride_AutoUpgrade{
				AutoUpgrade: true,
			},
		}, nil
	default:
		return nil, nil
	}
}

// --- Workflow context options (sourced fields) ---

// WorkflowContextOptions configures a workflow started from workflow code.
type WorkflowContextOptions struct {
	// Namespace is the namespace in which to start or signal the workflow.
	// The current workflow's namespace is used when this is empty.
	Namespace string
	// ID is the required workflow ID.
	ID string
	// TaskQueue is the required task queue for a newly started workflow.
	TaskQueue string
	// WorkflowExecutionTimeout is the end-to-end timeout, including retries and continue-as-new.
	WorkflowExecutionTimeout time.Duration
	// WorkflowRunTimeout is the timeout for a single workflow run.
	WorkflowRunTimeout time.Duration
	// WorkflowTaskTimeout is the timeout for a single workflow task.
	WorkflowTaskTimeout time.Duration
	// WorkflowIDReusePolicy controls reuse of an ID from a closed workflow.
	WorkflowIDReusePolicy enums.WorkflowIdReusePolicy
	// RetryPolicy configures retries for the workflow.
	RetryPolicy *temporal.RetryPolicy
	// CronSchedule starts the workflow on the given cron schedule.
	CronSchedule string
	// Memo is the non-indexed information attached to the workflow.
	Memo map[string]any
	// SearchAttributes contains the typed search attributes attached to the workflow.
	SearchAttributes temporal.SearchAttributes
	// Priority configures the workflow's task priority.
	Priority temporal.Priority
}

type workflowContextOptionsKey struct{}

// WithWorkflowContextOptions returns a context carrying options for starting a workflow.
func WithWorkflowContextOptions(ctx workflow.Context, options WorkflowContextOptions) workflow.Context {
	return workflow.WithValue(ctx, workflowContextOptionsKey{}, options)
}

func workflowContextOptions(ctx workflow.Context) WorkflowContextOptions {
	options, _ := ctx.Value(workflowContextOptionsKey{}).(WorkflowContextOptions)
	return options
}

func workflowStartWorkflowID(ctx workflow.Context) string {
	id := workflowContextOptions(ctx).ID
	if id == "" {
		panic("workflow ID is required in WorkflowContextOptions")
	}
	return id
}

func workflowStartTaskQueue(ctx workflow.Context) string {
	taskQueue := workflowContextOptions(ctx).TaskQueue
	if taskQueue == "" {
		panic("task queue is required in WorkflowContextOptions")
	}
	return taskQueue
}

func workflowStartExecutionTimeout(ctx workflow.Context) time.Duration {
	return workflowContextOptions(ctx).WorkflowExecutionTimeout
}

func workflowStartRunTimeout(ctx workflow.Context) time.Duration {
	return workflowContextOptions(ctx).WorkflowRunTimeout
}

func workflowStartTaskTimeout(ctx workflow.Context) time.Duration {
	return workflowContextOptions(ctx).WorkflowTaskTimeout
}

func workflowStartIDReusePolicy(ctx workflow.Context) enums.WorkflowIdReusePolicy {
	return workflowContextOptions(ctx).WorkflowIDReusePolicy
}

func workflowStartRetryPolicy(ctx workflow.Context) temporal.RetryPolicy {
	if retryPolicy := workflowContextOptions(ctx).RetryPolicy; retryPolicy != nil {
		return *retryPolicy
	}
	return temporal.RetryPolicy{}
}

func workflowStartCronSchedule(ctx workflow.Context) string {
	return workflowContextOptions(ctx).CronSchedule
}

func workflowStartMemo(ctx workflow.Context) map[string]any {
	return workflowContextOptions(ctx).Memo
}

func workflowStartSearchAttributes(ctx workflow.Context) temporal.SearchAttributes {
	return workflowContextOptions(ctx).SearchAttributes
}

func workflowStartPriority(ctx workflow.Context) temporal.Priority {
	return workflowContextOptions(ctx).Priority
}

func workflowStartNamespace(ctx workflow.Context) string {
	namespace := workflowContextOptions(ctx).Namespace
	if namespace != "" {
		return namespace
	}
	return workflowNamespace(ctx)
}

// --- Workflow namespace (sourced field) ---

func workflowNamespace(ctx workflow.Context) string {
	if options := ctx.Value("wfEnvOptions"); options != nil {
		optionsValue := reflect.ValueOf(options)
		if optionsValue.Kind() == reflect.Pointer && !optionsValue.IsNil() {
			optionsValue = optionsValue.Elem()
		}
		if optionsValue.Kind() == reflect.Struct {
			field := optionsValue.FieldByName("Namespace")
			if field.IsValid() && field.Kind() == reflect.String {
				return field.String()
			}
		}
	}
	return ""
}
