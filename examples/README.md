# Examples

Each example starts from an authored WIT file in `inputs/`. Generated output is
checked in for .NET, Python, and TypeScript, with focused tests for the examples
that exercise runtime or typechecking behavior.

## `function-execution`

- WIT: [`inputs/function-execution.wit`](inputs/function-execution.wit)
- .NET: [`dotnet/function-execution/`](dotnet/function-execution/)
- Python: [`python/function_execution/`](python/function_execution/)
- TypeScript: [`typescript/function-execution/`](typescript/function-execution/)

## `user-service`

- WIT: [`inputs/user-service.wit`](inputs/user-service.wit)
- .NET: [`dotnet/user-service/`](dotnet/user-service/)
- Python: [`python/user_service/`](python/user_service/), [`python/tests/test_user_service.py`](python/tests/test_user_service.py)
- TypeScript: [`typescript/user-service/`](typescript/user-service/), [`typescript/tests/user-service.test.ts`](typescript/tests/user-service.test.ts)

## `type-showcase`

- WIT: [`inputs/type-showcase.wit`](inputs/type-showcase.wit)
- .NET: [`dotnet/type-showcase/`](dotnet/type-showcase/)
- Python: [`python/type_showcase/`](python/type_showcase/), [`python/tests/test_type_showcase.py`](python/tests/test_type_showcase.py)
- TypeScript: [`typescript/type-showcase/`](typescript/type-showcase/), [`typescript/tests/type-showcase.test.ts`](typescript/tests/type-showcase.test.ts)

## `start-workflow`

- WIT: [`inputs/start-workflow.wit`](inputs/start-workflow.wit)
- .NET: [`dotnet/start-workflow/`](dotnet/start-workflow/)
- Python: [`python/start_workflow/`](python/start_workflow/), [`python/tests/test_start_workflow.py`](python/tests/test_start_workflow.py)
- TypeScript: [`typescript/start-workflow/`](typescript/start-workflow/), [`typescript/tests/start-workflow.test.ts`](typescript/tests/start-workflow.test.ts)

## `workflow-service`

- WIT: [`inputs/workflow-service.wit`](inputs/workflow-service.wit)
- .NET: [`dotnet/workflow-service/`](dotnet/workflow-service/)
- Python: [`python/workflow_service/`](python/workflow_service/), [`python/tests/test_workflow_service.py`](python/tests/test_workflow_service.py)
- TypeScript: [`typescript/workflow-service/`](typescript/workflow-service/), [`typescript/tests/workflow-service.test.ts`](typescript/tests/workflow-service.test.ts)

## `type-roundtrip`

- WIT: [`inputs/type-roundtrip.wit`](inputs/type-roundtrip.wit)
- .NET: [`dotnet/type-roundtrip/`](dotnet/type-roundtrip/)
- Python: [`python/type_roundtrip/`](python/type_roundtrip/), [`python/tests/test_type_roundtrip.py`](python/tests/test_type_roundtrip.py)
- TypeScript: [`typescript/type-roundtrip/`](typescript/type-roundtrip/), [`typescript/tests/type-roundtrip.test.ts`](typescript/tests/type-roundtrip.test.ts)

Supporting example files:

- [`inputs/deps/`](inputs/deps/): reusable Temporal semantic/common type WIT inputs linked into proto-backed example generation.
- [`descriptors/temporal_api.bin`](descriptors/temporal_api.bin): Temporal API descriptor set used by proto-backed examples.
- [`python/README.md`](python/README.md): Python example suite workflow.
- [`typescript/README.md`](typescript/README.md): TypeScript example suite workflow.
