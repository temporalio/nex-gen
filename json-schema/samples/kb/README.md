# Multi-file sample — knowledge base

A **multi-input** closure and its `nex-gen` output in all four languages,
the companion to the single-input [`../chat.nexusrpc.yaml`](../chat.nexusrpc.yaml)
sample. Where the chat sample shows the single-input collapse, this one
exists to exercise everything in
[`../../generated-file-layout.md`](../../generated-file-layout.md): the
one-flat-package rule, flattened module names from nested input
directories, the shared `definitions` file, the re-exporting aggregator,
and — the headline — **recursion**, both a within-file self-cycle and a
cross-file cycle that Python hoists into `_recursive.py`.

These are **illustrative** of the intended output while the generator is
under development — hand-written, not emitted by the real tool, and not a
compatibility promise.

## The input closure

Four files under [`input/`](input), reached transitively from the Nexus
document by local `$ref`. Their common-ancestor directory (`input/`) is the
**input root**; each module name is the file's path relative to that root,
flattened with `_`:

| Input file | Module | Kind |
|---|---|---|
| [`input/kb.nexusrpc.yaml`](input/kb.nexusrpc.yaml) | `kb` | Nexus document (service + synthesized I/O) |
| [`input/tree/category.json`](input/tree/category.json) | `tree_category` | pure JSON Schema — `Category` (+ dead `Palette`) |
| [`input/content/page.json`](input/content/page.json) | `content_page` | pure JSON Schema — `Page` (+ `PageMeta`) |
| [`input/content/block.json`](input/content/block.json) | `content_block` | pure JSON Schema — `Block` (+ `BlockStyle`) |

`content/page.json` → `content_page`, `tree/category.json` →
`tree_category`: the nested directory collapses into the one flat package,
the separator becoming `_`.

## The two recursion cycles

This is the reason the sample exists. The reference graph has two cycles,
and they are handled differently:

1. **Within-file self-cycle — `Category`.** `Category.children` is an
   array of `Category` (`$ref: '#'`). The whole cycle lives in one input
   file, so it **stays in its module** (`tree_category`) in every language;
   the (possibly empty) array is the terminating edge that makes it
   satisfiable.

2. **Cross-file cycle — `Page` ↔ `Block`.** `Page.blocks` is an array of
   `Block` (in `content/block.json`); `Block.page` is an optional +
   nullable back-reference to `Page` (in `content/page.json`). The
   strongly-connected component `{Page, Block}` **spans two input files**,
   so per **P14** the *cyclic types* — not the whole files — hoist:
   - **Python** moves `Page` and `Block` wholesale into
     [`python/kb/_recursive.py`](python/kb/_recursive.py), turning the
     cross-module import cycle into a within-module one (forward-ref
     back-edge + `model_rebuild()`). The non-cyclic helpers `PageMeta` /
     `BlockStyle` stay behind in `content_page` / `content_block`.
   - **Go / Java** need no special file — a single package resolves the
     cycle natively (Go uses a `*Page` pointer for the back-edge).
   - **TypeScript** needs no special file either — `import type` is
     cycle-safe and validator-function imports are ESM live bindings, so
     the cyclic types stay in their per-input modules.

## Output layout

| Language | Path | Shape |
|---|---|---|
| Go | [`go/`](go) | `package kb`: one `<module>.go` per input + `definitions.go`; no aggregator (capitalized = exported) |
| TypeScript | [`typescript/`](typescript) | one `<module>.ts` per input + `definitions.ts` + `index.ts` aggregator |
| Python | [`python/kb/`](python/kb) | one `<module>.py` per input + `definitions.py` + `_recursive.py` + `__init__.py` aggregator |
| Java | [`java/com/example/kb/`](java/com/example/kb) | one `.java` per public type + each boilerplate class its own file; `KnowledgeBaseService` its own file |

### Files, by module

```
go/                          typescript/                python/kb/                 java/com/example/kb/
  definitions.go               definitions.ts             definitions.py             Violation.java
  kb.go                        kb.ts                      _recursive.py              ValidationException.java
  tree_category.go             tree_category.ts           kb.py                      SpecNumbers.java
  content_page.go              content_page.ts            tree_category.py           KnowledgeBaseService.java
  content_block.go             content_block.ts           content_page.py            GetPageInput.java
                               index.ts                   content_block.py           PutBlockOutput.java
                                                          __init__.py                GetCategoryTreeInput.java
                                                                                     Category.java  Palette.java
                                                                                     Page.java      PageMeta.java
                                                                                     Block.java     BlockStyle.java
```

## What each layout point produces

| generated-file-layout.md point | Where to look |
|---|---|
| **Single flat package** — nested dirs collapse | one Go package `kb`, one TS dir, one Python package, one Java package `com.example.kb` |
| **Flattened module names** | `content/page.json` → `content_page.*`; `tree/category.json` → `tree_category.*` |
| **Shared `definitions` file** (`Violation`/`ValidationError`, spec-number helpers, (de)serialize scaffolding — emitted once) | `definitions.go` / `definitions.ts` / `definitions.py`; Java splits it into `Violation.java` + `ValidationException.java` + `SpecNumbers.java` |
| **Aggregator re-exports all public types + `ValidationError`** | `index.ts`, `__init__.py` (`__all__`); Go/Java have none — capitalized/`public` is the export |
| **Cross-file `$ref`** (root and named) | service I/O in `kb`; `Page.blocks` → `Block`, `Block.page` → `Page` |
| **Cross-file SCC hoisted, not whole files** | `python/kb/_recursive.py` holds `Page` + `Block`; `PageMeta`/`BlockStyle` stay put |
| **Within-file cycle stays in its module** | `Category` in `tree_category` |
| **Service binding shares its file's module + namespace** | `KnowledgeBaseService` + synthesized `GetPageInput`/`PutBlockOutput`/`GetCategoryTreeInput` all in `kb` (Java: service is its own file) |
| **Dead `$defs` still emitted/exported** | `Palette` (defined in `tree/category.json`, referenced nowhere) |

## Schema features exercised

Complementary to the chat sample's feature tour, this one also covers:

| Feature | In the schema | What you get |
|---|---|---|
| **Self-recursive type** | `Category.children: [Category]` | native recursion; Go `[]Category`, others bare references |
| **Mutually-recursive cross-file types** | `Page` ↔ `Block` | the `_recursive.py` hoist (Python); native elsewhere |
| **Required nested object** | `Page.meta: PageMeta` | must be present and non-null; validation delegates to `PageMeta` |
| **Optional + nullable `$ref`** | `Block.page` | absent / `null` / value all accepted; terminates the cycle |
| **Numeric `minimum`** | `Block.order` (`minimum: 0`), `BlockStyle.indent` | runtime lower-bound check naming the bound and offending value |
| **`boolean` scalar** | `BlockStyle.bold` | Go `bool`, TS `boolean`, Python `bool`, Java `Boolean` |
| **Cross-file service I/O** | `getPage`/`putBlock`/`getCategoryTree` | operations typed by `$ref`s that reach into other input files |
| **Synthesized inline I/O** | `getPage.input`, `putBlock.output`, `getCategoryTree.input` | promoted to `GetPageInput` / `PutBlockOutput` / `GetCategoryTreeInput` in the `kb` module |

## The wire contract is identical

As with the chat sample, all four languages agree byte-for-byte on the
wire: the service is `"example.kb.v1.KnowledgeBaseService"`; the operations
are `"GetPage"`, `"PutBlock"`, `"GetCategoryTree"`; every member serializes
under its JSON name (`pageId`, `wordCount`, `blockId`, …) regardless of the
idiomatic identifier each language uses in code.
