import { app, shell } from 'electron';
import fg from 'fast-glob';
import Database from 'better-sqlite3';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import type {
  EdgeStat,
  ConnectorStatus,
  GraphLink,
  GraphNode,
  IndexTreeItem,
  IndexActionResult,
  IndexDetails,
  IndexSummary,
  IssueDraft,
  RankedSymbol,
  SignalEntry,
  UpdateStatus
} from '@shared/types';

const execFileAsync = promisify(execFile);

type RegistryRecord = {
  projectPath: string;
  manifestPath: string;
  dbPath: string;
  manifestMtimeMs: number;
  dbMtimeMs: number;
  summary: IndexSummary;
};

type Registry = {
  lastAccessed: Record<string, string>;
  indexCache: Record<string, RegistryRecord>;
  trackedProjects: string[];
  lastScanAt: string | null;
};

const REGISTRY_FILE = 'desktop-index-registry.json';
const INDEX_SCAN_CACHE_MS = 15_000;
const ACTIVE_CONNECTOR_WINDOW_MS = 1000 * 60 * 60 * 24;

type ConnectorDefinition = {
  id: string;
  name: string;
  surface: ConnectorStatus['surface'];
  harness: string;
  globalPaths?: (home: string) => string[];
  projectPaths?: (projectPath: string) => string[];
};

const CONNECTORS: ConnectorDefinition[] = [
  {
    id: 'claude-code',
    name: 'Claude Code',
    surface: 'native',
    harness: 'claude-code',
    projectPaths: (projectPath) => [path.join(projectPath, '.claude', '.mcp.json')]
  },
  {
    id: 'claude-desktop',
    name: 'Claude Desktop',
    surface: 'native',
    harness: 'claude-desktop',
    globalPaths: (home) => [
      process.platform === 'darwin'
        ? path.join(home, 'Library', 'Application Support', 'Claude', 'claude_desktop_config.json')
        : path.join(home, '.config', 'Claude', 'claude_desktop_config.json')
    ]
  },
  {
    id: 'opencode',
    name: 'OpenCode',
    surface: 'native',
    harness: 'opencode',
    globalPaths: (home) => [path.join(home, '.config', 'opencode', 'opencode.json')]
  },
  {
    id: 'codex',
    name: 'Codex CLI',
    surface: 'native',
    harness: 'codex',
    globalPaths: (home) => [path.join(home, '.codex', 'config.toml')]
  },
  {
    id: 'cursor',
    name: 'Cursor',
    surface: 'native',
    harness: 'cursor',
    projectPaths: (projectPath) => [path.join(projectPath, '.cursor', 'mcp.json')]
  },
  {
    id: 'windsurf',
    name: 'Windsurf',
    surface: 'native',
    harness: 'windsurf',
    projectPaths: (projectPath) => [path.join(projectPath, '.windsurf', 'mcp.json')]
  },
  {
    id: 'gemini',
    name: 'Gemini CLI',
    surface: 'native',
    harness: 'gemini',
    globalPaths: (home) => [path.join(home, '.gemini', 'settings.json')]
  },
  {
    id: 'hermes',
    name: 'Hermes Agent',
    surface: 'native',
    harness: 'hermes',
    globalPaths: (home) => [path.join(home, '.hermes', 'config.yaml')]
  },
  {
    id: 'aider',
    name: 'Aider',
    surface: 'config',
    harness: 'aider',
    projectPaths: (projectPath) => [path.join(projectPath, '.aider.conf.yml')]
  }
];

function repoRoot(): string {
  if (process.env.GRAPHIQ_REPO_ROOT) {
    return process.env.GRAPHIQ_REPO_ROOT;
  }
  return path.resolve(app.getAppPath(), '..', '..');
}

function registryPath(): string {
  return path.join(app.getPath('userData'), REGISTRY_FILE);
}

function defaultRegistry(): Registry {
  return { lastAccessed: {}, indexCache: {}, trackedProjects: [], lastScanAt: null };
}

