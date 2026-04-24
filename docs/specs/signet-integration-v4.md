# GraphIQ v4 — Signet Plugin Integration Spec

> Status: `approved`
> Last updated: 2026-04-24
> Companion to: `docs/specs/roadmap-v4.md`

## Purpose

Every v4 feature that adds MCP tools, CLI commands, capabilities, or manifest
changes must update both GraphIQ and Signet in lockstep. This spec defines
exactly what changes where, in what order, and what happens if they fall out of
sync.

---

## Current Contract Summary

### Two copies of truth

| Artifact | Owner | Location |
|---|---|---|
| Plugin manifest (sidecar) | GraphIQ | `signet-plugin/manifest.json` |
| Plugin manifest (bundled) | Signet | `packages/daemon/src/plugins/bundled/graphiq.ts` |
| Integration metadata | GraphIQ | `signet-plugin/integration.json` |
| Plugin state | Signet | `$SIGNET_WORKSPACE/.daemon/graphiq/state.json` |
| Index manifest | GraphIQ | `<project>/.graphiq/manifest.json` |

**Key divergence**: The bundled copy (Signet) declares `runtime.kind: "host-managed"`, `language: "typescript"`, and MCP tool names prefixed with `signet_code_`. The sidecar copy (GraphIQ) declares `runtime.kind: "sidecar"`, `language: "rust"`, and unprefixed `code_*` tool names. Signet manages GraphIQ by calling the `graphiq` CLI binary directly — it does NOT launch the MCP server. The MCP tool names in Signet's manifest are what Signet's proxy layer would expose to harnesses.

### Coupling points (ranked by fragility)

1. **CLI output parsing** — Signet regex-matches `graphiq index` stdout: `/Files:\s+(\d+)\s+Symbols:\s+(\d+).*?Edges:\s+(\d+)/s`. If GraphIQ changes this format, Signet breaks silently (stats become `undefined`).
2. **Capability strings** — Both manifests must list the same capabilities. If GraphIQ adds a new capability, Signet must also declare it or the tool won't activate.
3. **MCP tool names** — Signet's bundled manifest uses `signet_code_*` prefix. GraphIQ's MCP server exposes unprefixed names (`search`, `blast`, etc.). The mapping is convention, not enforced.
4. **Manifest schema version** — Signet reads `manifest.json` fields (`indexed_at`, `files`, `symbols`, `edges`). New fields are fine; removed/renamed fields break discovery.
5. **File layout** — Signet hardcodes `<project>/.graphiq/graphiq.db` and `<project>/.graphiq/manifest.json`. Moving these breaks everything.
6. **installSource union** — Signet validates against `"script" | "homebrew" | "source" | "existing"`. New sources from GraphIQ are rejected at parse time.

---

## Per-Feature Changes

### F1: Dead Code Detection

**GraphIQ changes:**
- New MCP tool `dead_code` on `graphiq-mcp` server
- New CLI subcommand `graphiq dead-code` (optional, for manual use)

**Signet changes:**
- New capability string: `code:dead-code`
- New MCP tool entry in bundled manifest: `signet_code_dead_code`
- New CLI command surface: `{ path: ["graphiq", "dead-code"], summary: "Find unreachable code in the active project" }`
- New `/api/graphiq/dead-code` daemon route (optional, for dashboard)
- **No** new state fields — dead code is computed on-the-fly from the graph

**Ordering:** GraphIQ ships the MCP tool first. Signet adds the capability + surface in a follow-up PR. Dead code tool works via CLI immediately; Signet surfaces it once it catches up.

**Backward compat if out of sync:** If GraphIQ has `dead_code` but Signet doesn't declare `code:dead-code`, the tool still works when agents call `graphiq-mcp` directly. Signet's proxy layer simply won't advertise it. No breakage.

---

### F2: File Watcher / Auto-Reindex

**GraphIQ changes:**
- New CLI flag: `graphiq-mcp /path --watch`
- New flag in MCP `status` response: `"watching": true`

**Signet changes:**
- Update `signet-plugin/integration.json` to document `--watch` flag
- Update Signet's install script to pass `--watch` when launching `graphiq-mcp` as a sidecar (future: when Signet transitions from host-managed to sidecar)
- **Current behavior unchanged** — Signet calls `graphiq` CLI directly. The `--watch` flag only applies to the MCP server, which Signet doesn't manage yet.
- No new capabilities, no new surfaces.

**Backward compat:** Completely safe. `--watch` is opt-in. Signet ignores it until it's ready to use it.

---

### F3: Multi-Agent Auto-Setup

**GraphIQ changes:**
- Extend `graphiq setup` to detect and configure multiple harnesses
- New `--harness <name>` flag to limit to one harness

**Signet changes:**
- None. This is purely a GraphIQ CLI feature.
- Signet's own connector system (`@signet/connector-*`) is separate and unaffected.
- The `connectorCapabilities` array in GraphIQ's sidecar manifest should stay empty (GraphIQ doesn't control Signet's connectors).

**Backward compat:** No coupling. GraphIQ can ship this independently.

---

### F4: Route Node Extraction

