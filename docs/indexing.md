# What GraphIQ Indexes

## Languages

**16 languages parsed** into symbols (full tree-sitter extraction):

TypeScript, TSX, JavaScript, JSX, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS.

**20+ more tracked at file level** (file-tracked for freshness, not symbol-extracted):

Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS, CMake, Dockerfile, Makefile, Meson.

## Layers

| Layer | Examples |
|---|---|
| **Symbols** | functions, methods, classes, interfaces, traits, structs, enums |
| **Structure** | calls, imports, references, containment, type flow, constants |
| **Context** | comments, signatures, file paths, sibling symbols, error surfaces |
| **Maintenance** | dead code, blast radius, topology, index health |

## Edge types

| Edge | What it captures |
|---|---|
| `calls` | direct function calls |
| `references` | symbol name references |
| `imports` | module imports |
| `contains` | scope containment (struct → method, class → member) |
| `extends` / `implements` | inheritance / interface implementation |
| `shares_type` / `shares_error_type` / `shares_data_shape` / `shares_constant` | deep-graph semantic edges |
| `comment_ref` | comments mentioning other symbol names |

See [How GraphIQ works](how-graphiq-works.md) for the full edge-weight and scoring detail.

## Excluded from symbol extraction

Generated data files are file-tracked but never symbolized (see [Reliability](reliability.md)):
`package-lock.json`, `Cargo.lock`, `pnpm-lock.yaml`, `yarn.lock`, `go.sum`, and oversized JSON/YAML/TOML blobs.
