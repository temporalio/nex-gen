# Python WIT Examples

`uv`-managed Python 3.10+ example suite for the WIT generator outputs, plus the
snapshot-only JSON-Schema **native-api** outputs.

- Authored WIT inputs live in `advanced/samples/inputs/*.wit`
- Checked-in generated packages live in `advanced/samples/python/wit/<name>/`,
  where `<name>` is the snake_case WIT input name
- Generated support fragments live under each package's private `_support/`
  package, and generated models are exposed as the public `models` module
- JSON-Schema native-api outputs (services + clients) live under
  `advanced/samples/python/json_schema/api/` and are snapshot-tested only
- Proto wire-compatibility fixtures live in `advanced/samples/wire/proto/`
- Pytest files (WIT round-trips + proto wire compatibility) live in
  `advanced/samples/python/tests/`
- `build_outputs.py` is a thin wrapper around `cargo build-examples --lang python`
- `cargo test` validates the checked-in generated packages and does not rebuild them

Top-level rebuild commands:

```bash
cargo build-examples --lang python        # WIT outputs
cargo build-json-examples --lang python   # JSON-Schema api + definitions outputs
```

Current workflow:

```bash
cd advanced/samples/python
uv run build_outputs.py
uv run pytest
uv run basedpyright
```

Set `NEX_GEN_BIN=/path/to/nex-gen` to make `build_outputs.py` use an
already-built binary instead of the cargo alias.

The beginner-facing JSON-Schema definitions samples live under `samples/python/`.
