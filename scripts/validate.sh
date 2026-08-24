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

validate_rust() {
  run cargo fmt --check
  # The `advanced` feature exposes the WIT/proto CLI surface the integration tests
  # exercise; enable it so the full suite runs.
  run cargo test --features advanced
}

validate_python() {
  for tier in samples advanced/samples; do
    run_in "$tier/python" uv sync --locked
    run_in "$tier/python" uv run ruff check .
    run_in "$tier/python" uv run ruff format --check .
    run_in "$tier/python" uv run basedpyright
    run_in "$tier/python" uv run pytest
  done
}

validate_typescript() {
  for tier in samples advanced/samples; do
    run_in "$tier/typescript" npm ci
    run_in "$tier/typescript" npm exec -- prettier --check .
    run_in "$tier/typescript" npm run typecheck
    run_in "$tier/typescript" npm run test
  done
}

validate_go() {
  for tier in samples advanced/samples; do
    run_in "$tier/go" bash -c 'unformatted="$(gofmt -l .)"; if [ -n "$unformatted" ]; then echo "gofmt required for:" >&2; echo "$unformatted" >&2; exit 1; fi'
    run_in "$tier/go" go test ./...
  done
}

validate_java() {
  for tier in samples advanced/samples; do
    run_in "$tier/java" ./gradlew build --no-daemon
  done
}

validate_dotnet() {
  for tier in samples advanced/samples; do
    run_in "$tier/dotnet" dotnet test tests/ --nologo
  done
}

usage() {
  echo "Usage: $0 [rust|python|typescript|go|java|dotnet]" >&2
}

if (( $# > 1 )); then
  usage
  exit 2
fi

case "${1:-all}" in
  all)
    validate_rust
    validate_python
    validate_typescript
    validate_go
    validate_java
    validate_dotnet
    ;;
  rust) validate_rust ;;
  python) validate_python ;;
  typescript) validate_typescript ;;
  go) validate_go ;;
  java) validate_java ;;
  dotnet) validate_dotnet ;;
  *)
    usage
    exit 2
    ;;
esac
