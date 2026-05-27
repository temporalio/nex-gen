#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
  printf '\n==> %s\n' "$*"
  "$@"
}

run_in() {
  local dir="$1"
  shift
  printf '\n==> (cd %s && %s)\n' "$dir" "$*"
  (cd "$dir" && "$@")
}

run_in examples/python uv sync --locked
run_in examples/typescript npm ci

run cargo fmt --check
run cargo test

run cargo build-examples --lang python
run_in examples/python uv run ruff format --check .
run_in examples/python uv run basedpyright
run_in examples/python uv run pytest tests --workflow-environment local

run cargo build-examples --lang typescript
run_in examples/typescript npm exec -- prettier --check .
run_in examples/typescript npm run typecheck
run_in examples/typescript npm run test

run git diff --exit-code