async function readRegistry(): Promise<Registry> {
  try {
    const raw = await fs.readFile(registryPath(), 'utf8');
    const parsed = JSON.parse(raw) as Partial<Registry>;
    const lastAccessed = parsed.lastAccessed ?? {};
    const indexCache = parsed.indexCache ?? {};
    const trackedProjects = [
      ...(parsed.trackedProjects ?? []),
      ...Object.keys(indexCache),
      ...Object.keys(lastAccessed)
    ].filter(Boolean);
    return {
      lastAccessed,
      indexCache,
      trackedProjects: [...new Set(trackedProjects)],
      lastScanAt: parsed.lastScanAt ?? null
    };
  } catch {
    return defaultRegistry();
  }
}

async function writeRegistry(registry: Registry): Promise<void> {
  await fs.mkdir(path.dirname(registryPath()), { recursive: true });
  await fs.writeFile(registryPath(), JSON.stringify(registry, null, 2));
}

function friendlyName(projectPath: string): string {
  return path.basename(projectPath);
}

function sortSummaries(summaries: IndexSummary[]): IndexSummary[] {
  return [...summaries].sort((a, b) => {
    const left = a.lastAccessedAt ?? a.lastIndexedAt ?? a.createdAt ?? '';
    const right = b.lastAccessedAt ?? b.lastIndexedAt ?? b.createdAt ?? '';
    return right.localeCompare(left);
  });
}

function withDisambiguatedNames(summaries: IndexSummary[]): IndexSummary[] {
  const home = os.homedir();
  const grouped = new Map<string, IndexSummary[]>();
  for (const summary of summaries) {
    const key = path.basename(summary.projectPath);
    const list = grouped.get(key) ?? [];
    list.push(summary);
    grouped.set(key, list);
  }

  return summaries.map((summary) => {
    const key = path.basename(summary.projectPath);
    const matches = grouped.get(key) ?? [];
    if (matches.length <= 1) {
      return summary;
    }

    const relativePath = summary.projectPath.startsWith(home)
      ? `~${summary.projectPath.slice(home.length)}`
      : summary.projectPath;

    return {
      ...summary,
      name: `${key} - ${relativePath}`
    };
  });
}

function parseEpochSeconds(value: string | null | undefined): string | null {
  if (!value) {
    return null;
  }
  const seconds = Number(value);
  if (Number.isNaN(seconds)) {
    return null;
  }
  return new Date(seconds * 1000).toISOString();
}

async function statInfo(filePath: string): Promise<{ birthtime: string | null; mtimeMs: number }> {
  try {
    const stat = await fs.stat(filePath);
    return {
      birthtime: stat.birthtime.toISOString(),
      mtimeMs: stat.mtimeMs
    };
  } catch {
    return { birthtime: null, mtimeMs: 0 };
  }
}

function openDb(dbPath: string): Database.Database {
  return new Database(dbPath, { readonly: true, fileMustExist: true });
}

async function scanRoots(): Promise<string[]> {
  const home = os.homedir();
  const candidates = [
    repoRoot(),
    path.join(home, 'graphiq'),
    path.join(home, 'signetai'),
    path.join(home, 'Documents'),
    path.join(home, 'Desktop'),
    path.join(home, 'Code'),
    path.join(home, 'Projects'),
    path.join(home, 'Development')
  ];

  const existing = await Promise.all(
    candidates.map(async (candidate) => {
      try {
        const stat = await fs.stat(candidate);
        return stat.isDirectory() ? candidate : null;
      } catch {
        return null;
      }
    })
  );

  return [...new Set(existing.filter(Boolean) as string[])];
}

