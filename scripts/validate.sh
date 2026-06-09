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

run_in examples/python uv run ruff format --check .

run_in examples/typescript npm exec -- prettier --check .

run_in examples/go bash -c 'unformatted="$(gofmt -l .)"; if [ -n "$unformatted" ]; then echo "gofmt required for:" >&2; echo "$unformatted" >&2; exit 1; fi'
run_in examples/go go test ./...
