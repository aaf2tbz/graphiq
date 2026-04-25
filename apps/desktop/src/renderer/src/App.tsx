import { useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity,
  AlertCircle,
  Boxes,
  Cable,
  ChevronDown,
  ChevronRight,
  FileCode2,
  Filter,
  FolderTree,
  GitBranch,
  GitCommitHorizontal,
  Moon,
  Network,
  Orbit,
  PlayCircle,
  RefreshCcw,
  Search,
  Settings as SettingsIcon,
  Sparkles,
  Sun,
  Trash2
} from 'lucide-react';
import graphiqIconUrl from './assets/graphiq-icon.png';
import type {
  ConnectorStatus,
  DesktopApi,
  GraphNode,
  IndexActionResult,
  IndexDetails,
  IndexSummary,
  IndexTreeItem,
  IssueDraft,
  UpdateStatus
} from '@shared/types';

type Page = 'indexes' | 'ontology' | 'connectors' | 'settings';
type GraphMode = 'constellation' | 'orbit';
type Theme = 'light' | 'dark';
type IndexPanelKey = 'tree' | 'related' | 'neighbors' | 'summary';

const pageCopy: Record<Exclude<Page, 'settings'>, { kicker: string; title: string; summary: string }> = {
  indexes: {
    kicker: 'Index Workspace',
    title: 'Indexes',
    summary: 'Inspect project structure, grouped files, and ranked symbols from the active graph.'
  },
  ontology: {
    kicker: 'Graph Topology',
    title: 'Interactive Topology',
    summary: 'Explore central symbols, edge pressure, and structural relationships in the graph.'
  },
  connectors: {
    kicker: 'Harness Control',
    title: 'Connectors',
    summary: 'Pair GraphIQ with installed agent harnesses and monitor their connection state.'
  }
};

const issueTemplate: IssueDraft = {
  title: '',
  expected: '',
  actual: '',
  reproduction: '',
  environment: ''
};

function getDesktopApi() {
  const api = window.graphiq as DesktopApi | undefined;
  if (!api) {
    throw new Error('Desktop bridge unavailable. Restart the app so the Electron preload can attach cleanly.');
  }
  return api;
}

function formatDate(value: string | null) {
  if (!value) {
    return 'Unknown';
  }
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short'
  }).format(new Date(value));
}

function initials(name: string) {
  return name
    .split(/[\s-_]+/)
    .map((part) => part[0]?.toUpperCase())
    .join('')
    .slice(0, 2);
}

function healthLabel(health: IndexSummary['health']) {
  if (health === 'ready') {
    return 'Ready';
  }
  if (health === 'stale') {
    return 'Fallback';
  }
  return 'Missing';
}

function truncateText(value: string, max = 64) {
  const normalized = value.replace(/\s+/g, ' ').trim();
  return normalized.length > max ? `${normalized.slice(0, max - 3)}...` : normalized;
}

function compactLog(value: string, maxLines = 10, maxLineLength = 140) {
  const lines = value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  const tail = lines.slice(-maxLines);
  return tail.map((line) => truncateText(line, maxLineLength)).join('\n');
}

function LoadingDots() {
  return (
    <span className="loading-dots" aria-hidden="true">
      <span>.</span>
      <span>.</span>
      <span>.</span>
    </span>
  );
}

function formatElapsedLabel(since: number | null, now: number) {
  if (!since) {
    return '';
  }
  const elapsedSeconds = Math.max(0, Math.floor((now - since) / 1000));
  if (elapsedSeconds < 60) {
    return `${elapsedSeconds}s`;
  }
  const minutes = Math.floor(elapsedSeconds / 60);
  const seconds = elapsedSeconds % 60;
  return `${minutes}m ${seconds.toString().padStart(2, '0')}s`;
}

function LoadingElapsed({ since }: { since: number | null }) {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!since) {
      return undefined;
    }
    setNow(Date.now());
    const interval = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(interval);
  }, [since]);

  if (!since) {
    return null;
  }

  return <span className="loading-elapsed">{formatElapsedLabel(since, now)}</span>;
}

type ConstellationNode = {
  id: string | number;
  label: string;
  importance: number;
};

type ConstellationLink = {
  source: string | number;
  target: string | number;
  weight: number;
};

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function constrainPointToCircle(x: number, y: number, center: number, radius: number) {
  const dx = x - center;
  const dy = y - center;
  const distance = Math.hypot(dx, dy);
  if (distance <= radius) {
    return { x, y };
  }
  const scale = radius / Math.max(distance, 0.001);
  return {
    x: center + dx * scale,
    y: center + dy * scale
  };
}

function describeDomain(label: string, filePath?: string | null) {
  const haystack = `${label} ${filePath ?? ''}`.toLowerCase();
  if (/(parse|parser|ast|token|syntax)/.test(haystack)) {
    return 'parsing and syntax translation';
  }
  if (/(index|search|query|rank|match|retriev)/.test(haystack)) {
    return 'indexing and retrieval';
  }
  if (/(open|load|read|decode|import)/.test(haystack)) {
    return 'loading and intake';
  }
  if (/(write|save|persist|store|cache|db|sqlite|memory)/.test(haystack)) {
    return 'state, storage, or memory flow';
  }
  if (/(build|compile|generate|emit|render)/.test(haystack)) {
    return 'build and generation work';
  }
  if (/(handle|route|serve|request|http|api)/.test(haystack)) {
    return 'request handling and routing';
  }
  if (/(config|setup|init|boot|start)/.test(haystack)) {
    return 'startup and configuration';
  }
  if (/(alias|name|symbol|describe|explain)/.test(haystack)) {
    return 'naming, symbol shaping, or explanation';
  }
  return 'general graph orchestration';
}

function describeRole(inbound: number, outbound: number) {
  if (inbound >= 18 && outbound >= 18) {
    return 'a central relay point that both receives and redistributes structural pressure';
  }
  if (outbound >= inbound * 1.8 && outbound >= 8) {
    return 'a dispatcher that fans work outward into the graph';
  }
  if (inbound >= outbound * 1.8 && inbound >= 8) {
    return 'a shared dependency that many other symbols lean on';
  }
  if (inbound + outbound <= 6) {
    return 'a leaf-level helper with a narrow local role';
  }
  return 'a mid-graph connector that bridges nearby graph regions';
}

function summarizeStructuralNode(node: GraphNode, activeKind: string) {
  const domain = describeDomain(node.name);
  const role = describeRole(node.inbound, node.outbound);
  return `${truncateText(node.name, 36)} looks like ${node.kind} work focused on ${domain}. In this view it behaves like ${role}${activeKind === 'all' ? '.' : ` while filtering to ${activeKind} edges.`}`;
}

function summarizeRankedSymbol(symbol: IndexDetails['rankedSymbols'][number]) {
  const domain = describeDomain(symbol.name, symbol.filePath);
  const role = describeRole(symbol.inbound, symbol.outbound);
  return `${truncateText(symbol.name, 36)} is likely involved in ${domain}. Its ranking suggests it acts like ${role} inside ${truncateText(symbol.filePath, 52)}.`;
}

function summarizeSignalEntry(signal: IndexDetails['signals'][number], entry: IndexDetails['signals'][number]['entries'][number] | null) {
  if (!entry) {
    return signal.detail;
  }
  const domain = describeDomain(entry.label, entry.supporting);
  return `${truncateText(entry.label, 32)} is a ${signal.label.toLowerCase()}-weighted symbol tied to ${domain}. ${signal.detail}`;
}

function groupedSymbols(symbols: IndexDetails['rankedSymbols']) {
  return symbols.reduce<Record<string, IndexDetails['rankedSymbols']>>((groups, symbol) => {
    const key = symbol.language || symbol.kind || 'mixed';
    groups[key] = [...(groups[key] ?? []), symbol];
    return groups;
  }, {});
}

function findTreeSelection(items: IndexTreeItem[], selectedId: string | null) {
  for (const item of items) {
    if (item.id === selectedId) {
      return { item, parent: null as IndexTreeItem | null };
    }
    for (const child of item.children ?? []) {
      if (child.id === selectedId) {
        return { item: child, parent: item };
      }
    }
  }
  return null;
}

function matchesTreeSelection(symbol: IndexDetails['rankedSymbols'][number], item: IndexTreeItem, parent: IndexTreeItem | null) {
  if (item.type === 'file') {
    return symbol.filePath === item.path;
  }
  const extension = item.extension.startsWith('.') ? item.extension : `.${item.extension}`;
  return symbol.filePath.endsWith(extension) || symbol.language.toLowerCase() === item.language.toLowerCase();
}

function nearbyFilesForSelection(selection: { item: IndexTreeItem; parent: IndexTreeItem | null } | null) {
  const selectedItem = selection?.item ?? null;
  const parent = selection?.parent ?? null;
  if (!selectedItem) {
    return [];
  }
  if (selectedItem.type === 'extension') {
    return [...(selectedItem.children ?? [])]
      .sort((left, right) => right.symbolCount - left.symbolCount || right.lineCount - left.lineCount)
      .slice(0, 12);
  }
  return (parent?.children ?? [])
    .filter((child) => child.id !== selectedItem.id)
    .sort((left, right) => right.inbound + right.outbound - (left.inbound + left.outbound) || right.symbolCount - left.symbolCount)
    .slice(0, 12);
}

