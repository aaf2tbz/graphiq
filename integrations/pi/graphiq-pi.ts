/**
 * GraphIQ extension for pi.
 *
 * pi intentionally does NOT ship built-in MCP support. Signet integrates with
 * pi by shipping a pi extension that talks to its daemon over HTTP; graphiq has
 * no daemon, so this extension talks to the graphiq CLI directly (the same
 * reliable, tested binary agents use everywhere else).
 *
 * What this gives pi:
 *   - `graphiq_search`, `graphiq_context`, `graphiq_status` tools the LLM can call
 *   - `/graphiq` command to index/inspect the active project
 *   - on session_start, auto-index the project in the background if a graphiq
 *     binary is present (matching how signet auto-warms per session)
 *
 * Install: `graphiq setup --harness pi` copies this file into
 * ~/.pi/agent/extensions/graphiq-pi.ts (pi auto-discovers it there).
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// Resolve the graphiq binary: explicit override, then PATH lookup, then sibling.
const GRAPHIQ_BIN = process.env.GRAPHIQ_BIN ?? "graphiq";

function graphiq(ctx: any, args: string[], opts: { timeoutMs?: number } = {}): Promise<string> {
  return new Promise((resolve) => {
    const cwd = ctx?.sessionManager?.getCwd?.() ?? process.cwd();
    const child = spawn(GRAPHIQ_BIN, args, {
      cwd,
      env: process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let out = "";
    let err = "";
    const timer = opts.timeoutMs
      ? setTimeout(() => {
          child.kill("SIGTERM");
        }, opts.timeoutMs)
      : null;
    child.stdout.on("data", (d) => (out += d.toString()));
    child.stderr.on("data", (d) => (err += d.toString()));
    child.on("close", () => {
      if (timer) clearTimeout(timer);
      // graphiq prints progress to stderr and results to stdout; prefer stdout.
      resolve(out.trim() || err.trim());
    });
    child.on("error", (e) => {
      if (timer) clearTimeout(timer);
      resolve(`graphiq failed to start: ${e.message}`);
    });
  });
}

function isGraphiqInstalled(): boolean {
  if (process.env.GRAPHIQ_BIN && existsSync(process.env.GRAPHIQ_BIN)) return true;
  // Cheap PATH check via `which`/`where`.
  try {
    const { spawnSync } = require("node:child_process");
    const probe = process.platform === "win32" ? "where" : "which";
    const r = spawnSync(probe, ["graphiq"], { stdio: "ignore" });
    return r.status === 0;
  } catch {
    return false;
  }
}

const DB_FLAG = () => {
  // Project-local .graphiq db by default; overridable via GRAPHIQ_DB.
  return process.env.GRAPHIQ_DB ?? ".graphiq/graphiq.db";
};

// Track the background indexer so we can stop it on session shutdown (dormancy:
// graphiq must not keep doing work after the harness/project closes).
let backgroundIndexer: ReturnType<typeof spawn> | null = null;

export default function (pi: ExtensionAPI) {
  // Status + auto-index on session start (mirrors signet's per-session warm).
  pi.on("session_start", async (_event, ctx) => {
    if (!isGraphiqInstalled()) {
      ctx.ui.notify(
        "GraphIQ not installed. Run: curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash",
        "info",
      );
      return;
    }
    ctx.ui.setStatus("graphiq", ctx.ui.theme.fg("accent", "graphiq:ready"));
    // Index in the background if no index exists yet. Keep load light.
    const cwd = ctx?.sessionManager?.getCwd?.() ?? process.cwd();
    const db = join(cwd, DB_FLAG());
    if (!existsSync(db)) {
      ctx.ui.notify("GraphIQ indexing project in background…", "info");
      // NOT detached: the child is tracked so session_shutdown can stop it,
      // keeping graphiq dormant when no session is active.
      backgroundIndexer = spawn(
        GRAPHIQ_BIN,
        ["index", cwd, "--db", db],
        {
          cwd,
          env: { ...process.env, GRAPHIQ_INDEX_MODE: "background", RAYON_NUM_THREADS: "2" },
          stdio: "ignore",
        },
      );
      backgroundIndexer.on("exit", () => {
        backgroundIndexer = null;
      });
    }
  });

  pi.on("session_shutdown", async (_event, ctx) => {
    // DORMANCY: stop any background indexer the moment the session closes, so
    // graphiq does no work while no harness/project is active.
    if (backgroundIndexer && backgroundIndexer.pid) {
      try {
        backgroundIndexer.kill("SIGTERM");
      } catch {
        /* already gone */
      }
      backgroundIndexer = null;
    }
    ctx.ui.setStatus("graphiq", undefined);
  });

  // Tools the LLM can call. These mirror the graphiq-mcp tool surface so pi
  // agents get the same code-intelligence as MCP harnesses.
  pi.registerTool({
    name: "graphiq_search",
    label: "GraphIQ Search",
    description:
      "Search the GraphIQ-indexed codebase by symbol name, natural language, file path, or error message. Returns ranked results with scores, locations, and signatures. This is structural code search — better than grep for finding where things are and how code is connected. The project must be indexed first (run /graphiq index).",
    promptSnippet: "Search indexed code by symbol name, description, or file path",
    parameters: Type.Object({
      query: Type.String({ description: "Search query — symbol name, natural language, file path, or error message" }),
      top: Type.Optional(Type.Number({ description: "Max results (default 10)", default: 10 })),
    }),
    async execute(_id, params, _signal, _onUpdate, ctx) {
      if (!isGraphiqInstalled()) {
        return { content: [{ type: "text" as const, text: "GraphIQ is not installed." }] };
      }
      const q = String(params.query ?? "");
      const top = String(params.top ?? 10);
      const text = await graphiq(ctx, ["search", q, "--top", top, "--db", DB_FLAG()], { timeoutMs: 30000 });
      return { content: [{ type: "text" as const, text }] };
    },
  });

  pi.registerTool({
    name: "graphiq_context",
    label: "GraphIQ Context",
    description:
      "Read a symbol's full source and structural neighborhood (callers, callees, members). Use after graphiq_search to go deeper on a specific result.",
    parameters: Type.Object({
      symbol: Type.String({ description: "Symbol name" }),
    }),
    async execute(_id, params, _signal, _onUpdate, ctx) {
      if (!isGraphiqInstalled()) {
        return { content: [{ type: "text" as const, text: "GraphIQ is not installed." }] };
      }
      const sym = String(params.symbol ?? "");
      const text = await graphiq(ctx, ["context", sym, "--db", DB_FLAG()], { timeoutMs: 15000 });
      return { content: [{ type: "text" as const, text }] };
    },
  });

  pi.registerTool({
    name: "graphiq_status",
    label: "GraphIQ Status",
    description:
      "Show GraphIQ index status for the active project (file/symbol/edge counts, search mode, artifact health). Use to check whether the index is fresh before relying on search.",
    parameters: Type.Object({}),
    async execute(_id, _params, _signal, _onUpdate, ctx) {
      if (!isGraphiqInstalled()) {
        return { content: [{ type: "text" as const, text: "GraphIQ is not installed." }] };
      }
      const text = await graphiq(ctx, ["status", "--db", DB_FLAG()], { timeoutMs: 10000 });
      return { content: [{ type: "text" as const, text }] };
    },
  });

  // /graphiq command — index, status, or search from the prompt line.
  pi.registerCommand("graphiq", {
    description: "GraphIQ: index | status | search <query>",
    handler: async (args, ctx) => {
      if (!isGraphiqInstalled()) {
        ctx.ui.notify(
          "GraphIQ not installed. Run: curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash",
          "warning",
        );
        return;
      }
      const parts = String(args ?? "").trim().split(/\s+/);
      const sub = parts[0] ?? "status";
      const cwd = ctx?.sessionManager?.getCwd?.() ?? process.cwd();
      if (sub === "index") {
        ctx.ui.notify("GraphIQ indexing…", "info");
        const out = await graphiq(ctx, ["index", cwd, "--db", DB_FLAG()], { timeoutMs: 120000 });
        ctx.ui.notify("GraphIQ index complete", "success");
        // surface a short summary
        const summary = out.split("\n").filter((l) => /Files:|Symbols:|done/i.test(l)).slice(0, 2).join(" | ");
        if (summary) ctx.ui.notify(summary, "info");
      } else if (sub === "status") {
        const out = await graphiq(ctx, ["status", "--db", DB_FLAG()], { timeoutMs: 10000 });
        ctx.ui.notify(out.split("\n").slice(0, 6).join("\n"), "info");
      } else if (sub === "search") {
        const q = parts.slice(1).join(" ");
        if (!q) {
          ctx.ui.notify("Usage: /graphiq search <query>", "warning");
          return;
        }
        const out = await graphiq(ctx, ["search", q, "--db", DB_FLAG(), "--top", "5"], { timeoutMs: 30000 });
        ctx.ui.notify(out.split("\n").slice(0, 8).join("\n"), "info");
      } else {
        ctx.ui.notify("Usage: /graphiq index | status | search <query>", "warning");
      }
    },
  });
}
