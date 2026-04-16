use rusqlite::{params, Connection, Result as SqlResult};
use std::path::Path;

use crate::edge::{Edge, EdgeKind};
use crate::symbol::{SourceFile, Symbol, SymbolKind, Visibility};

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '1');

CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    language TEXT NOT NULL,
    content_hash BLOB NOT NULL,
    mtime_ms INTEGER NOT NULL,
    line_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS symbols (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    qualified_name TEXT,
    kind TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    signature TEXT,
    visibility TEXT NOT NULL DEFAULT 'public',
    doc_comment TEXT,
    source TEXT NOT NULL,
    name_decomposed TEXT NOT NULL,
    content_hash BLOB NOT NULL,
    language TEXT NOT NULL,
    metadata TEXT DEFAULT '{}',
    importance REAL NOT NULL DEFAULT 0.5
);

CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);
CREATE INDEX IF NOT EXISTS idx_symbols_qualified ON symbols(qualified_name);

CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
    name,
    name_decomposed,
    qualified_name,
    signature,
    source,
    doc_comment,
    file_path,
    kind,
    language,
    content=symbols,
    content_rowid=id,
    tokenize='porter unicode61'
);

CREATE TRIGGER IF NOT EXISTS symbols_ai AFTER INSERT ON symbols BEGIN
    INSERT INTO symbols_fts(rowid, name, name_decomposed, qualified_name, signature, source, doc_comment, file_path, kind, language)
    SELECT new.id, new.name, new.name_decomposed, new.qualified_name, new.signature, new.source, new.doc_comment, f.path, new.kind, new.language
    FROM files f WHERE f.id = new.file_id;
END;

CREATE TRIGGER IF NOT EXISTS symbols_ad AFTER DELETE ON symbols BEGIN
    INSERT INTO symbols_fts(symbols_fts, rowid, name, name_decomposed, qualified_name, signature, source, doc_comment, file_path, kind, language)
    VALUES ('delete', old.id, old.name, old.name_decomposed, old.qualified_name, old.signature, old.source, old.doc_comment, '', old.kind, old.language);
END;

CREATE TABLE IF NOT EXISTS edges (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 1.0,
    metadata TEXT DEFAULT '{}',
    UNIQUE(source_id, target_id, kind)
);

CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id, kind);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id, kind);
CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);

CREATE TABLE IF NOT EXISTS file_edges (
    id INTEGER PRIMARY KEY,
    source_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    target_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    metadata TEXT DEFAULT '{}',
    UNIQUE(source_file_id, target_file_id, kind)
);

CREATE INDEX IF NOT EXISTS idx_file_edges_source ON file_edges(source_file_id, kind);
CREATE INDEX IF NOT EXISTS idx_file_edges_target ON file_edges(target_file_id, kind);

-- blast_cache: DISPOSABLE. Fully recomputable from edges. Not a source of truth.
CREATE TABLE IF NOT EXISTS blast_cache (
    symbol_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    dependent_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    direction TEXT NOT NULL,
    distance INTEGER NOT NULL,
    edge_kinds TEXT NOT NULL,
    computed_at INTEGER NOT NULL,
    PRIMARY KEY (symbol_id, dependent_id, direction)
);

CREATE INDEX IF NOT EXISTS idx_blast_symbol ON blast_cache(symbol_id, direction);
"#;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("file not found: {0}")]
    FileNotFound(String),
    #[error("symbol not found: {0}")]
    SymbolNotFound(String),
}

pub struct GraphDb {
    conn: Connection,
}

impl GraphDb {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    fn init_schema(&self) -> Result<(), DbError> {
        self.conn.execute_batch(SCHEMA_V1)?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<String, DbError> {
        let version: String = self.conn.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )?;
        Ok(version)
    }

    // --- Files ---

