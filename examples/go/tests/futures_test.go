package tests

import "go.temporal.io/sdk/workflow"

func getFutureResult[T any](ctx workflow.Context, fut workflow.NexusOperationFuture) (*T, error) {
	var result T
	if err := fut.Get(ctx, &result); err != nil {
		return nil, err
	}
	return &result, nil
}