**GraphIQ changes:**
- New `SymbolKind::Route` in `crates/graphiq-core/src/symbol.rs`
- New `EdgeKind::Handles` in `crates/graphiq-core/src/edge.rs`
- Routes appear in search results, briefing output, blast analysis
- No new MCP tools (routes are discovered via existing `search`, `context`, `blast`)
- Manifest `schema_version` bump: `3 → 4`

**Signet changes:**
- **`manifest.json` parsing:** Signet reads `files`, `symbols`, `edges` from GraphIQ's manifest. Routes add to `symbols` count (they ARE symbols). No parsing change needed.
- **`parseIndexStats` regex:** Still matches `Files: N Symbols: M ... Edges: K`. Route symbols increase the count but don't change the format. **No change needed.**
- **State type:** `GraphiqIndexedProject` stats (`files`, `symbols`, `edges`) remain numbers. No new fields needed.
- **Plugin manifest:** No new capabilities. Routes are transparent to Signet — they're just more symbols in the graph.

**Schema migration (manifest.json):**
```
schema_version: 3 → 4
```
Signet's `discoverGraphiqProjects` reads `indexed_at`, `files`, `symbols`, `edges` — all still present. Must NOT reject manifest.json with `schema_version: 4`. Current code doesn't check `schema_version` at all, so this is safe.

**Backward compat:** If GraphIQ has routes but Signet doesn't know about them, nothing breaks. Routes are symbols. They appear in search. The graph just has more nodes.

---

### F5: Community Detection

**GraphIQ changes:**
- New MCP tool `communities` on `graphiq-mcp` server
- New optional field in `manifest.json`: `communities: number` (count)
- `briefing` tool response gains `communities` field

**Signet changes:**
- New capability string: `code:architecture`
- New MCP tool entry in bundled manifest: `signet_code_communities`
- **Optional** new field in `GraphiqIndexedProject`: `communities?: number` (like `files`, `symbols`, `edges`)
- Update `discoverGraphiqProjects` to read `communities` from manifest if present
- Update `/api/graphiq/status` response to include `communities` count per project

**Ordering:** GraphIQ ships the MCP tool. Signet adds `code:architecture` capability and the new surface. Community count in state is optional — older state files without it still work.

**Backward compat:** If GraphIQ has communities but Signet hasn't added the capability yet, community data is in the graph but Signet's proxy won't advertise the `communities` tool. No breakage. The `communities` field in manifest.json is optional — Signet ignores it if absent.

---

### F6: Execution Flow Tracing

**GraphIQ changes:**
- New MCP tool `flows` on `graphiq-mcp` server
- New optional field in `manifest.json`: `flows: number`
- `briefing` tool response gains `flows` field and `entry_point_types` array

**Signet changes:**
- Reuse `code:architecture` capability (shared with F5)
- New MCP tool entry in bundled manifest: `signet_code_flows`
- **Optional** new field in `GraphiqIndexedProject`: `flows?: number`
- Update `discoverGraphiqProjects` to read `flows` from manifest if present

**Backward compat:** Same pattern as F5. Tool is available if GraphIQ has it; Signet advertises it when it adds the surface. No state coupling beyond optional count fields.

---

### F7: Git Change Coupling

**GraphIQ changes:**
- New `EdgeKind::CoupledWith` (file-level edge, not symbol-level)
- New optional manifest field: `coupling: boolean`
- No new MCP tools — coupling edges enrich existing `blast` results
- CLI flag: `graphiq index --with-coupling`

**Signet changes:**
- None. Coupling is transparent to Signet — it makes blast results richer.
- No new capabilities, no new surfaces.
- `parseIndexStats` still matches (edge count goes up, format unchanged).
- **Optional** future: `GraphiqIndexedProject` could track `coupledFiles?: number`

**Backward compat:** Fully backward compatible. Coupling edges are just more edges. Signet doesn't inspect edge types.

---

## Capability Versioning Protocol

### Current state

```
Capabilities (v1 — current):
  code:index
  code:search
  code:context
  code:blast
  code:status
  code:doctor
  prompt:contribute:user-prompt-submit
  mcp:tool
  cli:command
```

### After v4

```
Capabilities (v2 — after v4):
  code:index              (unchanged)
  code:search             (unchanged)
  code:context            (unchanged)
  code:blast              (unchanged)
  code:status             (unchanged)
  code:doctor             (unchanged)
  code:dead-code          (NEW — F1)
  code:architecture       (NEW — F5, F6)
  prompt:contribute:user-prompt-submit  (unchanged)
  mcp:tool                (unchanged)
  cli:command             (unchanged)
```

### Rules

1. Capabilities are **additive only**. Never remove or rename one.
2. Both manifests (sidecar + bundled) must declare the same set.
3. New capabilities require updating BOTH manifests in the same PR or in coordinated PRs.
4. The `pluginApi` version does NOT bump for new capabilities — it bumps only for breaking changes to the plugin lifecycle or manifest schema.

---

## MCP Tool Name Mapping

### Current mapping