    pub fn upsert_file(
        &self,
        path: &str,
        language: &str,
        content_hash: &str,
        mtime_ms: i64,
        line_count: u32,
    ) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO files (path, language, content_hash, mtime_ms, line_count) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(path) DO UPDATE SET language=?2, content_hash=?3, mtime_ms=?4, line_count=?5",
            params![path, language, content_hash.as_bytes(), mtime_ms, line_count],
        )?;
        let id = self.conn.query_row(
            "SELECT id FROM files WHERE path = ?1",
            params![path],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn get_file_by_path(&self, path: &str) -> Result<Option<SourceFile>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, language, content_hash, mtime_ms, line_count FROM files WHERE path = ?1",
        )?;
        let result = stmt.query_row(params![path], |row| {
            let hash_bytes: Vec<u8> = row.get(3)?;
            Ok(SourceFile {
                id: row.get(0)?,
                path: row.get::<_, String>(1)?.into(),
                language: row.get(2)?,
                content_hash: String::from_utf8_lossy(&hash_bytes).to_string(),
                mtime_ms: row.get(4)?,
                line_count: row.get(5)?,
            })
        });
        match result {
            Ok(f) => Ok(Some(f)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::from(e)),
        }
    }

    pub fn delete_file(&self, path: &str) -> Result<bool, DbError> {
        let deleted = self
            .conn
            .execute("DELETE FROM files WHERE path = ?1", params![path])?;
        Ok(deleted > 0)
    }

    pub fn file_count(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?)
    }

    // --- Symbols ---

    pub fn insert_symbol(&self, sym: &Symbol) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO symbols (file_id, name, qualified_name, kind, line_start, line_end,
             signature, visibility, doc_comment, source, name_decomposed, content_hash,
             language, metadata, importance)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                sym.file_id,
                sym.name,
                sym.qualified_name,
                sym.kind.as_str(),
                sym.line_start,
                sym.line_end,
                sym.signature,
                sym.visibility.as_str(),
                sym.doc_comment,
                sym.source,
                sym.name_decomposed,
                sym.content_hash.as_bytes(),
                sym.language,
                sym.metadata.to_string(),
                sym.importance,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_symbol(&self, id: i64) -> Result<Option<Symbol>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, qualified_name, kind, line_start, line_end,
             signature, visibility, doc_comment, source, name_decomposed, content_hash,
             language, metadata, importance
             FROM symbols WHERE id = ?1",
        )?;
        let result = stmt.query_row(params![id], |row| row_to_symbol(row));
        match result {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::from(e)),
        }
    }

    pub fn symbols_by_file(&self, file_id: i64) -> Result<Vec<Symbol>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, qualified_name, kind, line_start, line_end,
             signature, visibility, doc_comment, source, name_decomposed, content_hash,
             language, metadata, importance
             FROM symbols WHERE file_id = ?1 ORDER BY line_start",
        )?;
        let symbols = stmt.query_map(params![file_id], |row| row_to_symbol(row))?;
        symbols
            .collect::<SqlResult<Vec<_>>>()
            .map_err(DbError::from)
    }

    pub fn symbols_by_name(&self, name: &str) -> Result<Vec<Symbol>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, qualified_name, kind, line_start, line_end,
             signature, visibility, doc_comment, source, name_decomposed, content_hash,
             language, metadata, importance
             FROM symbols WHERE name = ?1",
        )?;
        let symbols = stmt.query_map(params![name], |row| row_to_symbol(row))?;
        symbols
            .collect::<SqlResult<Vec<_>>>()
            .map_err(DbError::from)
    }

    pub fn delete_symbols_for_file(&self, file_id: i64) -> Result<usize, DbError> {
        Ok(self
            .conn
            .execute("DELETE FROM symbols WHERE file_id = ?1", params![file_id])?)
    }

    pub fn symbol_count(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?)
    }

    // --- Edges ---

    pub fn insert_edge(
        &self,
        source_id: i64,
        target_id: i64,
        kind: EdgeKind,
        weight: f64,
        metadata: serde_json::Value,
    ) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO edges (source_id, target_id, kind, weight, metadata) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source_id, target_id, kind) DO UPDATE SET weight=?4, metadata=?5",
            params![source_id, target_id, kind.as_str(), weight, metadata.to_string()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn edges_from(&self, source_id: i64) -> Result<Vec<Edge>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, target_id, kind, weight, metadata FROM edges WHERE source_id = ?1",
        )?;
        let edges = stmt.query_map(params![source_id], row_to_edge)?;
        edges.collect::<SqlResult<Vec<_>>>().map_err(DbError::from)
    }

    pub fn edges_to(&self, target_id: i64) -> Result<Vec<Edge>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, target_id, kind, weight, metadata FROM edges WHERE target_id = ?1",
        )?;
        let edges = stmt.query_map(params![target_id], row_to_edge)?;
        edges.collect::<SqlResult<Vec<_>>>().map_err(DbError::from)
    }

    pub fn edges_by_kind(&self, kind: EdgeKind, limit: i64) -> Result<Vec<Edge>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, target_id, kind, weight, metadata FROM edges WHERE kind = ?1 LIMIT ?2",
        )?;
        let edges = stmt.query_map(params![kind.as_str(), limit], row_to_edge)?;
        edges.collect::<SqlResult<Vec<_>>>().map_err(DbError::from)
    }

    pub fn delete_edges_for_symbols(&self, file_id: i64) -> Result<usize, DbError> {
        Ok(self.conn.execute(
            "DELETE FROM edges WHERE source_id IN (SELECT id FROM symbols WHERE file_id = ?1)
             OR target_id IN (SELECT id FROM symbols WHERE file_id = ?1)",
            params![file_id],
        )?)
    }

    pub fn edge_count(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))?)
    }

    // --- File Edges ---

    pub fn update_importance(&self, symbol_id: i64, importance: f64) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE symbols SET importance = ?1 WHERE id = ?2",
            params![importance, symbol_id],
        )?;
        Ok(())
    }

    pub fn compute_importance_scores(&self) -> Result<Vec<(i64, f64)>, DbError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT
                s.id,
                COALESCE(
                    CASE
                        WHEN total_edges = 0 THEN 0.3
                        ELSE MIN(1.0, 0.3
                            + 0.5 * COALESCE(call_in_degree, 0) / CAST(total_edges AS REAL)
                            + 0.2 * COALESCE(contains_count, 0) / CAST(total_edges AS REAL))
                    END,
                    0.3
                ) as importance
            FROM symbols s
            LEFT JOIN (
                SELECT target_id, COUNT(*) as call_in_degree
                FROM edges WHERE kind = 'calls'
                GROUP BY target_id
            ) calls ON calls.target_id = s.id
            LEFT JOIN (
                SELECT target_id, COUNT(*) as import_in_degree
                FROM edges WHERE kind = 'imports'
                GROUP BY target_id
            ) imports ON imports.target_id = s.id
            LEFT JOIN (
                SELECT source_id, COUNT(*) as contains_count
                FROM edges WHERE kind = 'contains'
                GROUP BY source_id
            ) contained ON contained.source_id = s.id
            CROSS JOIN (
                SELECT COUNT(*) as total_edges FROM edges
            )
            "#,
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?)))?;
        rows.collect::<SqlResult<Vec<_>>>().map_err(DbError::from)
    }

    pub fn insert_file_edge(
        &self,
        source_file_id: i64,
        target_file_id: i64,
        kind: &str,
    ) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO file_edges (source_file_id, target_file_id, kind) VALUES (?1, ?2, ?3)
             ON CONFLICT(source_file_id, target_file_id, kind) DO NOTHING",
            params![source_file_id, target_file_id, kind],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn file_edge_count(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM file_edges", [], |row| row.get(0))?)
    }

    // --- Stats ---

    pub fn stats(&self) -> Result<DbStats, DbError> {
        Ok(DbStats {
            files: self.file_count()?,
            symbols: self.symbol_count()?,
            edges: self.edge_count()?,
            file_edges: self.file_edge_count()?,
            schema_version: self.schema_version()?,
        })
    }
}

