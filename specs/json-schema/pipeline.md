# Loader → Generator pipeline

A set of JSON Schema files becomes model code in four languages. The
loader is language-agnostic: it parses, enforces the strict subset, and
lowers everything to one shared type model. The generator emits that model
for each target. The per-file document concerns the Parse step decides
first — the two file modes and the `nexusrpc` / `$schema` root rules —
live in [[input-files]]. See [PRINCIPLES.md](PRINCIPLES.md) and
`features/<keyword>.md` for detail.

```mermaid
flowchart TD
    F["JSON Schema files (2020-12)<br/>one or more"]

    subgraph LOADER["LOADER — runs once, language-agnostic"]
      direction TB
      P["Parse"]
      R["Resolve $ref<br/>local files & named targets only;<br/>normalize paths, compute input root"]
      S["Strict-subset gate<br/>reject unsupported features<br/>with fix-it diagnostics"]
      G["Reference graph<br/>find cycles, check satisfiability"]
      N["Identifier pass<br/>case-map, reject collisions"]
      P --> R --> S --> G --> N
    end

    IR[["Type model (IR)<br/>shape · optional vs nullable · constraints · const/default"]]

    subgraph GEN["GENERATOR — emit code per target"]
      direction LR
      GO["Go"]
      TS["TypeScript"]
      PY["Python"]
      JV["Java"]
    end

    OUT["Package per language — nested tree (Py/TS/Java), flat (Go)<br/>Models + shared Validators + definitions/aggregator"]

    F --> LOADER --> IR --> GEN --> OUT
```

## Runtime validators

Every (de)serializer is three layers around one shared `Validate(model)`,
identical in both directions.

```mermaid
flowchart LR
    W1["wire JSON"] --> PA["Parse adapter<br/>spec-number parse, null rules,<br/>absence→required, unknown keys"]
    PA --> VAL{{"Shared Validate(model)<br/>constraints · const/enum · counts"}}
    VAL --> M["typed model"]
    M --> VAL
    VAL --> EA["Encode adapter<br/>omit vs emit-null, default omission"]
    EA --> W2["wire JSON"]
    VAL -. "violations" .-> ERR["aggregated errors → BAD_REQUEST"]
```
