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

for tier in samples advanced/samples; do
  run_in "$tier/python" uv sync --locked
  run_in "$tier/typescript" npm ci
done

run cargo fmt --check
# The `advanced` feature exposes the WIT/proto CLI surface the integration tests
# exercise; enable it so the full suite runs.
run cargo test --features advanced

for tier in samples advanced/samples; do
  run_in "$tier/python" uv run ruff format --check .
  run_in "$tier/typescript" npm exec -- prettier --check .
  run_in "$tier/go" bash -c 'unformatted="$(gofmt -l .)"; if [ -n "$unformatted" ]; then echo "gofmt required for:" >&2; echo "$unformatted" >&2; exit 1; fi'
  run_in "$tier/go" go test ./...
  run_in "$tier/dotnet" dotnet test tests/ --nologo
done