function PageHeader({ page }: { page: Exclude<Page, 'settings'> }) {
  const copy = pageCopy[page];
  return (
    <header className="page-title-panel">
      <div>
        <p className="eyebrow">{copy.kicker}</p>
        <h1>{copy.title}</h1>
      </div>
      <p>{copy.summary}</p>
    </header>
  );
}

function defaultIndexPanels() {
  return new Set<IndexPanelKey>(['tree', 'summary']);
}

function graphLayout(
  nodes: GraphNode[],
  links: IndexDetails['graph']['links'],
  graphMode: GraphMode,
  size: number
) {
  const center = size / 2;
  const core = [...nodes].sort((a, b) => b.importance - a.importance)[0] ?? null;
  const points = new Map<number, { x: number; y: number; vx: number; vy: number }>();

  nodes.forEach((node, index) => {
    if (core && node.id === core.id) {
      points.set(node.id, { x: center, y: center, vx: 0, vy: 0 });
      return;
    }
    const angle = (index / Math.max(nodes.length - 1, 1)) * Math.PI * 2 - Math.PI / 2;
    const radius = graphMode === 'orbit' ? 118 + (index % 3) * 28 : 96 + Math.sqrt(index + 1) * 28;
    points.set(node.id, {
      x: center + Math.cos(angle) * radius,
      y: center + Math.sin(angle) * radius,
      vx: 0,
      vy: 0
    });
  });

  for (let tick = 0; tick < 90; tick += 1) {
    for (let i = 0; i < nodes.length; i += 1) {
      const a = points.get(nodes[i].id);
      if (!a) {
        continue;
      }
      for (let j = i + 1; j < nodes.length; j += 1) {
        const b = points.get(nodes[j].id);
        if (!b) {
          continue;
        }
        const dx = a.x - b.x || 0.01;
        const dy = a.y - b.y || 0.01;
        const distanceSq = Math.max(dx * dx + dy * dy, 120);
        const push = (graphMode === 'orbit' ? 520 : 760) / distanceSq;
        a.vx += dx * push;
        a.vy += dy * push;
        b.vx -= dx * push;
        b.vy -= dy * push;
      }
    }

    for (const link of links) {
      const source = points.get(link.source);
      const target = points.get(link.target);
      if (!source || !target) {
        continue;
      }
      const dx = target.x - source.x;
      const dy = target.y - source.y;
      const distance = Math.max(Math.hypot(dx, dy), 1);
      const targetLength = graphMode === 'orbit' ? 88 : 116;
      const pull = ((distance - targetLength) / distance) * 0.018 * Math.max(link.weight, 0.4);
      source.vx += dx * pull;
      source.vy += dy * pull;
      target.vx -= dx * pull;
      target.vy -= dy * pull;
    }

    for (const node of nodes) {
      const point = points.get(node.id);
      if (!point) {
        continue;
      }
      const isCore = core && node.id === core.id;
      if (isCore) {
        point.x = center;
        point.y = center;
        point.vx = 0;
        point.vy = 0;
        continue;
      }
      const centerPull = graphMode === 'orbit' ? 0.026 : 0.014;
      point.vx += (center - point.x) * centerPull;
      point.vy += (center - point.y) * centerPull;
      point.vx *= 0.72;
      point.vy *= 0.72;
      point.x = Math.min(size - 42, Math.max(42, point.x + point.vx));
      point.y = Math.min(size - 42, Math.max(42, point.y + point.vy));
    }
  }

  return points;
}

function GraphCanvas({
  details,
  selectedNodeId,
  activeKind,
  graphMode,
  onSelectNode,
  onGraphMode,
  onKindChange
}: {
  details: IndexDetails;
  selectedNodeId: number | null;
  activeKind: string;
  graphMode: GraphMode;
  onSelectNode: (id: number | null) => void;
  onGraphMode: (mode: GraphMode) => void;
  onKindChange: (kind: string) => void;
}) {
  const size = 460;
  const center = size / 2;
  const baseRadius = graphMode === 'constellation' ? 138 : 118;
  const visibleLinks = details.graph.links.filter((link) => activeKind === 'all' || link.kind === activeKind);
  const visibleNodeIds = new Set<number>();
  visibleLinks.forEach((link) => {
    visibleNodeIds.add(link.source);
    visibleNodeIds.add(link.target);
  });
  const nodes = details.graph.nodes.filter((node) => visibleNodeIds.size === 0 || visibleNodeIds.has(node.id));
  const positions = useMemo(() => graphLayout(nodes, visibleLinks, graphMode, size), [nodes, visibleLinks, graphMode]);

  const selectedNode = details.graph.nodes.find((node) => node.id === selectedNodeId) ?? null;
  const coreNode = nodes[0] ?? null;

  return (
    <div className="graph-card">
      <div className="section-header">
        <div>
          <p className="eyebrow">Structural Graph</p>
          <h3>Interactive topology view</h3>
        </div>
        <div className="section-actions">
          <button
            className={`segmented ${graphMode === 'constellation' ? 'active' : ''}`}
            onClick={() => onGraphMode('constellation')}
          >
            <Network size={15} />
            Constellation
          </button>
          <button className={`segmented ${graphMode === 'orbit' ? 'active' : ''}`} onClick={() => onGraphMode('orbit')}>
            <Orbit size={15} />
            Orbit
          </button>
          <span className="pill">{nodes.length} visible</span>
        </div>
      </div>

      <div className="graph-toolbar">
        <label className="compact-selector">
          <Filter size={14} />
          <select
            value={activeKind}
            onChange={(event) => {
              onSelectNode(null);
              onKindChange(event.target.value);
            }}
          >
            <option value="all">All edge kinds</option>
            {details.graph.edgeKinds.map((kind) => (
              <option key={kind} value={kind}>
                {kind}
              </option>
            ))}
          </select>
        </label>
      </div>

      <svg className="graph-canvas" viewBox={`0 0 ${size} ${size}`}>
        <defs>
          <linearGradient id="graph-stroke" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stopColor="#71f0d1" />
            <stop offset="100%" stopColor="#4a8dff" />
          </linearGradient>
        </defs>
        <circle cx={center} cy={center} r={baseRadius + 48} className="graph-orbit" />
        {visibleLinks.map((link, index) => {
          const source = positions.get(link.source);
          const target = positions.get(link.target);
          if (!source || !target) {
            return null;
          }
          const selected = selectedNodeId != null && (link.source === selectedNodeId || link.target === selectedNodeId);
          return (
            <line
              key={`${link.source}-${link.target}-${index}`}
              x1={source.x}
              y1={source.y}
              x2={target.x}
              y2={target.y}
              className={`graph-link ${selected ? 'is-selected' : ''}`}
            />
          );
        })}
        {nodes.map((node) => {
          const point = positions.get(node.id);
          if (!point) {
            return null;
          }
          const selected = selectedNodeId === node.id;
          const isCore = coreNode?.id === node.id;
          return (
            <g key={node.id} transform={`translate(${point.x}, ${point.y})`} onClick={() => onSelectNode(selected ? null : node.id)}>
              <circle className={`graph-node ${selected ? 'is-selected' : ''} ${isCore ? 'is-core' : ''}`} r={15 + node.importance * 5} />
              <text className="graph-label" textAnchor="middle" y={40}>
                {truncateText(node.name, 14)}
              </text>
            </g>
          );
        })}
      </svg>

      <div className="node-focus">
        <div>
          <p className="eyebrow">Focused Node</p>
          <h4>{selectedNode?.name ?? 'Select a node in the graph'}</h4>
        </div>
        {selectedNode ? (
          <div className="focus-grid">
            <span>{selectedNode.kind}</span>
            <span>{selectedNode.language ?? 'mixed'}</span>
            <span>{selectedNode.inbound} inbound</span>
            <span>{selectedNode.outbound} outbound</span>
          </div>
        ) : (
          <p className="muted-copy">Click any node to inspect its local role and edge pressure.</p>
        )}
      </div>
    </div>
  );
}