| GraphIQ MCP tool | Signet proxy name |
|---|---|
| `search` | `signet_code_search` |
| `context` | `signet_code_context` |
| `blast` | `signet_code_blast` |
| `status` | `signet_code_status` |
| `doctor` | `signet_code_doctor` |
| `constants` | `signet_code_constants` |

### After v4

| GraphIQ MCP tool | Signet proxy name | Added by |
|---|---|---|
| `search` | `signet_code_search` | — |
| `context` | `signet_code_context` | — |
| `blast` | `signet_code_blast` | — |
| `status` | `signet_code_status` | — |
| `doctor` | `signet_code_doctor` | — |
| `constants` | `signet_code_constants` | — |
| `dead_code` | `signet_code_dead_code` | F1 |
| `communities` | `signet_code_communities` | F5 |
| `flows` | `signet_code_flows` | F6 |

### Rules

1. GraphIQ MCP tools use snake_case without prefix.
2. Signet proxy names prepend `signet_code_`.
3. Mapping is 1:1 — one GraphIQ tool maps to exactly one Signet name.
4. Signet does NOT expose GraphIQ tools that manage indexing (`briefing`, `interrogate`, `explain`, `topology`, `why`, `index`, `upgrade_index`). These are GraphIQ-internal or for direct MCP consumption only.

---

## Manifest Schema Evolution

### GraphIQ `manifest.json` schema versions

| Version | Fields | Notes |
|---|---|---|
| 3 (current) | `schema_version`, `indexed_at`, `symbols`, `edges`, `files`, `artifacts`, `active_search_mode`, `best_available_mode`, `downgrade_reasons` | Current |
| 4 (v4) | All v3 + `communities?: number`, `flows?: number`, `coupling?: boolean`, `routes?: number` | Additive. All new fields optional. |

### Signet's `state.json` schema

| Version | Fields | Notes |
|---|---|---|
| 1 (current) | `pluginId`, `enabled`, `managedBy`, `installSource`, `activeProject`, `indexedProjects[]`, `updatedAt` | Current |
| 2 (v4) | All v1 + `indexedProjects[].communities?`, `indexedProjects[].flows?`, `indexedProjects[].routes?` | Additive. All new fields optional. |

### Rules

1. New fields in manifest.json are always optional with `?` in Signet types.
2. Signet MUST NOT reject manifest.json or state.json with unknown fields — forward-compatible parsing.
3. `schema_version` in manifest.json is GraphIQ-internal. Signet reads it for diagnostics only, never gates functionality on it.
4. `installSource` union: GraphIQ must NOT produce values outside `"script" | "homebrew" | "source" | "existing"` without a coordinated Signet PR.

---

## CLI Output Stability Contract

### Index output format

GraphIQ CLI `graphiq index` MUST produce output matching this pattern for Signet to parse stats:
```
Files: <n> Symbols: <n> Edges: <n>
```

Signet's regex: `/Files:\s+(\d+)\s+Symbols:\s+(\d+).*?Edges:\s+(\d+)/s`

**v4 commitment:** This format does not change. New stats (routes, communities, flows) may appear AFTER the existing format on separate lines. They are ignored by Signet's regex. Example of acceptable evolution:

```
Files: 142 Symbols: 623 Edges: 1847
Routes: 34 Communities: 8 Flows: 12
```

### Status/doctor/upgrade-index output

Signet passes stdout through to the user. Format changes are acceptable since these are display-only.

---

## Coordination Checklist per Feature

When shipping a v4 feature that touches the Signet contract:

- [ ] GraphIQ PR adds the MCP tool + CLI command
- [ ] GraphIQ PR updates `signet-plugin/manifest.json` with new capabilities + surfaces
- [ ] GraphIQ PR updates `signet-plugin/integration.json` with new `graphiqVersion`
- [ ] Signet PR updates `packages/daemon/src/plugins/bundled/graphiq.ts` (capabilities + surfaces)
- [ ] Signet PR updates `packages/core/src/graphiq.ts` types if new state fields
- [ ] Signet PR updates `/api/graphiq/status` response if new stats
- [ ] Both PRs test against each other before merge
- [ ] `installSource` union unchanged (or coordinated change)
- [ ] CLI output format backward-compatible

---

## Failure Modes & Mitigations

| Scenario | Impact | Mitigation |
|---|---|---|
| GraphIQ adds `dead_code` tool but Signet hasn't updated | Tool works via direct MCP; Signet proxy doesn't advertise it | Safe degradation |
| GraphIQ bumps manifest `schema_version` to 4 | Signet ignores it (doesn't check) | Safe |
| GraphIQ adds `Route` symbol kind | Just more symbols. Counts go up. | Safe |
| GraphIQ changes index output format | Signet gets `undefined` for stats | GraphIQ MUST NOT change format. Spec enforces this. |
| GraphIQ adds `CoupledWith` edge kind | More edges. Count goes up. | Safe |
| Signet adds capability GraphIQ doesn't have yet | Tool calls fail gracefully | Signet MUST gate features on binary version check |
| `installSource` gets a new value from GraphIQ | Signet parse rejects state.json | GraphIQ MUST NOT add values without coordinated Signet PR |