async function findManifestPaths(extraProjectPaths: string[] = []): Promise<string[]> {
  const roots = await scanRoots();
  const home = os.homedir();
  const explicitManifests = extraProjectPaths.map((projectPath) => path.join(projectPath, '.graphiq', 'manifest.json'));
  const homeChildManifests = await fg('*/.graphiq/manifest.json', {
    cwd: home,
    absolute: true,
    followSymbolicLinks: false,
    deep: 3,
    suppressErrors: true,
    ignore: ['Library/**', '.Trash/**', '.cache/**', '.npm/**', '.cargo/**', '.rustup/**', '.codex/**']
  });
  const found = await Promise.all(
    roots.map((root) =>
      fg('**/.graphiq/manifest.json', {
        cwd: root,
        absolute: true,
        followSymbolicLinks: false,
        deep: 8,
        suppressErrors: true,
        ignore: ['**/.git/**', '**/node_modules/**', '**/target/**', '**/dist/**', '**/out/**']
      })
    )
  );
  return [...new Set([...found.flat(), ...homeChildManifests, ...explicitManifests])];
}

async function indexedProjectPaths(): Promise<string[]> {
  const registry = await readRegistry();
  const manifests = await findManifestPaths(registry.trackedProjects);
  return [...new Set(manifests.map((manifest) => path.dirname(path.dirname(manifest))))];
}

async function fileContainsGraphiq(filePath: string): Promise<{ paired: boolean; mtime: string | null; active: boolean }> {
  try {
    const [content, stat] = await Promise.all([fs.readFile(filePath, 'utf8'), fs.stat(filePath)]);
    const mentionsGraphiq = /graphiq(?:-mcp)?/i.test(content);
    return {
      paired: mentionsGraphiq,
      mtime: stat.mtime.toISOString(),
      active: mentionsGraphiq && Date.now() - stat.mtimeMs < ACTIVE_CONNECTOR_WINDOW_MS
    };
  } catch {
    return { paired: false, mtime: null, active: false };
  }
}

async function connectorPaths(definition: ConnectorDefinition): Promise<string[]> {
  const home = os.homedir();
  const globalPaths = definition.globalPaths?.(home) ?? [];
  const projects = await indexedProjectPaths();
  const projectPaths = projects.flatMap((projectPath) => definition.projectPaths?.(projectPath) ?? []);
  return [...new Set([...globalPaths, ...projectPaths])];
}

export async function listConnectors(): Promise<ConnectorStatus[]> {
  const statuses = await Promise.all(
    CONNECTORS.map(async (connector): Promise<ConnectorStatus> => {
      const paths = await connectorPaths(connector);
      const inspected = await Promise.all(paths.map((filePath) => fileContainsGraphiq(filePath)));
      const connectedPaths = paths.filter((_filePath, index) => inspected[index]?.paired);
      const lastSeenAt =
        inspected
          .map((entry) => entry.mtime)
          .filter(Boolean)
          .sort()
          .at(-1) ?? null;

      return {
        id: connector.id,
        name: connector.name,
        surface: connector.surface,
        status: connectedPaths.length > 0 ? 'connected' : paths.length > 0 ? 'available' : 'missing',
        active: inspected.some((entry) => entry.active),
        configPaths: connectedPaths.length > 0 ? connectedPaths : paths,
        detail:
          connectedPaths.length > 0
            ? `GraphIQ is paired in ${connectedPaths.length} config location${connectedPaths.length === 1 ? '' : 's'}.`
            : paths.length > 0
              ? 'Config location detected; GraphIQ is not paired yet.'
              : 'No known config location found yet.',
        lastSeenAt
      };
    })
  );

  return statuses.sort((a, b) => {
    const connected = Number(b.status === 'connected') - Number(a.status === 'connected');
    if (connected !== 0) {
      return connected;
    }
    return a.name.localeCompare(b.name);
  });
}

async function runSetupForHarness(harness: string): Promise<IndexActionResult> {
  return runGraphiqCommand(['setup', '--skip-index', '--harness', harness]);
}

export async function pairConnector(connectorId: string): Promise<IndexActionResult> {
  const connector = CONNECTORS.find((entry) => entry.id === connectorId);
  if (!connector) {
    return { ok: false, message: `Unknown connector: ${connectorId}` };
  }
  try {
    const result = await runSetupForHarness(connector.harness);
    return {
      ok: result.ok,
      message: `Pairing requested for ${connector.name}.`,
      details: result.details ?? result.message
    };
  } catch (error) {
    return {
      ok: false,
      message: `Failed to pair ${connector.name}.`,
      details: error instanceof Error ? error.message : String(error)
    };
  }
}