function constellationLayout(
  nodes: ConstellationNode[],
  links: ConstellationLink[],
  size: number,
  variant: 'feature' | 'mini' = 'feature'
) {
  const center = size / 2;
  const padding = variant === 'feature' ? 28 : 20;
  const boundaryRadius = center - padding;
  const outerRadius = size * (variant === 'feature' ? 0.34 : 0.31);
  const repulsionBase = variant === 'feature' ? 420 : 260;
  const linkLength = variant === 'feature' ? 64 : 42;
  const ordered = [...nodes].sort((a, b) => b.importance - a.importance);
  const core = ordered[0] ?? null;
  const maxImportance = ordered[0]?.importance ?? 1;
  const minImportance = ordered.at(-1)?.importance ?? 0;
  const spread = Math.max(maxImportance - minImportance, 0.001);
  const points = new Map<ConstellationNode['id'], { x: number; y: number; vx: number; vy: number }>();
  const anchors = new Map<ConstellationNode['id'], { angle: number; radius: number }>();

  ordered.forEach((node, index) => {
    if (core && node.id === core.id) {
      points.set(node.id, { x: center, y: center, vx: 0, vy: 0 });
      anchors.set(node.id, { angle: -Math.PI / 2, radius: 0 });
      return;
    }
    const weakness = (maxImportance - node.importance) / spread;
    const radius = 34 + weakness * outerRadius;
    const angle = ((index - 1) / Math.max(ordered.length - 1, 1)) * Math.PI * 2 - Math.PI / 2;
    anchors.set(node.id, { angle, radius });
    points.set(node.id, {
      x: center + Math.cos(angle) * radius,
      y: center + Math.sin(angle) * radius,
      vx: 0,
      vy: 0
    });
  });

  for (let tick = 0; tick < 120; tick += 1) {
    for (let i = 0; i < ordered.length; i += 1) {
      const a = points.get(ordered[i].id);
      if (!a) {
        continue;
      }
      for (let j = i + 1; j < ordered.length; j += 1) {
        const b = points.get(ordered[j].id);
        if (!b) {
          continue;
        }
        const dx = a.x - b.x || 0.01;
        const dy = a.y - b.y || 0.01;
        const distanceSq = Math.max(dx * dx + dy * dy, 90);
        const push = repulsionBase / distanceSq;
        a.vx += dx * push;
        a.vy += dy * push;
        b.vx -= dx * push;
        b.vy -= dy * push;
      }
    }

    for (const link of links) {
      const source = points.get(link.source);
      const target = points.get(link.target);
      if (!source || !target) {
        continue;
      }
      const dx = target.x - source.x;
      const dy = target.y - source.y;
      const distance = Math.max(Math.hypot(dx, dy), 1);
      const pull = ((distance - linkLength) / distance) * 0.02 * Math.max(link.weight, 0.35);
      source.vx += dx * pull;
      source.vy += dy * pull;
      target.vx -= dx * pull;
      target.vy -= dy * pull;
    }

    for (const node of ordered) {
      const point = points.get(node.id);
      const anchor = anchors.get(node.id);
      if (!point || !anchor) {
        continue;
      }
      if (core && node.id === core.id) {
        point.x = center;
        point.y = center;
        point.vx = 0;
        point.vy = 0;
        continue;
      }

      const targetX = center + Math.cos(anchor.angle) * anchor.radius;
      const targetY = center + Math.sin(anchor.angle) * anchor.radius;
      point.vx += (targetX - point.x) * 0.08;
      point.vy += (targetY - point.y) * 0.08;
      point.vx += (center - point.x) * 0.006;
      point.vy += (center - point.y) * 0.006;
      point.vx *= 0.76;
      point.vy *= 0.76;
      const next = constrainPointToCircle(point.x + point.vx, point.y + point.vy, center, boundaryRadius);
      point.x = next.x;
      point.y = next.y;
    }
  }

  return { points, core };
}

function ConstellationScene({
  nodes,
  links,
  selectedId,
  onSelect,
  variant = 'feature',
  emptyMessage
}: {
  nodes: ConstellationNode[];
  links: ConstellationLink[];
  selectedId: string | number | null;
  onSelect: (id: string | number) => void;
  variant?: 'feature' | 'mini';
  emptyMessage?: string;
}) {
  const size = variant === 'feature' ? 356 : 214;
  const center = size / 2;
  const svgRef = useRef<SVGSVGElement | null>(null);
  const rafRef = useRef<number | null>(null);
  const dragRef = useRef<string | number | null>(null);
  const [, setFrame] = useState(0);
  const seed = useMemo(() => constellationLayout(nodes, links, size, variant), [nodes, links, size, variant]);
  const core = seed.core;
  const pointsRef = useRef(
    new Map<
      string | number,
      { x: number; y: number; vx: number; vy: number; anchorX: number; anchorY: number; phase: number; radius: number }
    >()
  );

  useEffect(() => {
    const next = new Map<
      string | number,
      { x: number; y: number; vx: number; vy: number; anchorX: number; anchorY: number; phase: number; radius: number }
    >();
    nodes.forEach((node, index) => {
      const point = seed.points.get(node.id);
      if (!point) {
        return;
      }
      next.set(node.id, {
        x: point.x,
        y: point.y,
        vx: 0,
        vy: 0,
        anchorX: point.x,
        anchorY: point.y,
        phase: index * 0.65,
        radius: variant === 'feature' ? 4 + node.importance * 2.4 : 3 + node.importance * 1.7
      });
    });
    pointsRef.current = next;
    setFrame((value) => value + 1);
  }, [nodes, seed.points, variant]);

  useEffect(() => {
    let cancelled = false;
    const border = variant === 'feature' ? 22 : 16;
    const boundaryRadius = center - border;
    const wobble = variant === 'feature' ? 1.25 : 0.7;

    const step = (time: number) => {
      if (cancelled) {
        return;
      }
      const points = pointsRef.current;
      const entries = [...points.entries()];

      for (let i = 0; i < entries.length; i += 1) {
        const [idA, a] = entries[i];
        for (let j = i + 1; j < entries.length; j += 1) {
          const [, b] = entries[j];
          const dx = a.x - b.x || 0.01;
          const dy = a.y - b.y || 0.01;
          const distanceSq = Math.max(dx * dx + dy * dy, 95);
          const push = (variant === 'feature' ? 130 : 76) / distanceSq;
          a.vx += dx * push;
          a.vy += dy * push;
          b.vx -= dx * push;
          b.vy -= dy * push;
        }

        const isCore = core?.id === idA;
        if (isCore || dragRef.current === idA) {
          continue;
        }

        const wobbleX = Math.cos(time / 1100 + a.phase) * wobble;
        const wobbleY = Math.sin(time / 930 + a.phase) * wobble;
        a.vx += (a.anchorX + wobbleX - a.x) * 0.026;
        a.vy += (a.anchorY + wobbleY - a.y) * 0.026;
        a.vx += (center - a.x) * 0.0018;
        a.vy += (center - a.y) * 0.0018;
      }

      for (const link of links) {
        const source = points.get(link.source);
        const target = points.get(link.target);
        if (!source || !target) {
          continue;
        }
        const dx = target.x - source.x;
        const dy = target.y - source.y;
        const distance = Math.max(Math.hypot(dx, dy), 1);
        const targetLength = variant === 'feature' ? 54 : 38;
        const pull = ((distance - targetLength) / distance) * 0.011 * Math.max(link.weight, 0.4);
        if (dragRef.current !== link.source) {
          source.vx += dx * pull;
          source.vy += dy * pull;
        }
        if (dragRef.current !== link.target) {
          target.vx -= dx * pull;
          target.vy -= dy * pull;
        }
      }

      for (const [id, point] of points) {
        const isCore = core?.id === id;
        if (isCore) {
          point.x += (center - point.x) * 0.16;
          point.y += (center - point.y) * 0.16;
          point.vx = 0;
          point.vy = 0;
          continue;
        }
        if (dragRef.current === id) {
          point.vx = 0;
          point.vy = 0;
          continue;
        }
        point.vx *= 0.9;
        point.vy *= 0.9;
        const next = constrainPointToCircle(point.x + point.vx, point.y + point.vy, center, boundaryRadius);
        point.x = next.x;
        point.y = next.y;
      }

      setFrame((value) => value + 1);
      rafRef.current = window.requestAnimationFrame(step);
    };

    rafRef.current = window.requestAnimationFrame(step);
    return () => {
      cancelled = true;
      if (rafRef.current != null) {
        window.cancelAnimationFrame(rafRef.current);
      }
    };
  }, [center, core, links, size, variant]);

  useEffect(() => {
    function toSvgPoint(clientX: number, clientY: number) {
      const svg = svgRef.current;
      if (!svg) {
        return null;
      }
      const rect = svg.getBoundingClientRect();
      if (!rect.width || !rect.height) {
        return null;
      }
      return {
        x: ((clientX - rect.left) / rect.width) * size,
        y: ((clientY - rect.top) / rect.height) * size
      };
    }

    function handleMove(event: PointerEvent) {
      if (dragRef.current == null) {
        return;
      }
      const point = toSvgPoint(event.clientX, event.clientY);
      const node = pointsRef.current.get(dragRef.current);
      if (!point || !node) {
        return;
      }
      const constrained = constrainPointToCircle(point.x, point.y, size / 2, size / 2 - 18);
      node.x = constrained.x;
      node.y = constrained.y;
      node.vx = 0;
      node.vy = 0;
      setFrame((value) => value + 1);
    }

    function handleUp() {
      dragRef.current = null;
    }

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
    return () => {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
    };
  }, [size]);

  if (nodes.length === 0) {
    return <div className={`constellation-empty ${variant}`}>{emptyMessage ?? 'No constellation data yet.'}</div>;
  }

  return (
    <svg ref={svgRef} className={`constellation-canvas ${variant}`} viewBox={`0 0 ${size} ${size}`}>
      <circle cx={center} cy={center} r={variant === 'feature' ? 112 : 70} className="constellation-ring" />
      <circle cx={center} cy={center} r={variant === 'feature' ? 74 : 42} className="constellation-core-ring" />
      {links.map((link, index) => {
        const source = pointsRef.current.get(link.source);
        const target = pointsRef.current.get(link.target);
        if (!source || !target) {
          return null;
        }
        const selected = selectedId != null && (link.source === selectedId || link.target === selectedId);
        return (
          <line
            key={`${String(link.source)}-${String(link.target)}-${index}`}
            x1={source.x}
            y1={source.y}
            x2={target.x}
            y2={target.y}
            className={`constellation-link ${selected ? 'is-selected' : ''}`}
          />
        );
      })}
      {nodes.map((node, index) => {
        const point = pointsRef.current.get(node.id);
        if (!point) {
          return null;
        }
        const selected = selectedId === node.id;
        const isCore = core?.id === node.id;
        const radius = point.radius;
        const showLabel =
          selected ||
          isCore ||
          (variant === 'feature' && index < 5 && node.importance > 0.64) ||
          (variant === 'mini' && index < 2 && node.importance > 0.8);

        return (
          <g
            key={String(node.id)}
            transform={`translate(${point.x}, ${point.y})`}
            onClick={() => onSelect(node.id)}
            onPointerDown={(event) => {
              dragRef.current = node.id;
              event.preventDefault();
            }}
          >
            <circle className={`constellation-node ${selected ? 'is-selected' : ''} ${isCore ? 'is-core' : ''}`} r={radius} />
            {showLabel ? (
              <text
                className={`constellation-label ${selected || isCore ? 'is-prominent' : ''}`}
                textAnchor="middle"
                y={variant === 'feature' ? 16 : 14}
              >
                {truncateText(node.label, variant === 'feature' ? 12 : 10)}
              </text>
            ) : null}
          </g>
        );
      })}
    </svg>
  );
}

