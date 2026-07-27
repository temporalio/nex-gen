# Samples

Beginner-friendly samples generated from **JSON Schema** inputs. Each schema in
[`schemas/`](schemas/) is turned into idiomatic data models (the generator's
*definitions* mode) for .NET, Go, Java, Python, and TypeScript. This is the
recommended starting point for using `nex-gen`.

For the more advanced WIT-based generation and the JSON Schema *native-api*
(service/client) mode, see [`../advanced/samples/`](../advanced/samples/).

## Layout

- [`schemas/`](schemas/): the authored JSON Schema / `*.nexusrpc.yaml` inputs
  (`chat`, `kb`, `showcase`, `temporal`).
- [`wire/json_schema/`](wire/json_schema/): canonical wire fixtures shared by the
  per-language round-trip tests.
- Per-language projects with the generated definitions and their tests:
  - [`dotnet/`](dotnet/) — see [`dotnet/README.md`](dotnet/README.md)
  - [`go/`](go/) — see [`go/README.md`](go/README.md)
  - [`java/`](java/) — see [`java/README.md`](java/README.md)
  - [`python/`](python/) — see [`python/README.md`](python/README.md)
  - [`typescript/`](typescript/) — see [`typescript/README.md`](typescript/README.md)

## Regenerating

From the repository root:

```sh
cargo build-json-examples            # all languages
cargo build-json-examples --lang go  # a single language
```

The generated definitions are checked in and snapshot-tested by `cargo test`.