function removeGraphiqFromText(content: string): string {
  const lines = content.split('\n');
  const filtered = lines.filter((line) => !/graphiq(?:-mcp)?/i.test(line));
  return filtered.join('\n').replace(/\n{3,}/g, '\n\n');
}

export async function unpairConnector(connectorId: string): Promise<IndexActionResult> {
  const connector = CONNECTORS.find((entry) => entry.id === connectorId);
  if (!connector) {
    return { ok: false, message: `Unknown connector: ${connectorId}` };
  }

  const paths = await connectorPaths(connector);
  let changed = 0;

  for (const filePath of paths) {
    try {
      const content = await fs.readFile(filePath, 'utf8');
      if (!/graphiq(?:-mcp)?/i.test(content)) {
        continue;
      }
      await fs.writeFile(filePath, removeGraphiqFromText(content));
      changed += 1;
    } catch {
      // Missing or unreadable connector files are treated as already unpaired.
    }
  }

  return {
    ok: true,
    message: changed > 0 ? `Unpaired ${connector.name} from ${changed} config file${changed === 1 ? '' : 's'}.` : `${connector.name} was not paired.`
  };
}

function normalizeKinds(rows: Array<{ kind: string; count: number }>): string[] {
  return rows
    .sort((a, b) => b.count - a.count || a.kind.localeCompare(b.kind))
    .slice(0, 6)
    .map((row) => row.kind);
}

function deriveHealth(activeMode: string | null): IndexSummary['health'] {
  if (!activeMode) {
    return 'missing';
  }
  if (activeMode.toLowerCase() === 'graphwalk') {
    return 'ready';
  }
  return 'stale';
}

async function buildIndexSummary(
  manifestPath: string,
  registry: Registry,
  cached?: RegistryRecord
): Promise<RegistryRecord | null> {
  const projectPath = path.dirname(path.dirname(manifestPath));
  const dbPath = path.join(projectPath, '.graphiq', 'graphiq.db');

  try {
    const [manifestStat, dbStat] = await Promise.all([statInfo(manifestPath), statInfo(dbPath)]);

    if (
      cached &&
      cached.manifestMtimeMs === manifestStat.mtimeMs &&
      cached.dbMtimeMs === dbStat.mtimeMs
    ) {
      return {
        ...cached,
        summary: {
          ...cached.summary,
          lastAccessedAt: registry.lastAccessed[projectPath] ?? cached.summary.lastAccessedAt
        }
      };
    }

    const manifestRaw = await fs.readFile(manifestPath, 'utf8');
    const manifest = JSON.parse(manifestRaw) as {
      files?: number;
      symbols?: number;
      edges?: number;
      indexed_at?: string;
      active_search_mode?: string;
    };

    let languages: string[] = [];
    try {
      const db = openDb(dbPath);
      const rows = db
        .prepare('select language as kind, count(*) as count from symbols group by language')
        .all() as Array<{ kind: string; count: number }>;
      db.close();
      languages = normalizeKinds(rows);
    } catch {
      languages = [];
    }

    const summary: IndexSummary = {
      id: projectPath,
      name: friendlyName(projectPath),
      projectPath,
      dbPath,
      manifestPath,
      files: manifest.files ?? 0,
      symbols: manifest.symbols ?? 0,
      edges: manifest.edges ?? 0,
      createdAt: dbStat.birthtime,
      lastIndexedAt: parseEpochSeconds(manifest.indexed_at),
      lastAccessedAt: registry.lastAccessed[projectPath] ?? null,
      activeMode: manifest.active_search_mode ?? null,
      languages,
      health: deriveHealth(manifest.active_search_mode ?? null)
    };

    return {
      projectPath,
      manifestPath,
      dbPath,
      manifestMtimeMs: manifestStat.mtimeMs,
      dbMtimeMs: dbStat.mtimeMs,
      summary
    };
  } catch {
    return null;
  }
}