#[derive(Debug)]
pub struct DbStats {
    pub files: i64,
    pub symbols: i64,
    pub edges: i64,
    pub file_edges: i64,
    pub schema_version: String,
}

fn row_to_symbol(row: &rusqlite::Row) -> SqlResult<Symbol> {
    let hash_bytes: Vec<u8> = row.get(12)?;
    let kind_str: String = row.get(4)?;
    let vis_str: String = row.get(8)?;
    let meta_str: String = row.get(14)?;

    Ok(Symbol {
        id: row.get(0)?,
        file_id: row.get(1)?,
        name: row.get(2)?,
        qualified_name: row.get(3)?,
        kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Section),
        line_start: row.get(5)?,
        line_end: row.get(6)?,
        signature: row.get(7)?,
        visibility: Visibility::from_str(&vis_str).unwrap_or(Visibility::Public),
        doc_comment: row.get(9)?,
        source: row.get(10)?,
        name_decomposed: row.get(11)?,
        content_hash: String::from_utf8_lossy(&hash_bytes).to_string(),
        language: row.get(13)?,
        metadata: serde_json::from_str(&meta_str).unwrap_or(serde_json::Value::Null),
        importance: row.get(15)?,
    })
}

fn row_to_edge(row: &rusqlite::Row) -> SqlResult<Edge> {
    let kind_str: String = row.get(3)?;
    let meta_str: String = row.get(5)?;
    Ok(Edge {
        id: row.get(0)?,
        source_id: row.get(1)?,
        target_id: row.get(2)?,
        kind: EdgeKind::from_str(&kind_str).unwrap_or(EdgeKind::References),
        weight: row.get(4)?,
        metadata: serde_json::from_str(&meta_str).unwrap_or(serde_json::Value::Null),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolBuilder;

    #[test]
    fn test_open_in_memory() {
        let db = GraphDb::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), "1");
    }

    #[test]
    fn test_upsert_and_get_file() {
        let db = GraphDb::open_in_memory().unwrap();
        let id = db
            .upsert_file("src/main.ts", "typescript", "abc123", 1000, 50)
            .unwrap();
        assert!(id > 0);

        let f = db.get_file_by_path("src/main.ts").unwrap().unwrap();
        assert_eq!(f.path, std::path::PathBuf::from("src/main.ts"));
        assert_eq!(f.language, "typescript");

        let missing = db.get_file_by_path("nope.ts").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_delete_file_cascades() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/main.ts", "typescript", "abc", 1000, 10)
            .unwrap();

        let sym = SymbolBuilder::new(
            fid,
            "main".into(),
            SymbolKind::Function,
            "fn main() {}".into(),
            "typescript".into(),
        )
        .lines(1, 1)
        .build();
        let sid = db.insert_symbol(&sym).unwrap();

        let edges_before = db.edges_from(sid).unwrap();
        assert!(edges_before.is_empty());

        db.delete_file("src/main.ts").unwrap();
        assert!(db.get_symbol(sid).unwrap().is_none());
    }

    #[test]
    fn test_insert_and_get_symbol() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/auth.ts", "typescript", "abc", 1000, 100)
            .unwrap();

        let sym = SymbolBuilder::new(
            fid,
            "authenticateUser".into(),
            SymbolKind::Function,
            "fn authenticateUser(): User".into(),
            "typescript".into(),
        )
        .lines(10, 25)
        .signature("fn authenticateUser(token: string): Promise<User>")
        .build();

        let id = db.insert_symbol(&sym).unwrap();
        let fetched = db.get_symbol(id).unwrap().unwrap();

        assert_eq!(fetched.name, "authenticateUser");
        assert_eq!(fetched.name_decomposed, "authenticate user");
        assert_eq!(fetched.kind, SymbolKind::Function);
        assert_eq!(fetched.line_start, 10);
        assert_eq!(fetched.line_end, 25);
    }

    #[test]
    fn test_symbols_by_name() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/auth.ts", "typescript", "abc", 1000, 100)
            .unwrap();

        let sym = SymbolBuilder::new(
            fid,
            "handleAuth".into(),
            SymbolKind::Function,
            "fn handleAuth() {}".into(),
            "typescript".into(),
        )
        .lines(1, 5)
        .build();
        db.insert_symbol(&sym).unwrap();

        let found = db.symbols_by_name("handleAuth").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "handleAuth");
    }

    #[test]
    fn test_symbols_by_file() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/auth.ts", "typescript", "abc", 1000, 100)
            .unwrap();

        for (i, (name, kind)) in [
            ("foo", SymbolKind::Function),
            ("Bar", SymbolKind::Class),
            ("baz", SymbolKind::Method),
        ]
        .iter()
        .enumerate()
        {
            let sym = SymbolBuilder::new(
                fid,
                name.to_string(),
                *kind,
                format!("fn {name}()"),
                "typescript".into(),
            )
            .lines(i as u32 + 1, i as u32 + 5)
            .build();
            db.insert_symbol(&sym).unwrap();
        }

        let syms = db.symbols_by_file(fid).unwrap();
        assert_eq!(syms.len(), 3);
    }

    #[test]
    fn test_insert_and_query_edges() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/app.ts", "typescript", "abc", 1000, 100)
            .unwrap();

        let s1 = SymbolBuilder::new(
            fid,
            "foo".into(),
            SymbolKind::Function,
            "fn foo()".into(),
            "typescript".into(),
        )
        .lines(1, 5)
        .build();
        let s2 = SymbolBuilder::new(
            fid,
            "bar".into(),
            SymbolKind::Function,
            "fn bar()".into(),
            "typescript".into(),
        )
        .lines(7, 10)
        .build();
        let id1 = db.insert_symbol(&s1).unwrap();
        let id2 = db.insert_symbol(&s2).unwrap();

        db.insert_edge(id1, id2, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();

        let from = db.edges_from(id1).unwrap();
        assert_eq!(from.len(), 1);
        assert_eq!(from[0].kind, EdgeKind::Calls);
        assert_eq!(from[0].target_id, id2);

        let to = db.edges_to(id2).unwrap();
        assert_eq!(to.len(), 1);
        assert_eq!(to[0].source_id, id1);
    }

    #[test]
    fn test_stats() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/main.ts", "typescript", "abc", 1000, 50)
            .unwrap();
        let sym = SymbolBuilder::new(
            fid,
            "main".into(),
            SymbolKind::Function,
            "fn main()".into(),
            "typescript".into(),
        )
        .lines(1, 5)
        .build();
        db.insert_symbol(&sym).unwrap();

        let stats = db.stats().unwrap();
        assert_eq!(stats.files, 1);
        assert_eq!(stats.symbols, 1);
        assert_eq!(stats.edges, 0);
        assert_eq!(stats.schema_version, "1");
    }

    #[test]
    fn test_file_edges() {
        let db = GraphDb::open_in_memory().unwrap();
        let f1 = db
            .upsert_file("src/a.ts", "typescript", "a", 1000, 10)
            .unwrap();
        let f2 = db
            .upsert_file("src/b.ts", "typescript", "b", 1000, 20)
            .unwrap();
        db.insert_file_edge(f1, f2, "imports").unwrap();
        assert_eq!(db.file_edge_count().unwrap(), 1);
    }
}
