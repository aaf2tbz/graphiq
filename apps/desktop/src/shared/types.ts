export type IndexSummary = {
  id: string;
  name: string;
  projectPath: string;
  dbPath: string;
  manifestPath: string;
  files: number;
  symbols: number;
  edges: number;
  createdAt: string | null;
  lastIndexedAt: string | null;
  lastAccessedAt: string | null;
  activeMode: string | null;
  languages: string[];
  health: 'ready' | 'stale' | 'missing';
};

export type RankedSymbol = {
  id: number;
  name: string;
  qualifiedName: string | null;
  kind: string;
  importance: number;
  filePath: string;
  language: string;
  inbound: number;
  outbound: number;
};

export type GraphNode = {
  id: number;
  name: string;
  kind: string;
  importance: number;
  inbound: number;
  outbound: number;
  language?: string;
};

export type GraphLink = {
  source: number;
  target: number;
  kind: string;
  weight: number;
};

export type SignalEntry = {
  label: string;
  value: string;
  supporting: string;
};

export type EdgeStat = {
  label: string;
  count: number;
  detail: string;
  entries: SignalEntry[];
};

export type IndexTreeItem = {
  id: string;
  type: 'extension' | 'file';
  name: string;
  path: string | null;
  extension: string;
  language: string;
  fileCount: number;
  lineCount: number;
  symbolCount: number;
  inbound: number;
  outbound: number;
  children?: IndexTreeItem[];
};

export type GraphSnapshot = {
  nodes: GraphNode[];
  links: GraphLink[];
  edgeKinds: string[];
};

export type IndexDetails = {
  summary: IndexSummary;
  graph: GraphSnapshot;
  tree: IndexTreeItem[];
  signals: EdgeStat[];
  rankedSymbols: RankedSymbol[];
};

export type IndexActionResult = {
  ok: boolean;
  message: string;
  details?: string;
};

export type UpdateStatus = {
  currentVersion: string;
  latestVersion: string | null;
  updateAvailable: boolean;
  releaseUrl: string;
  notes?: string;
};

export type IssueDraft = {
  title: string;
  expected: string;
  actual: string;
  reproduction: string;
  environment: string;
};

export type ConnectorStatus = {
  id: string;
  name: string;
  surface: 'native' | 'signet' | 'config';
  status: 'connected' | 'available' | 'missing';
  active: boolean;
  configPaths: string[];
  detail: string;
  lastSeenAt: string | null;
};

export type DesktopApi = {
  listIndexes: (forceRefresh?: boolean) => Promise<IndexSummary[]>;
  getIndexDetails: (projectPath: string) => Promise<IndexDetails>;
  indexProject: (projectPath: string) => Promise<IndexActionResult>;
  deleteIndex: (projectPath: string) => Promise<IndexActionResult>;
  listConnectors: () => Promise<ConnectorStatus[]>;
  pairConnector: (connectorId: string) => Promise<IndexActionResult>;
  unpairConnector: (connectorId: string) => Promise<IndexActionResult>;
  chooseProjectFolder: () => Promise<string | null>;
  checkForUpdates: () => Promise<UpdateStatus>;
  pullLatestMain: () => Promise<IndexActionResult>;
  uninstallApp: () => Promise<IndexActionResult>;
  submitIssue: (draft: IssueDraft) => Promise<IndexActionResult>;
};
