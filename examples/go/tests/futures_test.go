package tests

import "go.temporal.io/sdk/workflow"

type operationFuture interface {
	Get(workflow.Context, any) error
}

func getFutureResult[T any](ctx workflow.Context, fut operationFuture) (*T, error) {
	var result T
	if err := fut.Get(ctx, &result); err != nil {
		return nil, err
	}
	return &result, nil
}
