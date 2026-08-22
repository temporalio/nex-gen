# services / generated-file-layout / input-files / pipeline — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/services.md` — the `services`/`operations` Nexus extension: grammar, two-name model (key vs `fqn`) plus `x-<lang>-name`, object-only I/O, void encodings, per-language binding shapes, TS operation type info, P15 namespace sharing.
- `specs/json-schema/generated-file-layout.md` — one module per input file (P14), directory mirroring for Py/TS/Java vs Go's flat package, the single-input special case, shared `definitions` runtime, module-path encoding, collision scopes (P15), the Python `_recursive` SCC hoist, exports/barrels.
- `specs/json-schema/input-files.md` — per-file document modes (Nexus document vs pure JSON Schema), the definitions-only exception, root `nexusrpc` / `$schema` / `$id` / `$vocabulary` rules, the stray-`services` guard.
- `specs/json-schema/pipeline.md` — loader stage order (Parse → closure/root → strict-subset gate → reference graph → identifier pass), declaration marking that travels into reachability, and the three-layer (de)serializer shape.
- Also read: `specs/json-schema/PRINCIPLES.md` (P1–P16 + per-language sections) and `architecture.md`.

## Summary

- **The single biggest hole is Go service emission.** A module that declares a service but no models of its own (every operation type `$ref`d from another file — the exact shape `generated-file-layout.md` calls out) silently loses its `var <Service>` binding and instead emits the WIT/native-API operation-function shape, importing `go.temporal.io/sdk/workflow` and inventing an endpoint string. The repo's own test for that scenario (`tests/generate_go.rs:3121`) passes because it only counts `type Page struct {`.
- **Service and operation *keys* never run the shared identifier pipeline's Stage 3 (validity) or the P15 collision pass.** Only model `properties` do. Consequences: an operation named `import` emits uncompilable Java (`void import(...)`) and auto-mangled Go/TS/Python (`import_`), which P15 forbids outright; and two operations whose keys recase alike (`getId` + `getID`) emit duplicate struct fields / duplicate class attributes / duplicate object keys **and the same default wire name**, with no diagnostic.
- **Module-path segments are never checked against target keyword grammars.** An input file `class.json` produces `from .class import Class` (Python `SyntaxError`) and `package outj.class;` (uncompilable Java). Only the *generated-file* reserved names (`models`, `services`, `definitions`, …) are checked.
- **`deprecated: true` on an operation breaks every Python definitions-only package**: the emitter writes `typing.Annotated[...]` but only emits `import typing` under unrelated conditions, so the module raises `NameError` on import. No sample and no definitions-only test covers a deprecated operation.
- **Python omits `@service(name=…)` when the wire name equals the class name**, directly contradicting the spec's "always emitted explicitly … kept unconditionally" P1 rule (the other three targets do emit it).
- Layout mechanics that *are* right and well covered: directory mirroring, the single-input flattening, Go's flat package + `definitions.go`, `_recursive.py` cross-file SCC hoist, barrels/`__all__`, reserved generated-module names, run-wide vs per-module P15 scope (`scope_is_run_wide` — Python **is** rejected run-wide, answering the brief's question), TS operation type info incl. cross-module converter imports and the void-side omission.
- Smaller divergences: Go service/operation doc comments are not name-led (violating PRINCIPLES Go §1 and the spec's own worked example); Python emits an empty `models.py` for a module the spec says should emit none; hoisted cyclic types are not re-exported by their per-input `__init__.py`; an empty `fqn` is accepted and yields an empty wire name; the `x-output-module` fix-it the layout spec promises does not exist; root `$id` in a Nexus document reports the wrong diagnostic.
- Test-side: no cross-language conformance case covers services or the multi-file layout; no test uses an operation-level `fqn`; no CLI-binary test exercises a JSON Schema input or the Go/Java/TS subcommands (P16 convergence effectively untested); the `service_module_without_own_types` trio of tests asserts non-duplication but never asserts the service binding is present.

## Implementation divergences

### 1. Go drops the service binding entirely when the declaring module owns no models

- **Severity** P0
- **Spec cite** `services.md:224` ("Service binding | pkg-level `var <Name> = struct{…}{…}`"), `services.md:404-408`; `generated-file-layout.md:87-95` ("A service file whose every operation type is `$ref`d from elsewhere therefore declares nothing of its own"); P4 (`PRINCIPLES.md:20`, minimal deps).
- **Code cite** `src/generator/go.rs:823-832` (`GoExternalModels::new` selects the JSON backend only when the leaf's own `external_types()` still holds a `Json` binding), `src/generator/go.rs:980-1001` (the WIT/native operation path runs when `!renders_operation_references()`), `src/planning/reachability.rs:80-85` (prune drops foreign `External` declarations from a module-scoped leaf).
- **What the spec requires** The service emits into its module's `<module>.go` as `var <Service> = struct { ServiceName string; … nexus.OperationReference[...] }{…}`, with the I/O types resolved as within-package references.
- **What the code does** Reachability strips the foreign `Page` binding from the service leaf, leaving zero JSON external types; `GoExternalModels::new` then picks the proto/WIT backend, so `render_services` is never called. The leaf emits WIT-style operation functions instead.
- **Concrete failing input** The fixture already in the repo at `tests/generate_go.rs:3126-3141` (`a/page.json` + `svc.nexusrpc.yaml` whose only operation input is `{$ref: "a/page.json"}`). Actual `svc.go`:

  ```go
  package out
  import ("go.temporal.io/sdk/workflow")
  // --- Operations (internal) ---
  func one(ctx workflow.Context, request Page) workflow.Future {
      c := workflow.NewNexusClient("svc", "example.v1.Svc")
      fut := c.ExecuteOperation(ctx, "One", request, workflow.NexusOperationOptions{})
      return fut
  }
  ```
  No `var Svc`, no `nexus` import, a Temporal-SDK dependency in definitions-only output, and an invented endpoint `"svc"`.
- **Confidence** High (reproduced with the shipped binary).

### 2. Service/operation keys skip Stage-3 identifier validity — Java emits uncompilable code, the rest auto-mangle

- **Severity** P0
- **Spec cite** `services.md:373-376` (keys "run through the shared [[properties]] 4-stage algorithm — … Stage 3 per-target validity"), `services.md:544` ("Identifier invalid/reserved in an emitted lang (no override) → reject"), `features/properties.md:131-143` (Stage 3 rejects a reserved word), P15 (`PRINCIPLES.md:53`, "never silently mangled").
- **Code cite** `src/parser/json_schema.rs:4516-4535` (`build_operation` validates only the `^[a-z][a-zA-Z\d]+$` regex), `src/parser/json_schema.rs:7018-7043` (`validate_member_scope` — the only Stage-3 site — iterates `schema.properties` only), `src/parser/json_schema.rs:6380-6409` (`recase_member` appends `_` for Go/TS/Python reserved words and does **nothing** for Java).
- **What the spec requires** A key whose recased identifier is a reserved word in an emitted target is a load reject with an `x-<lang>-name` fix-it.
- **What the code does** No check at all. Java emits the keyword verbatim; Go/TS/Python auto-mangle with a trailing underscore.
- **Concrete failing input**
  ```yaml
  nexusrpc: "1.0.0"
  services:
    S1:
      operations:
        import:
          input: { type: object, properties: { a: { type: string } } }
  ```
  Java: `void import(ImportInput input);` — will not compile. TypeScript: `import_: nexus.operation<…>` (mangled although `import` is a legal TS property). Python: `import_`. A service key `Class` similarly yields TS `export const class_` (TS binds the service to a lower-camel const, so service keys are exposed to this too).
- **Confidence** High (reproduced).

### 3. Operation identifiers never enter the P15 collision pass — duplicate members and duplicate wire names

- **Severity** P0
- **Spec cite** `services.md:412-417` ("Service identifiers, **operation field identifiers**, and synthesized I/O type names all live in the identifier namespace of the declaring module (P15)"), P15 (`PRINCIPLES.md:53`), P1 (wire name must be unambiguous).
- **Code cite** `src/parser/json_schema.rs:6787-6851` (the scope loop inserts models, synthesized names, boilerplate, and services — never operations), `src/parser/json_schema.rs:4460-4477` (`build_service` collects operations with no namespace check).
- **What the spec requires** A coincidence between two operations' emitted identifiers is a load reject with a fix-it.
- **What the code does** Emits both. Because `to_upper_camel_case` also collapses them, both operations get the **same default wire name** too.
- **Concrete failing input**
  ```yaml
  nexusrpc: "1.0.0"
  services:
    S1:
      operations:
        getId:  { input: { $ref: "#/$defs/A" } }
        getID:  { input: { $ref: "#/$defs/B" } }
  $defs:
    A: { type: object, properties: { a: { type: string } } }
    B: { type: object, properties: { b: { type: string } } }
  ```
  Go: two `GetId` fields in one struct literal (compile error). Python: `get_id` declared twice (second silently wins). TypeScript: duplicate `getId` key. All four emit `name: "GetId"` twice. Accepted by every target with no diagnostic.
- **Confidence** High (reproduced).

### 4. Module-path segments are not validated against target identifier grammars

- **Severity** P0
- **Spec cite** `generated-file-layout.md:157-172` (each input file's path becomes a Python subpackage / TS directory / Java package verbatim), `generated-file-layout.md:222-233` (reserved-name rejects with a fix-it), P7.1 / P15.
- **Code cite** `src/parser/json_schema.rs:447-462` + `src/parser/json_schema.rs:555-566` (`is_reserved_module_name` checks only `definitions`/`_definitions`/`_recursive`/`models`/`services`/`index`/`__init__`), `src/parser/json_schema.rs:568-577` (`module_path_from_relative_source` passes segments through unmodified).
- **What the spec requires** By P7.1/P15 a module name that cannot be emitted in a target must reject loudly rather than produce broken output.
- **What the code does** Emits it verbatim.
- **Concrete failing input** A closure containing `class.json` and `other.json`.
  Python root `__init__.py`: `from .class import Class` → `SyntaxError: invalid syntax` (verified by running the generated package).
  Java: `samples-style` output writes `package outj.class;` in `class/Class.java` → uncompilable.
  Go is unaffected (file name only); TypeScript is unaffected (`'./class'` is a string).
- **Confidence** High (reproduced; Python import failure observed).

### 5. A deprecated operation makes every Python definitions-only package unimportable

- **Severity** P0
- **Spec cite** `services.md:230` ("Deprecated | … Python PEP 702 `typing_extensions.deprecated(..., category=None)`"), `services.md:218-220`.
- **Code cite** `src/generator/python.rs:5029-5031` (emits `: typing.Annotated[Operation[`), `src/generator/python.rs:4171-4183` (`import typing` is emitted only when an I/O type ref contains `"typing."`, when the native-API client body does, or when `needs_type_checking_imports`), `src/generator/python.rs:4184-4192` (`import typing_extensions` *is* emitted for deprecation).
- **What the spec requires** A deprecated operation lowers to a PEP 702 marker; the module must import.
- **What the code does** References `typing.Annotated` without importing `typing`. `from __future__ import annotations` defers the annotation, but `nexusrpc`'s `@service` calls `inspect.get_annotations(..., eval_str=True)`, which evaluates it.
- **Concrete failing input**
  ```yaml
  nexusrpc: "1.0.0"
  services:
    S1:
      operations:
        doIt:
          deprecated: true
          input:  { type: object, properties: { a: { type: string } } }
          output: { type: object, properties: { b: { type: string } } }
  ```
  `nexgen python … --output pkg` then `import pkg` →
  `NameError: name 'typing' is not defined. Did you forget to import 'typing'?`
  The existing test `tests/generate_python.rs:1623` misses it because it passes `generate_native_api: true`, where the endpoint-client body sets `needs_type_checking_imports` and `import typing` appears.
- **Confidence** High (reproduced end-to-end against `samples/python/.venv`).

### 6. Python omits `@service(name=…)` when the wire name equals the class name

- **Severity** P1
- **Spec cite** `services.md:143-151` ("The resolved wire name is **always emitted explicitly** (`name=…` / …), even when it equals what the SDK would default to. … The redundancy is cosmetic; the explicitness is the cross-language wire guarantee, so it is kept unconditionally."), `services.md:227`.
- **Code cite** `src/generator/python.rs:4994-5000`:
  ```rust
  if service.wire_name == service.name {
      output.push_str("@service\n");
  } else {
      output.push_str("@service(name=");
  ```
- **What the spec requires** `@nexusrpc.service(name="<wire>")` unconditionally.
- **What the code does** Bare `@service` when they coincide. Go/TS/Java all emit the name unconditionally.
- **Concrete failing input** A service `MatrixService` with no `fqn` → `@service` + `class MatrixService`. Today the wire bytes still match (nexusrpc defaults to `cls.__name__`); the rule exists precisely so a later recasing cannot shift the wire name silently. (An `x-py-name` rename *does* re-enable the explicit form, so this is narrow.)
- **Confidence** High.

### 7. Empty `fqn` is accepted and produces an empty wire name

- **Severity** P1
- **Spec cite** `services.md:111-112` (fqn is the wire name), P1 (`PRINCIPLES.md:14`) / P7.1 (`PRINCIPLES.md:27`).
- **Code cite** `src/parser/json_schema.rs:4498` (`wire_name: service.fqn.clone().unwrap_or(service_name)`), `src/parser/json_schema.rs:4593` (same for operations) — no emptiness/shape check, unlike `description`, which *is* checked at `src/parser/json_schema.rs:996-1004` / `1023-1031`.
- **What the spec requires** The spec says "optional; arbitrary chars" without an explicit emptiness rule, but a service/operation with no name is not a valid Nexus contract and P7.1 forbids silently-wrong output.
- **What the code does** `fqn: ""` emits `ServiceName: ""`, `nexus.NewOperationReference[...]("")`, `@Service(name = "")`, `nexus.service('')`.
- **Confidence** High for the behavior; medium that the spec intends a reject (the spec is silent — that silence is itself a gap).

### 8. Go service, client and operation doc comments are not name-led

- **Severity** P2
- **Spec cite** `services.md:213-217` and the worked example at `services.md:272-278` (`// ChatService - A service for sending chat messages.`, `// PollMessages - Poll for new messages.`); `features/description.md:140-153` ("title absent, description present → the identifier is prefixed to the first line of the description"); PRINCIPLES Go §1 (`PRINCIPLES.md:88`, "never a bare, unattributed sentence", explicitly naming "service/operation client bindings").
- **Code cite** `src/generator/json_schema/go.rs:5837-5843` (`render_go_doc_comment` uses the authored text verbatim and only falls back to the name-led string when absent), used at `:1145-1152`, `:1163-1171`, `:1254-1262`, `:1287-1295`. Contrast `render_go_schema_doc` at `:5854-5870`, which *does* apply the name lead for models.
- **What the code does** `samples/go/chat/chat.go:11` emits `// Send messages and look up rooms.` above `var ChatService`, and `// Post a message to a room.` above the `SendMessage` field. The `ServiceName string` field carries no comment at all.
- **Confidence** High.

### 9. Python emits an empty `models.py` for a module that declares nothing

- **Severity** P2
- **Spec cite** `generated-file-layout.md:87-95` ("A service file whose every operation type is `$ref`d from elsewhere therefore declares nothing of its own, and emits **no models file at all**").
- **Code cite** `src/generator/python.rs:738-752` inserts `models.py` unconditionally. TypeScript does honour the rule (`src/generator/typescript.rs:3080-3099`, `has_models_module`).
- **What the code does** For the service-only closure, `svc/models.py` contains only the header, `from __future__ import annotations`, and an unused `import typing`; `svc/__init__.py` correctly does not import from it.
- **Confidence** High (reproduced).

### 10. Hoisted cyclic types are not re-exported by their per-input `__init__.py`

- **Severity** P2
- **Spec cite** `generated-file-layout.md:274-280` ("those modules and the aggregators import the finished classes back from `_recursive.py`"), `generated-file-layout.md:317-321`.
- **Code cite / evidence** `samples/python/kb/content/page/__init__.py` exports only `PageMeta`; `samples/python/kb/content/__init__.py` exports only `BlockStyle`, `PageMeta`. `Page`/`Block` appear only in the package-root `samples/python/kb/__init__.py`.
- **What the code does** `from kb.content.page import Page` fails; only `from kb import Page` works.
- **Note** Re-exporting from the per-input `__init__.py` would reintroduce the import cycle the hoist exists to break (`_recursive` imports `.content.page.models`, which triggers `.content.page.__init__`). So the *spec sentence* is probably the thing that is wrong; flagged so one of the two moves.
- **Confidence** High on the observed behavior, medium on which side should change.

### 11. `x-output-module` is promised by the layout spec but does not exist

- **Severity** P2
- **Spec cite** `generated-file-layout.md:206` ("→ **load reject** with a fix-it (`x-output-module` override or rename)").
- **Code cite** `src/parser/json_schema.rs:454-461` — the actual fix-it is "rename the input file or directory". `rg x-output-module src/` returns nothing.
- **Confidence** High.

### 12. Go's flatten collision rejects at emit time, not load time, and carries no fix-it

- **Severity** P2
- **Spec cite** `generated-file-layout.md:202-215` ("Any collision in that namespace → **load reject** with a fix-it"; explicitly lists "two inputs flattening to the same module (`full/name` vs `full_name`)").
- **Code cite** `src/generator/go.rs:763-783` (`insert_generated_file` → `Error::GeneratedFileConflict`); the load-time reserved-name check at `src/parser/json_schema.rs:447-462` does not model the flattened Go namespace.
- **What the code does** Errors with "`full_name.go` … conflicts with another generated file" during emission (`tests/generate_go.rs:1683` documents this as intentional). Still loud, but the wrong stage, no fix-it, and — because it is emit-time — the same closure loads cleanly for Python/TS/Java, which is arguably correct but is not what the spec says.
- **Confidence** High.

### 13. Root `$id` in a Nexus document reports the envelope diagnostic instead of the `$id` reject

- **Severity** P2
- **Spec cite** `input-files.md:57` ("`$id` | both | **Rejected anywhere** (root or nested)").
- **Code cite** `src/parser/json_schema.rs:6088-6096` (`root_is_schema_shaped` treats the typed `id` field as schema-shape) feeding `src/parser/json_schema.rs:953-958` / `1330-1336`.
- **What the code does** `nexusrpc: "1.0.0"` + root `$id` → "a Nexus JSON schema document root is an envelope, not a model; move the model into `$defs`". Moving it into `$defs` does not fix anything. A pure schema file gives the correct "`$id` is not supported".
- **Confidence** High (reproduced).

### 14. The `.nexusrpc` filename infix is stripped from module and root-type names, undocumented

- **Severity** P2
- **Spec cite** `generated-file-layout.md:157-165` ("with the filename (minus extension) becoming the leaf per-input directory") — no mention of a `.nexusrpc` infix. Only `features/format.md:78` and `README.md` mention the naming convention, and only as an IDE-schema hint.
- **Code cite** `src/parser/json_schema.rs:579-591` (`strip_json_schema_extension` also strips `.nexusrpc`), consumed by `module_path_from_relative_source` (`:568-577`) and `root_type_name` (`:6098-6102`).
- **What the code does** `chat.nexusrpc.yaml` → module `chat`, not `chat.nexusrpc`. Sensible, but it also means `chat.yaml` and `chat.nexusrpc.yaml` in one closure collide (correctly rejected as a duplicate module path) — a rule a reader of the layout spec cannot predict.
- **Confidence** High.

## Testing gaps

### 1. No test asserts a Go service binding is emitted for a models-free module

- **Severity** P0
- **Untested** That `var <Service>` exists at all in the "service module whose every I/O is `$ref`d" layout. `tests/generate_go.rs:3121` only asserts `rendered.matches("type Page struct {").count() == 1`; the Python (`tests/generate_python.rs:1973`) and TypeScript (`tests/generate_typescript.rs:1795`) siblings likewise assert only non-duplication. Java's (`tests/generate_java.rs:956`) is the same shape.
- **Spec line** `generated-file-layout.md:87-95`; `services.md:224`.
- **Where** `tests/generate_go.rs` (extend `go_json_service_module_without_own_types_does_not_redeclare_refs`), plus the same assertion in the Python/TS/Java siblings.
- **Suggested case** Assert `rendered.contains("var Svc = struct {")`, `contains("nexus.OperationReference[Page,")`, and `!rendered.contains("workflow.NexusClient")`.

### 2. No definitions-only test of a deprecated operation in Python

- **Severity** P0
- **Untested** `deprecated: true` on an operation in `GenerationMode::DefinitionsOnly` (the CLI default). The only Python service-deprecation test (`tests/generate_python.rs:1623`) uses `generate_native_api: true`, which incidentally emits `import typing`.
- **Spec line** `services.md:230`.
- **Where** `tests/generate_python.rs`.
- **Suggested case** Duplicate `python_json_services_render_one_sided_io_names_and_deprecation` with `generate_native_api: false`, keeping the `assert_python_script_succeeds` import check. Also add `deprecated: true` to one `samples/schemas/*.nexusrpc.yaml` operation so the checked-in samples exercise it in all four targets (today only a *property* is deprecated, `samples/schemas/showcase.nexusrpc.yaml:218`).

### 3. No test for reserved-word / invalid service and operation keys

- **Severity** P0
- **Untested** Stage-3 validity for service/operation keys in any target. `rejects_reserved_member_without_override` (`src/parser/json_schema.rs:12116`) covers only `properties`.
- **Spec line** `services.md:544`; `features/properties.md:131-143`.
- **Where** `src/parser/json_schema.rs` tests, beside `rejects_invalid_service_name` / `rejects_invalid_operation_name`.
- **Suggested case** Operation key `import` must reject for Python and Java and be accepted for Go; service key `Class` must reject for TypeScript (lower-camel const) and be accepted for Go/Python/Java; each with an `x-<lang>-name` variant that resolves it.

### 4. No test for operation-identifier collisions within a service

- **Severity** P0
- **Untested** Two operation keys recasing to the same identifier (`getId` + `getID`), and the resulting duplicate default wire name.
- **Spec line** `services.md:412-417`; P15 (`PRINCIPLES.md:53`).
- **Where** `src/parser/json_schema.rs` tests, beside `rejects_service_colliding_with_model`.
- **Suggested case** Reject per target with a diagnostic naming both operation keys; add the `x-<lang>-name`-resolves-it counterpart.

### 5. No test that a module-path segment is a legal target identifier

- **Severity** P0
- **Untested** An input named after a target keyword (`class.json`, `import.json`, `package.json`, `def.json`).
- **Spec line** `generated-file-layout.md:157-172`, `:222-233`.
- **Where** `src/parser/json_schema.rs` tests, beside `rejects_reserved_module_name`.
- **Suggested case** `class.json` + `other.json` must reject for Python and Java (fix-it: rename the file/directory), and — if intentionally allowed — must be asserted importable for Go/TypeScript.

### 6. No cross-language conformance case covers services or the multi-file layout

- **Severity** P1
- **Untested** `samples/conformance/json-schema.json` has 4 cases, all single-schema model round-trips (`recursive-collections`, `mathematical-number-equality`, …). Nothing pins a service/operation wire name across the four targets, and nothing pins the multi-file/hoist layout.
- **Spec line** `services.md:361-362` ("All four agree byte-for-byte on the wire"); P1.
- **Where** `samples/conformance/json-schema.json` + `tests/json_schema_conformance_manifest.rs`.
- **Suggested case** A `service-wire-names` case over `samples/schemas/kb/kb.nexusrpc.yaml` asserting service fqn and each operation's resolved name in all four consumers, including a void-both-sides operation.

### 7. No operation-level `fqn` anywhere in tests or samples

- **Severity** P1
- **Untested** An operation `fqn` with non-identifier characters (the spec's own `poll-messages`). Every `fqn:` in `tests/` and `samples/schemas/` is service-level. Only the loader unit test at `src/parser/json_schema.rs:7495` sets one, and it uses an identifier-shaped value.
- **Spec line** `services.md:92`, `:111-117`, `:524`.
- **Where** `samples/schemas/chat.nexusrpc.yaml` (add `fqn: poll-messages` to an operation) so all four sample suites and the layout tests cover it.
- **Suggested case** Assert the literal appears verbatim in `nexus.NewOperationReference[…]("poll-messages")`, `nexus.operation({ name: "poll-messages" })`, `Operation(name="poll-messages")`, `@Operation(name = "poll-messages")`, and that the emitted identifiers stay `PollMessages`/`pollMessages`/`poll_messages`.

### 8. Empty `fqn` unspecified and untested

- **Severity** P1
- **Untested** `fqn: ""` on a service or operation.
- **Spec line** `services.md:111-112` (silent — the spec needs the rule first).
- **Where** `src/parser/json_schema.rs` tests.
- **Suggested case** Decide and pin: reject with a fix-it (recommended, P7.1) or document that an empty wire name is legal.

### 9. P16 (CLI ↔ API convergence) is barely tested

- **Severity** P1
- **Untested** The three CLI-binary tests (`tests/generate_python.rs:1043`, `:1067`, `:1120`) all use **WIT** inputs and only the `python` subcommand. No CLI test runs a JSON Schema input, the `go`/`typescript`/`java` subcommands, `--package-name`, or `--date-time-types`. `src/main.rs` is otherwise unexercised.
- **Spec line** P16 (`PRINCIPLES.md:57`), and `generator/mod.rs:101-117` documents `--date-time-types` as "P16 API parity".
- **Where** a new `tests/cli.rs`, or extend the existing CLI tests.
- **Suggested case** For each subcommand, run the binary over `samples/schemas/chat.nexusrpc.yaml` and byte-compare against `generate_to_file` with the equivalent `GenerateRequest`; include `ts --date-time-types temporal` and `java --package-name`.

### 10. Java's per-module P15 scope (two modules may each declare `Page`) is asserted nowhere

- **Severity** P1
- **Untested** The *positive* half of the P15 scope split. Go/TS/Python each have a `rejects_same_type_name_in_two_modules` test (`tests/generate_go.rs:2988`, `tests/generate_typescript.rs:1705`, `tests/generate_python.rs:1908`); Java has no counterpart asserting the same closure **succeeds** and lands two `Page.java` in distinct packages.
- **Spec line** `generated-file-layout.md:248-251`; P15 (`PRINCIPLES.md:53`).
- **Where** `tests/generate_java.rs`.
- **Suggested case** `a/page.json` + `b/page.json` → generation succeeds, `a/Page.java` and `b/Page.java` both exist with distinct `package` lines.

### 11. `rejects_service_colliding_with_model` runs for one language only

- **Severity** P2
- **Untested** The service-vs-model collision for Go and Java. `src/parser/json_schema.rs:11710` covers Python; `tests/generate_go.rs:3059` covers Go's *multi-input* variant; Java is uncovered, and the spec explicitly says the check is per emitted target and that TypeScript is the exception.
- **Spec line** `services.md:419-437`.
- **Where** `src/parser/json_schema.rs` tests — turn `rejects_service_colliding_with_model` into a per-language loop asserting reject for Go/Python/Java and accept for TypeScript.

### 12. Go doc-comment name-lead for services/operations is unasserted

- **Severity** P2
- **Untested** `tests/doc_rendering.rs` asserts the name-led rule for types and fields but not for the service `var`, its operation fields, or the client constructor. `tests/generate_go.rs:2390` asserts only the `// Deprecated:` line.
- **Spec line** `services.md:272-278`; `features/description.md:140-153`; PRINCIPLES Go §1.
- **Where** `tests/doc_rendering.rs`.
- **Suggested case** Assert `// ChatService Send messages and look up rooms.` (or the spec's `-` form) rather than the bare sentence.

### 13. Python's empty `models.py` / TS's `has_models_module` rule is asserted only for TypeScript

- **Severity** P2
- **Untested** That Python (and Java) emit no models artifact for a module declaring nothing.
- **Spec line** `generated-file-layout.md:87-95`.
- **Where** `tests/generate_python.rs:1973`.
- **Suggested case** `assert!(!output_path.join("svc/models.py").exists())`.

### 14. Non-schema files, empty files, and `.yml` inputs in a directory tree

- **Severity** P2
- **Untested** `collect_json_schema_files` (`src/parser/json_schema.rs:378-403`) skips non-`.json`/`.yaml`/`.yml` files, and an empty `.yaml` rejects with "plain JSON schema files must define a root schema or `$defs`". Neither behavior has a test, and `.yml` support is undocumented in `input-files.md`.
- **Spec line** `input-files.md` (silent on extensions and discovery — spec gap).
- **Where** `src/parser/json_schema.rs` tests beside `discovers_transitive_local_ref_closure_and_recomputes_common_root`.

### 15. Multiple explicit JSON Schema entry paths on one invocation

- **Severity** P2
- **Untested** `expand_json_schema_sources` (`src/parser/json_schema.rs:224-243`) accepts a mix of files and directories; every JSON Schema test passes exactly one path (a file or a directory).
- **Spec line** `pipeline.md:11` ("Parse begins from the explicitly supplied entry files", plural).
- **Suggested case** Two sibling files given explicitly, with the common root and module paths asserted; and a file + a directory that overlap (dedup via `canonical`).

## Combination gaps

| Feature A x Feature B | spec says | tested? | risk |
|---|---|---|---|
| Service binding x module with no own models (Go) | binding emits into `<module>.go`; the module emits no models file (`generated-file-layout.md:87-95`) | **no** — `tests/generate_go.rs:3121` asserts only `Page` non-duplication | **P0**: binding silently replaced by WIT operation functions |
| Deprecated operation x DefinitionsOnly mode (Python) | PEP 702 marker (`services.md:230`) | **no** — only NativeApi (`tests/generate_python.rs:1623`) | **P0**: `NameError` on import |
| Operation key x target reserved word (Java/Python) | load reject (`services.md:544`) | no | **P0**: uncompilable Java / auto-mangle |
| Two operation keys x case-folding collision | load reject (P15, `services.md:412-417`) | no | **P0**: duplicate members + duplicate wire name |
| Input file name x target keyword (`class.json`) | reject loudly, never mangle (P7.1/P15) | no | **P0**: Python `SyntaxError`, Java bad package |
| Operation `fqn` x non-identifier chars x all 4 emitters | wire name verbatim (`services.md:524`) | no (no op-level `fqn` in tests/samples) | P1: a quoting/recasing bug in any target ships unnoticed |
| Void input x present output x Java | **no-arg method** `Out m()` (`services.md:229`, `:364-370`) | partially — `accepts_every_one_sided_operation_io_combination` is loader-only; `tests/generate_java.rs` has no `outputOnly` shape; chat's `ping` is void-on-both-sides | P1 (behavior verified correct by hand here) |
| Cross-file `$ref` I/O x TS converter import x cycle-hoisted type | converter imported as a value from the declaring module (`services.md:523`) | yes (kb sample + `typescript_json_cross_module_ts_name_override_moves_every_reference`) | low |
| Cross-file SCC hoist x service module (Python `_recursive`) | hoisted classes imported back by modules and aggregators (`generated-file-layout.md:274-280`) | partially — `samples/python/kb/kb/services.py` imports from `.._recursive`, but no test asserts the per-input `__init__.py` re-export | P2 |
| Two modules declare `Page` x Java/.NET per-module scope | accepted, distinct qualified names (`generated-file-layout.md:248-251`) | no (only the Go/TS/Python *reject* half) | P1: a future scope change could silently start rejecting Java |
| Service x model collision x each of the 4 targets | reject in Go/Py/Java, accept in TS (`services.md:419-437`) | Python + Go(multi-input) + TS(converter fold) yes; Java no | P2 |
| Nexus document x pure JSON Schema in one closure | mode is per file (`input-files.md:46-49`) | yes (kb sample; `resolves_refs_across_input_files`) | low |
| Definitions-only file x Nexus document `$defs` x cross-file `$ref` | both contribute `$defs` only (`input-files.md:36-43`) | yes (`accepts_definitions_only_file`, `resolves_refs_across_input_files`) | low |
| Single-input special case x service x shared runtime | all at package root, Go still splits `definitions.go` (`generated-file-layout.md:60-71`) | yes (chat/showcase samples + regeneration tests) | low |
| CLI x JSON Schema input x each subcommand (P16) | CLI is a thin parser over the API (`PRINCIPLES.md:57`) | no (CLI tests are WIT + `python` only) | P1 |
| Service wire name x all four targets, byte-for-byte | `services.md:361-362` | no conformance case | P1 |

## Verified-good

- **Void I/O matrix in all four targets.** `input`-only, `output`-only, both, neither all lower correctly: Go `nexus.NoValue` on both sides, TS `void`, Python `None`, and Java's asymmetric `void m(In)` / `Out m()` / `void m()` — verified by generating a 4-operation matrix schema.
- **Object-only I/O.** `require_object_io` (`src/parser/json_schema.rs:4685-4723`) follows bare-`$ref` chains and rejects unions/scalars/arrays; shapeless `type: object` is caught by `validate_type_presence` (`:1667-1674`). Covered by `rejects_ref_union_operation_io`, `rejects_inline_union_operation_io`, `rejects_non_object_inline_operation_io`, `rejects_empty_inline_operation_io`.
- **P15 scope split.** `scope_is_run_wide` (`src/parser/json_schema.rs:6905-6910`) puts Go/TS/**Python** in the whole-run scope and Java/.NET per module — so the brief's question is answered: the Python barrel collision **is** rejected (`tests/generate_python.rs:1908`), matching the spec's "silent last-import-wins is exactly the P7 failure".
- **Tree-wide name manifest.** `EmittedNameResolutionPass::new` (`src/planning/emitted_names.rs:49-66`) builds the manifest over the whole tree, so cross-file `Page`/`Page` and cross-file `x-<lang>-name` overrides both resolve correctly (the per-leaf load check at `src/parser/json_schema.rs:833` alone would not catch either).
- **Directory mirroring and module paths.** Nested input directories become Python subpackages, TS barrel directories, and Java packages verbatim; Go flattens with `_` and preserves literal underscores. `discovers_transitive_local_ref_closure_and_recomputes_common_root`, `rejects_two_sources_with_the_same_module_path`, `rejects_source_module_path_conflicting_with_a_branch`, `rejects_reserved_module_name`, `rejects_shared_runtime_module_names` all cover this well.
- **Python `_recursive.py` hoist.** The cross-file `Page`↔`Block` SCC lands in exactly one package-root `_recursive.py`; `kb/kb/services.py` imports the hoisted classes from it; non-cyclic `PageMeta`/`BlockStyle` stay in their per-input modules. Go/TS/Java correctly emit no recursive file.
- **TS operation type info.** Non-void sides carry `{ transferTypeConverter: … }`, void sides carry neither field, and cross-module converters are imported as values beside the type-only import (`samples/typescript/kb/kb/services.ts`), asserted at `samples/typescript/tests/json-schema-kb-nexus.test.ts` including the `chatService.operations.ping` void counter-case.
- **P3 stock converter.** Go (`samples/go/tests/json_schema_kb_nexus_test.go`), Python (`samples/python/tests/test_kb_nexus.py`, explicitly on the SDK default converter via the `transfer_type_convertible` hook), and Java (`DefaultDataConverter.newDefaultInstance()`) each drive the generated service bindings end-to-end with no contrib package.
- **`x-<lang>-name` on services and operations.** Verbatim, per target, orthogonal to `fqn`, and does **not** move the synthesized `<Op>Input`/`Output` names — exercised by `samples/schemas/showcase.nexusrpc.yaml` in all four languages (`ShowcaseServiceGo`/`showcaseServiceTs`/`ShowcaseServicePy`/`ShowcaseServiceJava` with `GetShowcaseInput` unchanged). Invalid/reserved override values reject (`rejects_invalid_and_reserved_overrides`, verified for `x-go-name: "2fa"` on a service and `x-py-name: "class"` on an operation).
- **Document-mode gating.** `nexusrpc` exact-match pin, wrong `$schema` dialect, schema-shaped Nexus root, stray `services` without the marker, unknown envelope/service/operation keywords, the explicit `endpoint` reject, non-boolean `deprecated`, and empty/whitespace `description` all reject at load with named tests.
- **Definitions-only exception and dead `$defs`.** A root carrying only `$defs` emits no file-root type; dead `$defs` are still emitted and exported (verified: `DeadThing` appears in the Go package and the Python `__all__`).
- **Barrels and shared runtime placement.** `index.ts` per directory chaining upward and re-exporting `ValidationError`/`Violation` from `./definitions` at the root only; `__init__.py` per directory with `__all__`; Java runtime classes (`Violation`, `ValidationException`, `SpecNumbers`) as their own files in the root package; Go's single `definitions.go` in the flat package for both single- and multi-input closures.
