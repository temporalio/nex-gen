# Samples

Two feature-diverse inputs and their `nex-gen` output in all four
languages. These are **illustrative** of the intended output while the
generator is under development — not a generated-from-the-real-tool
artifact and not a compatibility promise.

- **[`chat.nexusrpc.yaml`](chat.nexusrpc.yaml)** (below) — one input file:
  the **single-input** layout and a broad feature tour.
- **[`kb/`](kb)** — a four-file closure: the **multi-input** layout
  (flattened module names, a shared `definitions` file, a re-exporting
  aggregator) plus recursion — a within-file self-cycle and a cross-file
  cycle that Python hoists into `_recursive.py`. It is the worked example
  for [`../generated-file-layout.md`](../generated-file-layout.md); see
  [`kb/README.md`](kb/README.md).

The rest of this page tours the single-input chat sample.

## The input

[`chat.nexusrpc.yaml`](chat.nexusrpc.yaml) — a small chat service written
as a Nexus document. One input file, so each language gets the
**single-input** layout (types + the shared validator core inline; no
aggregator). Java is the exception: it always emits one file per public
type plus its boilerplate classes.

## Output layout

| Language | Path | Shape |
|---|---|---|
| Go | [`go/chat.go`](go/chat.go) | one file, `package chat` |
| TypeScript | [`typescript/index.ts`](typescript/index.ts) | one file |
| Python | [`python/chat/__init__.py`](python/chat/__init__.py) | one module |
| Java | [`java/com/example/chat/`](java/com/example/chat) | one `.java` per type + boilerplate |

## What each feature produces

The schema is built to touch a representative slice of the supported
subset. Trace any row from the YAML to the generated code:

| Schema feature | In `chat.nexusrpc.yaml` | What you get |
|---|---|---|
| **Service binding** | `services.ChatService` (`fqn` set) | Go `var ChatService = struct{…}`, TS `chatService = nexus.service(…)`, Python `@nexusrpc.service` class, Java `@Service` interface. The resolved wire name is always emitted explicitly. |
| **Operation, `$ref` I/O** | `sendMessage` | typed reference `SendMessageInput → SendMessageOutput`. |
| **Operation, inline I/O** | `getRoom.input` | the inline object is promoted to a synthesized `GetRoomInput` type. |
| **Operation, void I/O** | `ping` | Go `nexus.NoValue`, TS `void`, Python `None`, Java a `void` no-arg method. |
| **`const` discriminator** | `Message.kind: {const: text}` | emitted as the **underlying primitive** (open form), enforced at runtime — Go `type MessageKind = string` + value const, TS `"text" \| (string & {})`, Python `Literal["text"] \| str`, Java a nested `Message.Kind` value class. Bumping the const value never breaks the type signature. |
| **Scalar `default`** | `Message.priority: {default: 0}` | off-the-wire: the field stays optional and is **never echoed back**. Surfaced on read — Go `PriorityOrDefault()`, Python native field default, Java getter fallback, TS `DEFAULT_PRIORITY` + `?? `. |
| **Optional + nullable** | `Message.replyToId` (`oneOf [string, null]`, not required) | absent / `null` / value all accepted; round-trips faithfully in TS & Python, conservatively omits in Go/Java. |
| **Required + nullable** | `Room.topic` (`oneOf [string, null]`, required) | must be present, may be `null`; the key is always emitted (never omitted), `null` ⟷ `null`. |
| **Open struct (default)** | `Room` (no `additionalProperties`) | extras are preserved into a named catch-all (`AdditionalProperties` / `additionalProperties` / `model_extra`) and round-trip verbatim — forward compatibility. |
| **Closed struct** | `SendMessageInput`, `SendMessageOutput`, `Message`, `GetRoomInput` (`additionalProperties: false`) | unknown keys are a validation error, aggregated with the rest. |
| **Typed map (named wrapper)** | `Labels` (`additionalProperties: {type: string}`) | a named wrapper holding a `string`-valued map — never a bare map alias, so adding properties later doesn't change the type's kind. |
| **Count assertion** | `Labels.maxProperties: 50` | runtime check over distinct wire keys. |
| **Array `items`** | `Room.members` | Go `[]string`, TS `string[]`, Python `list[str]`, Java `List<String>`. |
| **`$ref` reuse** | `Message` in `SendMessageInput`, `Labels` in `Room` | a field typed as the referenced model; validation delegates to that type's own validator. |
| **Identifier case-mapping** | `roomId`, `displayName`, `replyToId`, `messageId` | mapped to each language's idiom (`RoomId` / `roomId` / `room_id`), with the original JSON name pinned on the wire. |

## The wire contract is identical

The point of generating from one schema: all four languages agree
**byte-for-byte** on the wire. The service is `"example.chat.v1.ChatService"`;
the operations are `"SendMessage"`, `"GetRoom"`, `"Ping"`; every member
serializes under its JSON name (`roomId`, `displayName`, …) regardless of
the idiomatic identifier each language uses in code.

## Validators, not just types

Every generated type carries a shared `Validate` plus mirror-image parse
and encode adapters (see the [top-level README](../README.md#runtime-validators)).
A bad payload doesn't slip through — it aggregates into one
language-native error (Go/TS a single `ValidationError` over `[]Violation` /
`pydantic.ValidationError` / `ValidationException`) that a Nexus handler can
map onto a single `BAD_REQUEST`. This is the headline difference from a
plain schema-to-struct converter.