export async function listIndexes(forceRefresh = false): Promise<IndexSummary[]> {
  const registry = await readRegistry();
  const now = Date.now();
  const lastScanAge = registry.lastScanAt ? now - new Date(registry.lastScanAt).getTime() : Number.POSITIVE_INFINITY;

  if (!forceRefresh && lastScanAge < INDEX_SCAN_CACHE_MS && Object.keys(registry.indexCache).length > 0) {
    const summaries = Object.values(registry.indexCache)
      .map((record) => ({
        ...record.summary,
        lastAccessedAt: registry.lastAccessed[record.projectPath] ?? record.summary.lastAccessedAt
      }));
    return withDisambiguatedNames(sortSummaries(summaries));
  }

  const manifests = await findManifestPaths(registry.trackedProjects);
  const nextCache: Record<string, RegistryRecord> = {};

  for (const manifest of manifests) {
    const projectPath = path.dirname(path.dirname(manifest));
    const cached = registry.indexCache[projectPath];
    const record = await buildIndexSummary(manifest, registry, cached);
    if (record) {
      nextCache[record.projectPath] = record;
    }
  }

  registry.indexCache = nextCache;
  registry.lastScanAt = new Date().toISOString();
  await writeRegistry(registry);

  const summaries = Object.values(nextCache).map((record) => record.summary);
  return withDisambiguatedNames(sortSummaries(summaries));
}

function signalEntriesFromRows(
  rows: Array<{ label: string; value: number | string; supporting: string }>
): SignalEntry[] {
  return rows.map((row) => ({
    label: compactSignalLabel(row.label),
    value: String(row.value),
    supporting: row.supporting
  }));
}