function StructuralConstellationPanel({
  details,
  selectedNodeId,
  activeKind,
  onSelectNode,
  onKindChange
}: {
  details: IndexDetails;
  selectedNodeId: number | null;
  activeKind: string;
  onSelectNode: (id: number | null) => void;
  onKindChange: (kind: string) => void;
}) {
  const visibleLinks = details.graph.links.filter((link) => activeKind === 'all' || link.kind === activeKind);
  const visibleNodeIds = new Set<number>();
  visibleLinks.forEach((link) => {
    visibleNodeIds.add(link.source);
    visibleNodeIds.add(link.target);
  });

  const nodes = details.graph.nodes
    .filter((node) => visibleNodeIds.size === 0 || visibleNodeIds.has(node.id))
    .sort((a, b) => b.importance - a.importance)
    .map((node) => ({
      id: node.id,
      label: node.name,
      importance: clamp(node.importance, 0.18, 1)
    }));

  const links = visibleLinks.map((link) => ({
    source: link.source,
    target: link.target,
    weight: clamp(link.weight, 0.3, 1)
  }));

  const selectedNode = details.graph.nodes.find((node) => node.id === selectedNodeId) ?? details.graph.nodes[0] ?? null;
  const selectedNodeSummary = selectedNode ? summarizeStructuralNode(selectedNode, activeKind) : '';

  return (
    <section className="ontology-feature-card">
      <div className="section-header">
        <div>
          <p className="eyebrow">Structural Graph</p>
          <h3>Constellation flow - {nodes.length} nodes</h3>
        </div>
        <label className="compact-selector">
          <Filter size={14} />
          <select
            value={activeKind}
            onChange={(event) => {
              onSelectNode(null);
              onKindChange(event.target.value);
            }}
          >
            <option value="all">All edge kinds</option>
            {details.graph.edgeKinds.map((kind) => (
              <option key={kind} value={kind}>
                {kind}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="ontology-feature-layout">
        <div className="ontology-constellation-surface">
          <ConstellationScene
            nodes={nodes}
            links={links}
            selectedId={selectedNode?.id ?? null}
            onSelect={(id) => onSelectNode(Number(id))}
            variant="feature"
            emptyMessage="No structural graph is available for this index yet."
          />
        </div>

        <div className="ontology-info-pane">
          <p className="eyebrow">Constellation Info</p>
          <h4>{selectedNode?.name ?? 'No node selected'}</h4>
          {selectedNode ? (
            <>
              <div className="ontology-stat-grid">
                <span>{selectedNode.kind}</span>
                <span>{selectedNode.language ?? 'mixed'}</span>
                <span>{selectedNode.inbound} inbound</span>
                <span>{selectedNode.outbound} outbound</span>
              </div>
              <div className="ontology-detail-stack">
                <div className="ontology-detail-row">
                  <span>Importance</span>
                  <strong>{selectedNode.importance.toFixed(2)}</strong>
                </div>
                <div className="ontology-detail-row">
                  <span>Visible edge filter</span>
                  <strong>{activeKind === 'all' ? 'All edges' : activeKind}</strong>
                </div>
              </div>
              <p className="muted-copy">{selectedNodeSummary}</p>
            </>
          ) : (
            <p className="muted-copy">Select a node in the constellation to inspect its structural role.</p>
          )}
        </div>
      </div>
    </section>
  );
}

function SignalConstellationCard({
  signal,
  selectedEntryKey,
  onSelectEntry
}: {
  signal: IndexDetails['signals'][number];
  selectedEntryKey: string | null;
  onSelectEntry: (key: string) => void;
}) {
  const entries = signal.entries.slice(0, 14);
  const coreId = `${signal.label}-core`;
  const nodes: ConstellationNode[] = [
    { id: coreId, label: signal.label, importance: 1 },
    ...entries.map((entry, index) => ({
      id: `${signal.label}:${entry.label}:${entry.supporting}`,
      label: entry.label,
      importance: clamp(1 - index / Math.max(entries.length * 1.25, 1), 0.24, 0.92)
    }))
  ];
  const links: ConstellationLink[] = entries.map((entry, index) => ({
    source: coreId,
    target: `${signal.label}:${entry.label}:${entry.supporting}`,
    weight: clamp(1 - index / Math.max(entries.length * 1.4, 1), 0.22, 0.9)
  }));
  const activeEntry =
    entries.find((entry) => `${signal.label}:${entry.label}:${entry.supporting}` === selectedEntryKey) ?? entries[0] ?? null;
  const activeSummary = summarizeSignalEntry(signal, activeEntry);

  return (
    <section className="ontology-flow-card">
      <div className="section-header">
        <div>
          <p className="eyebrow">{signal.label} constellation</p>
          <h3>
            {signal.label} constellation - {signal.count} signals
          </h3>
        </div>
      </div>
      <ConstellationScene
        nodes={nodes}
        links={links}
        selectedId={activeEntry ? `${signal.label}:${activeEntry.label}:${activeEntry.supporting}` : null}
        onSelect={(id) => onSelectEntry(String(id))}
        variant="mini"
        emptyMessage={`No ${signal.label.toLowerCase()} signals are available.`}
      />
      <div className="ontology-flow-info">
        <strong>{activeEntry?.label ?? `No ${signal.label.toLowerCase()} node selected`}</strong>
        {activeEntry ? (
          <>
            <div className="ontology-info-chip-row">
              <span>{activeEntry.value}</span>
              <span>{truncateText(activeEntry.supporting.split('/').slice(-2).join('/'), 32)}</span>
            </div>
            <p>{activeSummary}</p>
          </>
        ) : (
          <p>{signal.detail}</p>
        )}
      </div>
    </section>
  );
}

function RankedConstellationPanel({
  symbols,
  selectedSymbolId,
  onSelectSymbol
}: {
  symbols: IndexDetails['rankedSymbols'];
  selectedSymbolId: number | null;
  onSelectSymbol: (id: number) => void;
}) {
  const ranked = [...symbols]
    .sort((a, b) => b.importance - a.importance)
    .slice(0, 22);
  const strongest = ranked[0] ?? null;
  const nodes: ConstellationNode[] = ranked.map((symbol, index) => ({
    id: symbol.id,
    label: symbol.name,
    importance: clamp(1 - index / Math.max(ranked.length * 1.15, 1), 0.2, 1)
  }));
  const links: ConstellationLink[] =
    strongest == null
      ? []
      : ranked.slice(1).map((symbol, index) => ({
          source: strongest.id,
          target: symbol.id,
          weight: clamp(1 - index / Math.max(ranked.length * 1.4, 1), 0.25, 0.95)
        }));
  const selectedSymbol = ranked.find((symbol) => symbol.id === selectedSymbolId) ?? strongest;
  const selectedSymbolSummary = selectedSymbol ? summarizeRankedSymbol(selectedSymbol) : '';

  return (
    <section className="ontology-feature-card">
      <div className="section-header">
        <div>
          <p className="eyebrow">Ranked Symbols</p>
          <h3>Constellation of centrality - {ranked.length} ranked</h3>
        </div>
      </div>

      <div className="ontology-feature-layout">
        <div className="ontology-constellation-surface">
          <ConstellationScene
            nodes={nodes}
            links={links}
            selectedId={selectedSymbol?.id ?? null}
            onSelect={(id) => onSelectSymbol(Number(id))}
            variant="feature"
            emptyMessage="No ranked symbols are available for this index yet."
          />
        </div>

        <div className="ontology-info-pane">
          <p className="eyebrow">Constellation Info</p>
          <h4>{selectedSymbol?.name ?? 'No symbol selected'}</h4>
          {selectedSymbol ? (
            <>
              <div className="ontology-stat-grid">
                <span>{selectedSymbol.kind}</span>
                <span>{selectedSymbol.language}</span>
                <span>{selectedSymbol.inbound} inbound</span>
                <span>{selectedSymbol.outbound} outbound</span>
              </div>
              <div className="ontology-detail-stack">
                <div className="ontology-detail-row">
                  <span>Importance</span>
                  <strong>{selectedSymbol.importance.toFixed(2)}</strong>
                </div>
                <div className="ontology-detail-row">
                  <span>Source</span>
                  <strong>{truncateText(selectedSymbol.filePath, 46)}</strong>
                </div>
              </div>
              <p className="muted-copy">{selectedSymbolSummary}</p>
            </>
          ) : (
            <p className="muted-copy">Select a ranked symbol to inspect where its centrality comes from.</p>
          )}
        </div>
      </div>
    </section>
  );
}

function IndexTreePanel({
  items,
  selectedId,
  expandedIds,
  onSelect,
  onToggle,
  isOpen,
  onToggleOpen
}: {
  items: IndexTreeItem[];
  selectedId: string | null;
  expandedIds: Set<string>;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
  isOpen: boolean;
  onToggleOpen: () => void;
}) {
  return (
    <div className="tree-card primary-tree-card">
      <button className="section-header panel-toggle" onClick={onToggleOpen}>
        <div>
          <p className="eyebrow">Index Tree</p>
          <h3>Project source flow</h3>
        </div>
        <div className="panel-toggle-meta">
          <span className="pill">{items.reduce((total, item) => total + item.fileCount, 0)} files</span>
          {isOpen ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        </div>
      </button>

      {isOpen ? (
        <>
          <p className="muted-copy tree-intro">Expand an extension to walk into its indexed files and inspect how each branch carries symbols and graph pressure.</p>

          <div className="tree-layout">
            <div className="tree-list">
              {items.map((item) => (
                <div key={item.id} className="tree-group">
                  <button
                    className={`tree-row tree-row-extension ${selectedId === item.id ? 'active' : ''}`}
                    onClick={() => {
                      onSelect(item.id);
                      onToggle(item.id);
                    }}
                  >
                    {expandedIds.has(item.id) ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
                    <FolderTree size={16} />
                    <span>{item.name}</span>
                    <small>{item.fileCount}</small>
                  </button>
                  {expandedIds.has(item.id) ? (
                    <div className="tree-children">
                      {(item.children ?? []).map((child) => (
                        <button
                          key={child.id}
                          className={`tree-row tree-row-file ${selectedId === child.id ? 'active' : ''}`}
                          onClick={() => onSelect(child.id)}
                          title={child.path ?? child.name}
                        >
                          <FileCode2 size={15} />
                          <div className="tree-row-copy">
                            <span>{truncateText(child.name, 36)}</span>
                            <small className="tree-row-meta">
                              {child.symbolCount} symbols · {child.inbound + child.outbound > 0 ? `${child.inbound + child.outbound} flow` : `${child.lineCount} lines`}
                            </small>
                          </div>
                          <small>{child.inbound + child.outbound > 0 ? child.inbound + child.outbound : child.symbolCount}</small>
                        </button>
                      ))}
                    </div>
                  ) : null}
                </div>
              ))}
            </div>
          </div>
        </>
      ) : null}
    </div>
  );
}

function RankedSymbolsPanel({
  symbols,
  expandedGroups,
  onToggleGroup,
  onSelectNode,
  query,
  onQuery
}: {
  symbols: IndexDetails['rankedSymbols'];
  expandedGroups: Set<string>;
  onToggleGroup: (group: string) => void;
  onSelectNode: (id: number) => void;
  query: string;
  onQuery: (value: string) => void;
}) {
  const groups = groupedSymbols(symbols);

  return (
    <div className="symbols-panel">
      <div className="section-header">
        <div>
          <p className="eyebrow">Ranked Symbols</p>
          <h3>Central symbols</h3>
        </div>
        <span className="pill">{symbols.length}</span>
      </div>
      <label className="search-row">
        <Search size={15} />
        <input value={query} onChange={(event) => onQuery(event.target.value)} placeholder="Filter symbols, kinds, or files" />
      </label>
      <div className="symbol-groups">
        {Object.entries(groups).map(([group, groupSymbols]) => (
          <div key={group} className="symbol-group">
            <button className="symbol-group-head" onClick={() => onToggleGroup(group)}>
              {expandedGroups.has(group) ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
              <span>{group} symbols</span>
              <small>{groupSymbols.length}</small>
            </button>
            {expandedGroups.has(group) ? (
              <div className="symbol-list compact">
                {groupSymbols.map((symbol) => (
                  <button key={symbol.id} className="symbol-row interactive" onClick={() => onSelectNode(symbol.id)}>
                    <div className="symbol-avatar">{initials(symbol.name)}</div>
                    <div>
                      <strong>{truncateText(symbol.name, 34)}</strong>
                      <p>
                        {symbol.kind} · in {symbol.inbound} · out {symbol.outbound}
                      </p>
                      <small>{truncateText(symbol.filePath, 58)}</small>
                    </div>
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  );
}

function FlowNeighborsCard({
  selection,
  isOpen,
  onToggleOpen
}: {
  selection: { item: IndexTreeItem; parent: IndexTreeItem | null } | null;
  isOpen: boolean;
  onToggleOpen: () => void;
}) {
  const selectedItem = selection?.item ?? null;
  const nearbyFiles = nearbyFilesForSelection(selection);

  return (
    <section className="tree-card mini-tree-card context-tree-card flow-tree-card">
      <button className="section-header panel-toggle" onClick={onToggleOpen}>
        <div>
          <p className="eyebrow">Flow Neighbors</p>
          <h3>{selectedItem?.type === 'file' ? 'Nearby files' : 'Branch files'}</h3>
        </div>
        <div className="panel-toggle-meta">
          <span className="pill">{nearbyFiles.length}</span>
          {isOpen ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        </div>
      </button>
      {isOpen ? (
        <div className="mini-tree-list flow-tree-list">
          {nearbyFiles.length > 0 ? (
            nearbyFiles.map((file) => (
              <div key={file.id} className="mini-tree-row">
                <FileCode2 size={14} />
                <div>
                  <strong>{truncateText(file.name, 28)}</strong>
                  <small>
                    {file.symbolCount} symbols · {file.inbound + file.outbound > 0 ? `${file.inbound + file.outbound} total flow` : `${file.lineCount} lines`}
                  </small>
                </div>
              </div>
            ))
          ) : (
            <p className="muted-copy">No nearby files available for this selection yet.</p>
          )}
        </div>
      ) : null}
    </section>
  );
}

function BranchSummaryCard({
  selection,
  relatedSymbols,
  isOpen,
  onToggleOpen
}: {
  selection: { item: IndexTreeItem; parent: IndexTreeItem | null } | null;
  relatedSymbols: IndexDetails['rankedSymbols'];
  isOpen: boolean;
  onToggleOpen: () => void;
}) {
  const selectedItem = selection?.item ?? null;
  const nearbyFiles = nearbyFilesForSelection(selection);

  const totals = useMemo(() => {
    const fileCount = selectedItem?.type === 'extension' ? selectedItem.fileCount : nearbyFiles.length + (selectedItem ? 1 : 0);
    const symbolCount = nearbyFiles.reduce((sum, file) => sum + file.symbolCount, selectedItem?.type === 'file' ? selectedItem.symbolCount : 0);
    const lineCount = nearbyFiles.reduce((sum, file) => sum + file.lineCount, selectedItem?.type === 'file' ? selectedItem.lineCount : selectedItem?.lineCount ?? 0);
    const totalFlow = nearbyFiles.reduce((sum, file) => sum + file.inbound + file.outbound, 0);
    return { fileCount, symbolCount, lineCount, totalFlow };
  }, [nearbyFiles, selectedItem]);

  const topDirectories = useMemo(() => {
    const counts = new Map<string, number>();
    for (const file of nearbyFiles) {
      const filePath = file.path ?? '';
      const parts = filePath.split('/').filter(Boolean);
      const dir = parts.slice(0, -1).slice(-2).join('/') || 'root';
      counts.set(dir, (counts.get(dir) ?? 0) + 1);
    }
    return [...counts.entries()]
      .sort((left, right) => right[1] - left[1])
      .slice(0, 3);
  }, [nearbyFiles]);

  const roleMix = useMemo(() => {
    const counts = new Map<string, number>();
    for (const symbol of relatedSymbols) {
      const key = symbol.kind || 'symbol';
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return [...counts.entries()]
      .sort((left, right) => right[1] - left[1])
      .slice(0, 4);
  }, [relatedSymbols]);

  const dominantFile = nearbyFiles[0] ?? (selectedItem?.type === 'file' ? selectedItem : null);

  return (
    <section className="tree-card mini-tree-card context-tree-card summary-tree-card">
      <button className="section-header panel-toggle" onClick={onToggleOpen}>
        <div>
          <p className="eyebrow">Selected Branch</p>
          <h3>{selectedItem ? selectedItem.name : 'Awaiting branch'}</h3>
        </div>
        {isOpen ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
      </button>
      {isOpen ? (
        selectedItem ? (
          <>
          <p className="muted-copy summary-intro">
            {selectedItem.type === 'extension'
              ? `${selectedItem.name} is weighted here by file density, symbol concentration, and the strongest branch neighbors below.`
              : `${selectedItem.name} is surrounded here by the files most likely to share structure, references, or graph pressure.`}
          </p>
          <div className="summary-stat-grid">
            <div className="summary-stat">
              <strong>{totals.fileCount}</strong>
              <span>files</span>
            </div>
            <div className="summary-stat">
              <strong>{totals.symbolCount}</strong>
              <span>symbols</span>
            </div>
            <div className="summary-stat">
              <strong>{totals.lineCount}</strong>
              <span>lines</span>
            </div>
            <div className="summary-stat">
              <strong>{totals.totalFlow}</strong>
              <span>total flow</span>
            </div>
          </div>

          <div className="summary-section">
            <span className="summary-label">Top directories</span>
            <div className="summary-chip-list">
              {topDirectories.length > 0 ? (
                topDirectories.map(([directory, count]) => (
                  <span key={directory} className="summary-chip">
                    {directory} <small>{count}</small>
                  </span>
                ))
              ) : (
                <span className="summary-chip">branch root</span>
              )}
            </div>
          </div>

          <div className="summary-section">
            <span className="summary-label">Role mix</span>
            <div className="summary-chip-list">
              {roleMix.length > 0 ? (
                roleMix.map(([kind, count]) => (
                  <span key={kind} className="summary-chip">
                    {kind} <small>{count}</small>
                  </span>
                ))
              ) : (
                <span className="summary-chip">no ranked symbols yet</span>
              )}
            </div>
          </div>

          {dominantFile ? (
            <div className="summary-feature">
              <span className="summary-label">Dominant file</span>
              <strong>{dominantFile.name}</strong>
              <p>
                {dominantFile.symbolCount} symbols · {dominantFile.lineCount} lines
                {dominantFile.inbound + dominantFile.outbound > 0 ? ` · ${dominantFile.inbound + dominantFile.outbound} flow` : ''}
              </p>
            </div>
          ) : null}
          </>
        ) : (
          <p className="muted-copy">Choose an extension or file to see branch totals, directory concentration, and dominant files.</p>
        )
      ) : null}
    </section>
  );
}

function RelatedSymbolsCard({
  selection,
  relatedSymbols,
  selectedSymbolId,
  onSelectSymbol,
  query,
  onQuery,
  isOpen,
  onToggleOpen
}: {
  selection: { item: IndexTreeItem; parent: IndexTreeItem | null } | null;
  relatedSymbols: IndexDetails['rankedSymbols'];
  selectedSymbolId: number | null;
  onSelectSymbol: (id: number) => void;
  query: string;
  onQuery: (value: string) => void;
  isOpen: boolean;
  onToggleOpen: () => void;
}) {
  const selectedItem = selection?.item ?? null;
  const selectedSymbol = relatedSymbols.find((symbol) => symbol.id === selectedSymbolId) ?? relatedSymbols[0] ?? null;

  return (
    <section className="tree-card mini-tree-card context-tree-card related-tree-card">
      <button className="section-header panel-toggle" onClick={onToggleOpen}>
        <div>
          <p className="eyebrow">Related Symbols</p>
          <h3>{selectedItem ? 'Branch canopy' : 'Project canopy'}</h3>
        </div>
        <div className="panel-toggle-meta">
          <span className="pill">{relatedSymbols.length}</span>
          {isOpen ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        </div>
      </button>
      {isOpen ? (
        <>
          <label className="search-row mini-search-row">
            <Search size={15} />
            <input value={query} onChange={(event) => onQuery(event.target.value)} placeholder="Filter local symbols or files" />
          </label>
          <div className="mini-tree-list related-tree-list">
            {relatedSymbols.length > 0 ? (
              relatedSymbols.slice(0, 10).map((symbol) => (
                <button
                  key={symbol.id}
                  className={`mini-tree-row interactive ${selectedSymbol?.id === symbol.id ? 'active' : ''}`}
                  onClick={() => onSelectSymbol(symbol.id)}
                >
                  <FileCode2 size={14} />
                  <div>
                    <strong>{truncateText(symbol.name, 28)}</strong>
                    <small>
                      {symbol.kind} · in {symbol.inbound} · out {symbol.outbound}
                    </small>
                    <small>{truncateText(symbol.filePath.split('/').slice(-3).join('/'), 38)}</small>
                  </div>
                </button>
              ))
            ) : (
              <p className="muted-copy">No ranked symbols match this branch yet.</p>
            )}
          </div>
          {selectedSymbol ? <p className="muted-copy branch-summary">{summarizeRankedSymbol(selectedSymbol)}</p> : null}
        </>
      ) : null}
    </section>
  );
}

function WorkspaceRail({
  selectedSummary,
  busy,
  busySince,
  onChooseProject,
  onRefresh,
  onDelete
}: {
  selectedSummary: IndexSummary | null;
  busy: string | null;
  busySince: number | null;
  onChooseProject: () => void;
  onRefresh: () => void;
  onDelete: () => void;
}) {
  const liveIndexReady = selectedSummary?.health === 'ready';

  return (
    <aside className="workspace-rail">
      <section className="rail-section">
        <div className="section-header compact">
          <div>
            <p className="eyebrow">Index Launcher</p>
            <h3>Index a project</h3>
          </div>
          <PlayCircle size={17} />
        </div>
        <button className="file-command" onClick={onChooseProject}>
          <span>{busy ? 'Building graph' : 'Choose folder'}</span>
          <strong>{busy ? 'Project selected' : 'Start a new index'}</strong>
          <PlayCircle size={18} />
        </button>
      </section>

      <section className="rail-section rail-status">
        <div className="section-header compact">
          <div>
            <p className="eyebrow">Live Index</p>
            <h3>Status</h3>
          </div>
          <Activity
            className={`status-pulse-icon ${liveIndexReady ? 'is-ready' : 'is-alert'}`}
            size={17}
            aria-label={liveIndexReady ? 'Live index ready' : 'Live index not ready'}
          />
        </div>
        <div className="status-grid">
          <span>Files</span>
          <strong>{selectedSummary?.files ?? 0}</strong>
          <span>Symbols</span>
          <strong>{selectedSummary?.symbols ?? 0}</strong>
          <span>Mode</span>
          <strong>{selectedSummary?.activeMode ?? 'Unknown'}</strong>
          <span>Health</span>
          <strong>{selectedSummary ? healthLabel(selectedSummary.health) : 'Idle'}</strong>
        </div>
      </section>

      <section className="rail-section rail-terminal">
        <div className="section-header compact">
          <div>
            <p className="eyebrow">Terminal Commands</p>
            <h3>GraphIQ Terminal</h3>
          </div>
          <GitCommitHorizontal size={17} />
        </div>
        <button className="terminal-line" onClick={onRefresh}>
          <span>&gt;</span>
          <code>graphiq index --refresh</code>
        </button>
        <button className="terminal-line" onClick={onChooseProject}>
          <span>&gt;</span>
          <code>
            {busy ? (
              <>
                indexing project
                <LoadingDots />
                <LoadingElapsed since={busySince} />
              </>
            ) : (
              'graphiq index /project'
            )}
          </code>
        </button>
        <button className="terminal-line danger" onClick={onDelete} disabled={!selectedSummary}>
          <span>&gt;</span>
          <code>delete active index</code>
        </button>
      </section>
    </aside>
  );
}

function ConnectorsPage({ onOpenIndexes }: { onOpenIndexes: () => void }) {
  const [connectors, setConnectors] = useState<ConnectorStatus[]>([]);
  const [busyConnector, setBusyConnector] = useState<string | null>(null);
  const [cardNotice, setCardNotice] = useState<Record<string, IndexActionResult>>({});
  const [globalNotice, setGlobalNotice] = useState<IndexActionResult | null>(null);
  const [selectedSummary, setSelectedSummary] = useState<IndexSummary | null>(null);
  const [busyIndexPath, setBusyIndexPath] = useState<string | null>(null);
  const [busyIndexSince, setBusyIndexSince] = useState<number | null>(null);

  async function refreshConnectors() {
    const next = await getDesktopApi().listConnectors();
    setConnectors(next);
  }

  async function refreshRail(forceRefresh = false) {
    const next = await getDesktopApi().listIndexes(forceRefresh);
    setSelectedSummary(next[0] ?? null);
  }

  useEffect(() => {
    refreshConnectors().catch((error) =>
      setGlobalNotice({
        ok: false,
        message: 'Failed to load connectors.',
        details: compactLog(String(error))
      })
    );
    refreshRail().catch(() => undefined);
  }, []);

  async function runRailAction(action: Promise<IndexActionResult>, openIndexes = false) {
    const result = await action;
    setGlobalNotice({
      ok: result.ok,
      message: result.message,
      details: result.details ? compactLog(result.details) : undefined
    });
    if (result.ok) {
      await refreshRail(true);
      if (openIndexes) {
        onOpenIndexes();
      }
    }
  }

  async function createIndex(projectPath: string | null) {
    if (!projectPath) {
      return;
    }
    setBusyIndexPath(projectPath);
    setBusyIndexSince(Date.now());
    try {
      await runRailAction(getDesktopApi().indexProject(projectPath), true);
    } finally {
      setBusyIndexPath(null);
      setBusyIndexSince(null);
    }
  }

  async function runConnectorAction(connector: ConnectorStatus) {
    setBusyConnector(connector.id);
    setCardNotice((current) => ({
      ...current,
      [connector.id]: {
        ok: true,
        message: `${connector.status === 'connected' ? 'Unpairing' : 'Pairing'} ${connector.name}...`,
        details: 'Waiting for live output...'
      }
    }));
    try {
      const api = getDesktopApi();
      const result =
        connector.status === 'connected'
          ? await api.unpairConnector(connector.id)
          : await api.pairConnector(connector.id);
      setCardNotice((current) => ({
        ...current,
        [connector.id]: {
          ok: result.ok,
          message: result.message,
          details: result.details ? compactLog(result.details) : undefined
        }
      }));
      await refreshConnectors();
      setGlobalNotice(null);
    } catch (error) {
      setCardNotice((current) => ({
        ...current,
        [connector.id]: {
          ok: false,
          message: `Failed to ${connector.status === 'connected' ? 'unpair' : 'pair'} ${connector.name}.`,
          details: compactLog(String(error))
        }
      }));
    } finally {
      setBusyConnector(null);
    }
  }

  return (
    <div className="workspace-layout">
      <div className="workspace-main">
        <div className="connectors-page">
          <PageHeader page="connectors" />

          <div className="connector-grid">
            {connectors.map((connector) => (
              <section key={connector.id} className="connector-card">
                {(() => {
                  const notice = cardNotice[connector.id];
                  return notice ? (
                    <div className={`connector-live-window ${notice.ok ? 'ok' : 'error'}`}>
                      <div className="connector-live-window-head">
                        <span className="eyebrow">Live status</span>
                        <strong>{busyConnector === connector.id ? 'Running' : notice.ok ? 'Latest' : 'Error'}</strong>
                      </div>
                      <p>{notice.message}</p>
                      {notice.details ? <pre>{notice.details}</pre> : null}
                    </div>
                  ) : null;
                })()}

                <div className="connector-head">
                  <div>
                    <p className="eyebrow">{connector.surface}</p>
                    <h3>{connector.name}</h3>
                  </div>
                  <span className={`activity-dot ${connector.active ? 'active' : ''}`} title={connector.active ? 'Active' : 'Inactive'} />
                </div>

                <div className="connector-status">
                  <span>Status</span>
                  <strong>{connector.status}</strong>
                  <span>Activity</span>
                  <strong>{connector.active ? 'active use' : 'idle'}</strong>
                  <span>Last seen</span>
                  <strong>{formatDate(connector.lastSeenAt)}</strong>
                </div>

                <p className="connector-detail">{connector.detail}</p>
                <div className="connector-paths">
                  {connector.configPaths.slice(0, 3).map((configPath) => (
                    <code key={configPath}>{truncateText(configPath, 72)}</code>
                  ))}
                </div>

                <button className="button secondary full" onClick={() => runConnectorAction(connector)} disabled={busyConnector === connector.id}>
                  {busyConnector === connector.id ? 'Working...' : connector.status === 'connected' ? 'Unpair' : 'Pair'}
                </button>
              </section>
            ))}
          </div>

          {globalNotice ? (
            <div className={`notice ${globalNotice.ok ? 'ok' : 'error'}`}>
              {globalNotice.message}
              {globalNotice.details ? `\n${globalNotice.details}` : ''}
            </div>
          ) : null}
        </div>
      </div>

      <WorkspaceRail
        selectedSummary={selectedSummary}
        busy={busyIndexPath}
        busySince={busyIndexSince}
        onChooseProject={() => getDesktopApi().chooseProjectFolder().then(createIndex)}
        onRefresh={() => refreshRail(true)}
        onDelete={() =>
          selectedSummary
            ? runRailAction(getDesktopApi().deleteIndex(selectedSummary.projectPath)).catch((error) =>
                setGlobalNotice({
                  ok: false,
                  message: String(error)
                })
              )
            : undefined
        }
      />
    </div>
  );
}

function WorkspacePage({ page, onOpenIndexes }: { page: Exclude<Page, 'settings' | 'connectors'>; onOpenIndexes: () => void }) {
  const [indexes, setIndexes] = useState<IndexSummary[]>([]);
  const [selectedPath, setSelectedPath] = useState<string>('');
  const [details, setDetails] = useState<IndexDetails | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [busySince, setBusySince] = useState<number | null>(null);
  const [notice, setNotice] = useState<string>('');
  const [graphMode, setGraphMode] = useState<GraphMode>('constellation');
  const [selectedNodeId, setSelectedNodeId] = useState<number | null>(null);
  const [activeKind, setActiveKind] = useState<string>('all');
  const [symbolQuery, setSymbolQuery] = useState('');
  const [selectedTreeId, setSelectedTreeId] = useState<string | null>(null);
  const [expandedTreeIds, setExpandedTreeIds] = useState<Set<string>>(new Set());
  const [selectedSignalEntries, setSelectedSignalEntries] = useState<Record<string, string>>({});
  const [selectedRankedSymbolId, setSelectedRankedSymbolId] = useState<number | null>(null);
  const [openPanels, setOpenPanels] = useState<Set<IndexPanelKey>>(defaultIndexPanels);

  async function refreshIndexes(forceRefresh = false, preferredPath?: string) {
    const api = getDesktopApi();
    const next = await api.listIndexes(forceRefresh);
    setIndexes(next);
    const active = preferredPath || selectedPath || next[0]?.projectPath || '';
    setSelectedPath(active);
    if (active) {
      const nextDetails = await api.getIndexDetails(active);
      setDetails(nextDetails);
      setActiveKind('all');
      setSelectedNodeId(nextDetails.graph.nodes[0]?.id ?? null);
      setSelectedTreeId(nextDetails.tree[0]?.id ?? null);
      setExpandedTreeIds(new Set());
      setSelectedSignalEntries({});
      setSelectedRankedSymbolId(nextDetails.rankedSymbols[0]?.id ?? null);
      setOpenPanels(defaultIndexPanels());
    } else {
      setDetails(null);
      setSelectedTreeId(null);
      setExpandedTreeIds(new Set());
      setSelectedSignalEntries({});
      setSelectedRankedSymbolId(null);
      setOpenPanels(defaultIndexPanels());
    }
  }

  useEffect(() => {
    refreshIndexes(true).catch((error) => setNotice(String(error)));
  }, []);

  useEffect(() => {
    if (!selectedPath) {
      return;
    }
    getDesktopApi()
      .getIndexDetails(selectedPath)
      .then((nextDetails) => {
        setDetails(nextDetails);
        setActiveKind('all');
        setSelectedNodeId(nextDetails.graph.nodes[0]?.id ?? null);
        setSelectedTreeId(nextDetails.tree[0]?.id ?? null);
        setExpandedTreeIds(new Set());
        setSelectedSignalEntries({});
        setSelectedRankedSymbolId(nextDetails.rankedSymbols[0]?.id ?? null);
        setOpenPanels(defaultIndexPanels());
      })
      .catch((error) => setNotice(String(error)));
  }, [selectedPath]);

  async function runAction(action: Promise<IndexActionResult>, reloadPath?: string) {
    const result = await action;
    setNotice(result.details ? `${result.message}\n${result.details}` : result.message);
    if (result.ok) {
      await refreshIndexes(true, reloadPath);
      if (reloadPath) {
        onOpenIndexes();
      }
    }
  }

  async function createIndex(projectPath: string | null) {
    if (!projectPath) {
      return;
    }
    setBusy(projectPath);
    setBusySince(Date.now());
    try {
      await runAction(getDesktopApi().indexProject(projectPath), projectPath);
    } finally {
      setBusy(null);
      setBusySince(null);
    }
  }

  const selectedSummary = useMemo(
    () => indexes.find((entry) => entry.projectPath === selectedPath) ?? indexes[0] ?? null,
    [indexes, selectedPath]
  );

  const filteredGraphKinds = details?.graph.edgeKinds ?? [];

  const selectedNode = useMemo(
    () => details?.graph.nodes.find((node) => node.id === selectedNodeId) ?? null,
    [details, selectedNodeId]
  );

  const treeSelection = useMemo(() => (details ? findTreeSelection(details.tree, selectedTreeId) : null), [details, selectedTreeId]);

  const relatedTreeSymbols = useMemo(() => {
    if (!details || !treeSelection) {
      return details?.rankedSymbols ?? [];
    }
    return details.rankedSymbols.filter((symbol) => matchesTreeSelection(symbol, treeSelection.item, treeSelection.parent));
  }, [details, treeSelection]);

  const visibleBranchSymbols = useMemo(() => {
    return relatedTreeSymbols.filter((symbol) => {
      if (!symbolQuery.trim()) {
        return true;
      }
      const haystack = `${symbol.name} ${symbol.qualifiedName ?? ''} ${symbol.kind} ${symbol.filePath}`.toLowerCase();
      return haystack.includes(symbolQuery.toLowerCase());
    });
  }, [relatedTreeSymbols, symbolQuery]);

  function toggleTree(id: string) {
    setExpandedTreeIds((current) => {
      const next = new Set(current);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  function togglePanel(key: IndexPanelKey) {
    setOpenPanels((current) => {
      const next = new Set(current);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  }

  return (
    <div className="workspace-layout">
      <div className="workspace-main">
        <PageHeader page={page} />

        <div className="project-strip">
          <label className="project-selector">
            <span>Project:</span>
            <select value={selectedSummary?.projectPath ?? ''} onChange={(event) => setSelectedPath(event.target.value)}>
              {indexes.length === 0 ? <option value="">No indexes found yet</option> : null}
              {indexes.map((index) => (
                <option key={index.projectPath} value={index.projectPath}>
                  {index.name}
                </option>
              ))}
            </select>
          </label>
          <div className="project-meta">
            <span>Created: {formatDate(selectedSummary?.createdAt ?? null)}</span>
            <span>Last Access: {formatDate(selectedSummary?.lastAccessedAt ?? null)}</span>
            <span>Mode: {selectedSummary?.activeMode ?? 'Unknown'}</span>
          </div>
        </div>

        {page === 'indexes' ? (
          <div className="indexes-stack">
            <div className="indexes-tree-shell">
              <div className="indexes-primary-column">
                {details ? (
                  <IndexTreePanel
                    items={details.tree}
                  selectedId={selectedTreeId}
                  expandedIds={expandedTreeIds}
                  onSelect={setSelectedTreeId}
                  onToggle={toggleTree}
                  isOpen={openPanels.has('tree')}
                  onToggleOpen={() => togglePanel('tree')}
                />
              ) : null}
              <RelatedSymbolsCard
                selection={treeSelection}
                relatedSymbols={visibleBranchSymbols}
                  selectedSymbolId={selectedRankedSymbolId}
                onSelectSymbol={setSelectedRankedSymbolId}
                query={symbolQuery}
                onQuery={setSymbolQuery}
                isOpen={openPanels.has('related')}
                onToggleOpen={() => togglePanel('related')}
              />
            </div>
            <div className="indexes-secondary-column">
                <FlowNeighborsCard
                  selection={treeSelection}
                  isOpen={openPanels.has('neighbors')}
                  onToggleOpen={() => togglePanel('neighbors')}
                />
                <BranchSummaryCard
                  selection={treeSelection}
                  relatedSymbols={visibleBranchSymbols}
                  isOpen={openPanels.has('summary')}
                  onToggleOpen={() => togglePanel('summary')}
                />
              </div>
            </div>
          </div>
        ) : null}

        {page === 'ontology' ? (
          <div className="ontology-stack">
            {details ? (
              <StructuralConstellationPanel
                details={details}
                selectedNodeId={selectedNodeId}
                activeKind={activeKind}
                onSelectNode={setSelectedNodeId}
                onKindChange={setActiveKind}
              />
            ) : null}
            {details ? (
              <div className="ontology-flow-grid">
                {details.signals.slice(0, 3).map((signal) => (
                  <SignalConstellationCard
                    key={signal.label}
                    signal={signal}
                    selectedEntryKey={selectedSignalEntries[signal.label] ?? null}
                    onSelectEntry={(key) =>
                      setSelectedSignalEntries((current) => ({
                        ...current,
                        [signal.label]: key
                      }))
                    }
                  />
                ))}
              </div>
            ) : null}
            {details ? (
              <RankedConstellationPanel
                symbols={details.rankedSymbols}
                selectedSymbolId={selectedRankedSymbolId}
                onSelectSymbol={setSelectedRankedSymbolId}
              />
            ) : null}
          </div>
        ) : null}

        {notice ? <div className="notice">{notice}</div> : null}
      </div>

      <WorkspaceRail
        selectedSummary={selectedSummary}
        busy={busy}
        busySince={busySince}
        onChooseProject={() => getDesktopApi().chooseProjectFolder().then(createIndex)}
        onRefresh={() => refreshIndexes(true)}
        onDelete={() =>
          selectedSummary
            ? runAction(getDesktopApi().deleteIndex(selectedSummary.projectPath)).catch((error) => setNotice(String(error)))
            : undefined
        }
      />
    </div>
  );
}

function SettingsPage() {
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
  const [notice, setNotice] = useState<string>('');
  const [draft, setDraft] = useState<IssueDraft>(issueTemplate);

  async function handleAction(action: Promise<IndexActionResult>) {
    const result = await action;
    setNotice(result.details ? `${result.message}\n${result.details}` : result.message);
  }

  return (
    <div className="page page-settings">
      <div className="hero-panel settings-hero">
        <div>
          <p className="eyebrow">Settings</p>
          <h1>Keep the desktop shell sharp and the bug reports useful.</h1>
          <p className="lede">
            No accounts. No cloud lock-in. Just update controls, uninstall behavior, and a structured issue flow
            that forces high-signal reports.
          </p>
        </div>
      </div>

      <div className="settings-grid">
        <div className="settings-card">
          <div className="section-header">
            <div>
              <p className="eyebrow">Application</p>
              <h3>Maintenance</h3>
            </div>
            <Sparkles size={18} />
          </div>
          <button className="button secondary full" onClick={() => getDesktopApi().checkForUpdates().then(setUpdateStatus)}>
            <RefreshCcw size={16} />
            Check for Updates
          </button>
          <button className="button secondary full" onClick={() => handleAction(getDesktopApi().pullLatestMain())}>
            <GitBranch size={16} />
            Update from GraphIQ Main
          </button>
          <button className="button danger full" onClick={() => handleAction(getDesktopApi().uninstallApp())}>
            <Trash2 size={16} />
            Uninstall Desktop App
          </button>
          {updateStatus ? (
            <div className="update-box">
              <strong>{updateStatus.currentVersion}</strong>
              <p>
                Latest release: {updateStatus.latestVersion ?? 'Unknown'} ·{' '}
                {updateStatus.updateAvailable ? 'Update available' : 'You are current or ahead in dev mode'}
              </p>
            </div>
          ) : null}
        </div>

        <div className="settings-card issue-card">
          <div className="section-header">
            <div>
              <p className="eyebrow">Issues</p>
              <h3>Submit a clean report</h3>
            </div>
            <AlertCircle size={18} />
          </div>

          <div className="form-grid">
            <label>
              Title
              <input
                value={draft.title}
                onChange={(event) => setDraft({ ...draft, title: event.target.value })}
                placeholder="Index graph view freezes on large repos"
              />
            </label>
            <label>
              Expected
              <textarea value={draft.expected} onChange={(event) => setDraft({ ...draft, expected: event.target.value })} />
            </label>
            <label>
              Actual
              <textarea value={draft.actual} onChange={(event) => setDraft({ ...draft, actual: event.target.value })} />
            </label>
            <label>
              Reproduction
              <textarea
                value={draft.reproduction}
                onChange={(event) => setDraft({ ...draft, reproduction: event.target.value })}
              />
            </label>
            <label>
              Environment
              <textarea
                value={draft.environment}
                onChange={(event) => setDraft({ ...draft, environment: event.target.value })}
              />
            </label>
          </div>

          <button
            className="button primary full"
            onClick={() => getDesktopApi().submitIssue(draft).then((result) => setNotice(result.message))}
            disabled={Object.values(draft).some((value) => !value.trim())}
          >
            <GitCommitHorizontal size={16} />
            Open Pre-Filled GitHub Issue
          </button>
        </div>
      </div>

      {notice ? <div className="notice">{notice}</div> : null}
    </div>
  );
}

export function App() {
  const [page, setPage] = useState<Page>('indexes');
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = window.localStorage.getItem('graphiq-theme');
    return saved === 'dark' || saved === 'light' ? saved : 'light';
  });

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    window.localStorage.setItem('graphiq-theme', theme);
  }, [theme]);

  return (
    <div className="shell">
      <div className="window-drag-region" aria-hidden="true" />
      <aside className="sidebar">
        <div>
          <div className="brand">
            <div className="brand-mark">
              <img src={graphiqIconUrl} alt="GraphIQ" />
            </div>
            <div>
              <strong>GraphIQ</strong>
              <p>Desktop Index Console</p>
            </div>
          </div>

          <nav className="nav">
            <button className={page === 'indexes' ? 'active' : ''} onClick={() => setPage('indexes')}>
              <Boxes size={18} />
              <span>Indexes</span>
            </button>
            <button className={page === 'connectors' ? 'active' : ''} onClick={() => setPage('connectors')}>
              <Cable size={18} />
              <span>Connectors</span>
            </button>
            <button className={page === 'ontology' ? 'active' : ''} onClick={() => setPage('ontology')}>
              <Network size={18} />
              <span>Ontology</span>
            </button>
          </nav>
        </div>

        <div className="sidebar-bottom">
          <button onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')}>
            {theme === 'light' ? <Moon size={18} /> : <Sun size={18} />}
            <span>{theme === 'light' ? 'Dark Mode' : 'Light Mode'}</span>
          </button>
          <button className={page === 'settings' ? 'active' : ''} onClick={() => setPage('settings')}>
            <SettingsIcon size={18} />
            <span>Settings</span>
          </button>
        </div>
      </aside>

      <main className="content">
        {page === 'settings' ? (
          <SettingsPage />
        ) : page === 'connectors' ? (
          <ConnectorsPage onOpenIndexes={() => setPage('indexes')} />
        ) : (
          <WorkspacePage page={page} onOpenIndexes={() => setPage('indexes')} />
        )}
      </main>
    </div>
  );
}