function compactSignalLabel(value: string): string {
  const normalized = value.replace(/\s+/g, ' ').trim();
  const declaration = normalized.match(/\b(?:const|let|var|function|class|type|interface)\s+([A-Za-z_$][\w$]*)/);
  if (declaration?.[1]) {
    return declaration[1];
  }

  const assignment = normalized.match(/^([A-Za-z_$][\w$]*)\s*(?::|=|=>)/);
  if (assignment?.[1]) {
    return assignment[1];
  }

  const member = normalized.match(/^([A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)?)\s*[{(]/);
  if (member?.[1]) {
    return member[1];
  }

  return normalized.length > 72 ? `${normalized.slice(0, 69)}...` : normalized;
}

function topSignals(db: Database.Database): EdgeStat[] {
  const callsCount = db
    .prepare("select count(*) as count from edges where lower(kind) like '%call%'")
    .get() as { count: number };
  const importsCount = db
    .prepare("select count(*) as count from edges where lower(kind) like '%import%'")
    .get() as { count: number };
  const constantsCount = db
    .prepare("select count(*) as count from symbols where lower(kind) like '%const%'")
    .get() as { count: number };

  const topCallers = db
    .prepare(
      `select s.name as label, count(*) as value, f.path as supporting
       from edges e
       join symbols s on s.id = e.source_id
       join files f on f.id = s.file_id
       where lower(e.kind) like '%call%'
       group by e.source_id
       order by count(*) desc, s.importance desc
       limit 5`
    )
    .all() as Array<{ label: string; value: number; supporting: string }>;

  const topImporters = db
    .prepare(
      `select s.name as label, count(*) as value, f.path as supporting
       from edges e
       join symbols s on s.id = e.source_id
       join files f on f.id = s.file_id
       where lower(e.kind) like '%import%'
       group by e.source_id
       order by count(*) desc, s.importance desc
       limit 5`
    )
    .all() as Array<{ label: string; value: number; supporting: string }>;

  const topConstants = db
    .prepare(
      `select s.name as label, printf('%.2f', s.importance) as value, f.path as supporting
       from symbols s
       join files f on f.id = s.file_id
       where lower(s.kind) like '%const%'
       order by s.importance desc, s.name asc
       limit 5`
    )
    .all() as Array<{ label: string; value: string; supporting: string }>;

  return [
    {
      label: 'Calls',
      count: callsCount.count,
      detail: 'Executable relationships and orchestration pressure across the graph',
      entries: signalEntriesFromRows(topCallers)
    },
    {
      label: 'Imports',
      count: importsCount.count,
      detail: 'Module dependencies, boundary wiring, and import reach across the graph',
      entries: signalEntriesFromRows(topImporters)
    },
    {
      label: 'Constants',
      count: constantsCount.count,
      detail: 'Shared values, fixed configuration points, and embedded system signals',
      entries: signalEntriesFromRows(topConstants)
    }
  ];
}

function rankedSymbols(db: Database.Database): RankedSymbol[] {
  return db
    .prepare(
      `select
        s.id,
        s.name,
        s.qualified_name as qualifiedName,
        s.kind,
        s.importance,
        f.path as filePath,
        s.language,
        (select count(*) from edges e where e.target_id = s.id) as inbound,
        (select count(*) from edges e where e.source_id = s.id) as outbound
       from symbols s
       join files f on f.id = s.file_id
       order by s.importance desc, inbound desc, outbound desc, s.name asc
       limit 28`
    )
    .all() as RankedSymbol[];
}

function graphSnapshot(db: Database.Database) {
  const edges = db
    .prepare(
      `select
        e.source_id as source,
        e.target_id as target,
        e.kind,
        e.weight,
        s1.name as sourceName,
        s1.kind as sourceKind,
        s1.importance as sourceImportance,
        s1.language as sourceLanguage,
        (select count(*) from edges e2 where e2.target_id = s1.id) as sourceInbound,
        (select count(*) from edges e2 where e2.source_id = s1.id) as sourceOutbound,
        s2.name as targetName,
        s2.kind as targetKind,
        s2.importance as targetImportance,
        s2.language as targetLanguage,
        (select count(*) from edges e3 where e3.target_id = s2.id) as targetInbound,
        (select count(*) from edges e3 where e3.source_id = s2.id) as targetOutbound
       from edges e
       join symbols s1 on s1.id = e.source_id
       join symbols s2 on s2.id = e.target_id
       order by (s1.importance + s2.importance) desc, e.weight desc
       limit 60`
    )
    .all() as Array<{
      source: number;
      target: number;
      kind: string;
      weight: number;
      sourceName: string;
      sourceKind: string;
      sourceImportance: number;
      sourceLanguage: string;
      sourceInbound: number;
      sourceOutbound: number;
      targetName: string;
      targetKind: string;
      targetImportance: number;
      targetLanguage: string;
      targetInbound: number;
      targetOutbound: number;
    }>;

  const nodeMap = new Map<number, GraphNode>();
  const edgeKinds = new Map<string, number>();

  for (const edge of edges) {
    edgeKinds.set(edge.kind, (edgeKinds.get(edge.kind) ?? 0) + 1);
    nodeMap.set(edge.source, {
      id: edge.source,
      name: edge.sourceName,
      kind: edge.sourceKind,
      importance: edge.sourceImportance,
      inbound: edge.sourceInbound,
      outbound: edge.sourceOutbound,
      language: edge.sourceLanguage
    });
    nodeMap.set(edge.target, {
      id: edge.target,
      name: edge.targetName,
      kind: edge.targetKind,
      importance: edge.targetImportance,
      inbound: edge.targetInbound,
      outbound: edge.targetOutbound,
      language: edge.targetLanguage
    });
  }

  return {
    nodes: [...nodeMap.values()].sort((a, b) => b.importance - a.importance).slice(0, 24),
    links: edges.map(
      (edge): GraphLink => ({
        source: edge.source,
        target: edge.target,
        kind: edge.kind,
        weight: edge.weight
      })
    ),
    edgeKinds: [...edgeKinds.entries()]
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .map(([kind]) => kind)
  };
}

function extensionFromPath(filePath: string): string {
  const extension = path.extname(filePath).replace(/^\./, '');
  return extension || 'no extension';
}

function indexTree(db: Database.Database): IndexTreeItem[] {
  const rows = db
    .prepare(
      `select
        f.id,
        f.path,
        f.language,
        f.line_count as lineCount,
        count(distinct s.id) as symbolCount,
        count(distinct incoming.id) as inbound,
        count(distinct outgoing.id) as outbound
       from files f
       left join symbols s on s.file_id = f.id
       left join file_edges incoming on incoming.target_file_id = f.id
       left join file_edges outgoing on outgoing.source_file_id = f.id
       group by f.id
       order by f.path asc`
    )
    .all() as Array<{
      id: number;
      path: string;
      language: string;
      lineCount: number;
      symbolCount: number;
      inbound: number;
      outbound: number;
    }>;

  const groups = new Map<string, IndexTreeItem>();

  for (const row of rows) {
    const extension = extensionFromPath(row.path);
    const groupId = `extension:${extension}`;
    const fileItem: IndexTreeItem = {
      id: `file:${row.id}`,
      type: 'file',
      name: path.basename(row.path),
      path: row.path,
      extension,
      language: row.language,
      fileCount: 1,
      lineCount: row.lineCount,
      symbolCount: row.symbolCount,
      inbound: row.inbound,
      outbound: row.outbound
    };

    const group = groups.get(groupId);
    if (group) {
      group.fileCount += 1;
      group.lineCount += row.lineCount;
      group.symbolCount += row.symbolCount;
      group.inbound += row.inbound;
      group.outbound += row.outbound;
      group.children?.push(fileItem);
    } else {
      groups.set(groupId, {
        id: groupId,
        type: 'extension',
        name: `.${extension}`,
        path: null,
        extension,
        language: row.language,
        fileCount: 1,
        lineCount: row.lineCount,
        symbolCount: row.symbolCount,
        inbound: row.inbound,
        outbound: row.outbound,
        children: [fileItem]
      });
    }
  }

  return [...groups.values()]
    .map((group) => ({
      ...group,
      children: group.children?.sort((a, b) => b.symbolCount - a.symbolCount || a.name.localeCompare(b.name)).slice(0, 30)
    }))
    .sort((a, b) => b.symbolCount - a.symbolCount || a.name.localeCompare(b.name))
    .slice(0, 12);
}

export async function getIndexDetails(projectPath: string): Promise<IndexDetails> {
  const indexes = await listIndexes();
  let summary = indexes.find((entry) => entry.projectPath === projectPath);
  if (!summary) {
    const registry = await readRegistry();
    const manifestPath = path.join(projectPath, '.graphiq', 'manifest.json');
    const record = await buildIndexSummary(manifestPath, registry, registry.indexCache[projectPath]);
    if (!record) {
      throw new Error(`Index not found for ${projectPath}`);
    }
    registry.indexCache[projectPath] = record;
    registry.trackedProjects = [...new Set([...registry.trackedProjects, projectPath])];
    await writeRegistry(registry);
    summary = record.summary;
  }

  const db = openDb(summary.dbPath);
  const details: IndexDetails = {
    summary,
    graph: graphSnapshot(db),
    tree: indexTree(db),
    signals: topSignals(db),
    rankedSymbols: rankedSymbols(db)
  };
  db.close();

  const registry = await readRegistry();
  registry.lastAccessed[projectPath] = new Date().toISOString();
  if (registry.indexCache[projectPath]) {
    registry.indexCache[projectPath].summary.lastAccessedAt = registry.lastAccessed[projectPath];
  }
  await writeRegistry(registry);

  return {
    ...details,
    summary: {
      ...details.summary,
      lastAccessedAt: registry.lastAccessed[projectPath]
    }
  };
}

async function runGraphiqCommand(args: string[]): Promise<IndexActionResult> {
  const root = repoRoot();
  const binaryPath = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'graphiq.exe' : 'graphiq');

  try {
    await fs.stat(binaryPath);
    const { stdout, stderr } = await execFileAsync(binaryPath, args, { cwd: root });
    return {
      ok: true,
      message: stdout.trim() || 'Command completed successfully.',
      details: stderr.trim() || undefined
    };
  } catch {
    const { stdout, stderr } = await execFileAsync('cargo', ['run', '-p', 'graphiq-cli', '--', ...args], {
      cwd: root,
      maxBuffer: 1024 * 1024 * 16
    });
    return {
      ok: true,
      message: stdout.trim() || 'Command completed successfully.',
      details: stderr.trim() || undefined
    };
  }
}

export async function indexProject(projectPath: string): Promise<IndexActionResult> {
  try {
    const result = await runGraphiqCommand(['index', projectPath]);
    const registry = await readRegistry();
    registry.trackedProjects = [...new Set([...registry.trackedProjects, projectPath])];
    registry.lastScanAt = null;
    await writeRegistry(registry);
    return result;
  } catch (error) {
    return {
      ok: false,
      message: `Indexing failed for ${projectPath}`,
      details: error instanceof Error ? error.message : String(error)
    };
  }
}

export async function deleteIndex(projectPath: string): Promise<IndexActionResult> {
  try {
    await fs.rm(path.join(projectPath, '.graphiq'), { recursive: true, force: true });
    const registry = await readRegistry();
    delete registry.lastAccessed[projectPath];
    delete registry.indexCache[projectPath];
    registry.trackedProjects = registry.trackedProjects.filter((entry) => entry !== projectPath);
    await writeRegistry(registry);
    return {
      ok: true,
      message: `Deleted GraphIQ index for ${friendlyName(projectPath)}.`
    };
  } catch (error) {
    return {
      ok: false,
      message: `Failed to delete index for ${projectPath}`,
      details: error instanceof Error ? error.message : String(error)
    };
  }
}

export async function checkForUpdates(): Promise<UpdateStatus> {
  const response = await fetch('https://api.github.com/repos/aaf2tbz/graphiq/releases/latest', {
    headers: { 'User-Agent': 'graphiq-desktop' }
  });
  const release = (await response.json()) as { tag_name?: string; html_url?: string; body?: string };
  const currentVersion = `dev-${(await execFileAsync('git', ['rev-parse', '--short', 'HEAD'], { cwd: repoRoot() })).stdout.trim()}`;
  return {
    currentVersion,
    latestVersion: release.tag_name ?? null,
    updateAvailable: Boolean(release.tag_name && !currentVersion.includes(release.tag_name)),
    releaseUrl: release.html_url ?? 'https://github.com/aaf2tbz/graphiq/releases',
    notes: release.body ?? ''
  };
}

export async function pullLatestMain(): Promise<IndexActionResult> {
  try {
    const { stdout, stderr } = await execFileAsync('git', ['pull', '--ff-only', 'origin', 'main'], {
      cwd: repoRoot()
    });
    return {
      ok: true,
      message: stdout.trim() || 'Pulled the latest GraphIQ main.',
      details: stderr.trim() || undefined
    };
  } catch (error) {
    return {
      ok: false,
      message: 'Update pull failed.',
      details: error instanceof Error ? error.message : String(error)
    };
  }
}

export async function uninstallApp(): Promise<IndexActionResult> {
  if (!app.isPackaged) {
    return {
      ok: false,
      message: 'Uninstall is only available for packaged builds.',
      details: 'This desktop shell is currently running from a development repo.'
    };
  }

  return {
    ok: false,
    message: 'Packaged uninstall flow is not implemented yet.',
    details: 'This needs an OS-specific helper so the app can remove itself safely.'
  };
}

export async function submitIssue(draft: IssueDraft): Promise<IndexActionResult> {
  const body = [
    '## Summary',
    draft.actual,
    '',
    '## Expected',
    draft.expected,
    '',
    '## Reproduction',
    draft.reproduction,
    '',
    '## Environment',
    draft.environment
  ].join('\n');

  const url = new URL('https://github.com/aaf2tbz/graphiq/issues/new');
  url.searchParams.set('title', draft.title);
  url.searchParams.set('body', body);
  await shell.openExternal(url.toString());

  return {
    ok: true,
    message: 'Opened a pre-filled GitHub issue draft in your browser.'
  };
}
