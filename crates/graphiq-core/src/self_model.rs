use std::collections::{HashMap, HashSet};

use rusqlite::params;

use crate::db::GraphDb;
use crate::subsystems::{self, SubsystemIndex};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ConceptKind {
    Subsystem,
    Concern,
    ErrorSurface,
    TestSurface,
    PublicApi,
}

impl ConceptKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConceptKind::Subsystem => "subsystem",
            ConceptKind::Concern => "concern",
            ConceptKind::ErrorSurface => "error_surface",
            ConceptKind::TestSurface => "test_surface",
            ConceptKind::PublicApi => "public_api",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "subsystem" => Some(ConceptKind::Subsystem),
            "concern" => Some(ConceptKind::Concern),
            "error_surface" => Some(ConceptKind::ErrorSurface),
            "test_surface" => Some(ConceptKind::TestSurface),
            "public_api" => Some(ConceptKind::PublicApi),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConceptNode {
    pub id: i64,
    pub kind: ConceptKind,
    pub name: String,
    pub vocabulary: Vec<String>,
    pub key_symbol_ids: Vec<i64>,
    pub key_symbol_names: Vec<String>,
    pub representative_symbol_id: Option<i64>,
    pub representative_symbol_name: Option<String>,
    pub boundary_symbol_ids: Vec<i64>,
    pub file_paths: Vec<String>,
    pub description: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct RepoSelfModel {
    pub concepts: Vec<ConceptNode>,
    pub subsystem_index: SubsystemIndex,
    vocab_index: HashMap<String, Vec<usize>>,
}

fn tokenize_text(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let stop_words: HashSet<&str> = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "shall", "can", "to", "of", "in", "for",
        "on", "with", "at", "by", "from", "as", "into", "through", "during",
        "before", "after", "above", "below", "between", "and", "but", "or",
        "not", "no", "nor", "so", "yet", "both", "either", "neither", "each",
        "every", "all", "any", "few", "more", "most", "other", "some", "such",
        "than", "too", "very", "just", "about", "up", "out", "off", "over",
        "then", "once", "here", "there", "when", "where", "why", "how", "what",
        "which", "who", "whom", "this", "that", "these", "those", "its", "it",
        "if", "else", "while", "return", "fn", "function", "let", "const",
        "var", "struct", "class", "impl", "pub", "use", "mod", "self",
    ].iter().cloned().collect();

    lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() >= 2)
        .filter(|t| !stop_words.contains(t))
        .map(|t| t.to_string())
        .collect()
}

fn symbol_vocabulary(db: &GraphDb, symbol_id: i64) -> Vec<String> {
    let conn = db.conn();
    let mut terms: HashMap<String, usize> = HashMap::new();

    if let Ok(mut stmt) = conn.prepare(
        "SELECT name, signature, doc_comment, source, search_hints FROM symbols WHERE id = ?1"
    ) {
        if let Ok(rows) = stmt.query_map(params![symbol_id], |row| {
            Ok((
                row.get::<_, String>(0).unwrap_or_default(),
                row.get::<_, Option<String>>(1).unwrap_or_default().unwrap_or_default(),
                row.get::<_, Option<String>>(2).unwrap_or_default().unwrap_or_default(),
                row.get::<_, String>(3).unwrap_or_default(),
                row.get::<_, Option<String>>(4).unwrap_or_default().unwrap_or_default(),
            ))
        }) {
            for row in rows.flatten() {
                let (name, sig, doc, source, hints) = row;
                for t in tokenize_text(&name) { *terms.entry(t).or_default() += 3; }
                for t in tokenize_text(&sig) { *terms.entry(t).or_default() += 2; }
                for t in tokenize_text(&doc) { *terms.entry(t).or_default() += 2; }
                let src_safe: String = source.chars().take(2000).collect();
                for t in tokenize_text(&src_safe) { *terms.entry(t).or_default() += 1; }
                for t in tokenize_text(&hints) { *terms.entry(t).or_default() += 4; }
            }
        }
    }

    let mut sorted: Vec<(String, usize)> = terms.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted.truncate(20);
    sorted.into_iter().map(|(t, _)| t).collect()
}

fn build_subsystem_concepts(
    db: &GraphDb,
    subsystem_index: &SubsystemIndex,
) -> Vec<ConceptNode> {
    let mut concepts = Vec::new();
    let mut next_id = 1i64;

    for sub in &subsystem_index.subsystems {
        if sub.symbol_ids.len() < 3 {
            continue;
        }

        let mut vocab: HashMap<String, usize> = HashMap::new();
        let mut file_set: HashSet<String> = HashSet::new();
        let mut boundary_ids: Vec<i64> = Vec::new();
        let mut best_rep: Option<(i64, String, usize)> = None;

        for &sid in &sub.symbol_ids {
            for t in symbol_vocabulary(db, sid) {
                *vocab.entry(t).or_default() += 1;
            }

            let file_path = db.conn()
                .query_row(
                    "SELECT f.path FROM symbols s JOIN files f ON s.file_id = f.id WHERE s.id = ?1",
                    params![sid],
                    |row| row.get::<_, String>(0),
                )
                .ok();
            if let Some(ref fp) = file_path {
                file_set.insert(fp.clone());
            }

            let out_count = db.edges_from(sid).map(|e| e.len()).unwrap_or(0);
            let in_count = db.edges_to(sid).map(|e| e.len()).unwrap_or(0);
            let degree = out_count + in_count;

            let vis = db.conn()
                .query_row("SELECT visibility FROM symbols WHERE id = ?1", params![sid], |row| row.get::<_, String>(0))
                .unwrap_or_default();
            let is_public = vis == "public" || vis == "exported";

            if is_public && degree > best_rep.as_ref().map(|(_, _, d)| *d).unwrap_or(0) {
                let name = db.conn()
                    .query_row("SELECT name FROM symbols WHERE id = ?1", params![sid], |row| row.get::<_, String>(0))
                    .unwrap_or_default();
                best_rep = Some((sid, name, degree));
            }

            let is_boundary = db.edges_from(sid).unwrap_or_default().iter()
                .any(|e| {
                    let src_sub = subsystem_index.symbol_to_subsystem.get(&e.source_id);
                    let tgt_sub = subsystem_index.symbol_to_subsystem.get(&e.target_id);
                    src_sub != tgt_sub
                }) || db.edges_to(sid).unwrap_or_default().iter()
                .any(|e| {
                    let src_sub = subsystem_index.symbol_to_subsystem.get(&e.source_id);
                    let tgt_sub = subsystem_index.symbol_to_subsystem.get(&e.target_id);
                    src_sub != tgt_sub
                });
            if is_boundary {
                boundary_ids.push(sid);
            }
        }

        let mut vocab_sorted: Vec<(String, usize)> = vocab.into_iter().collect();
        vocab_sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let vocab_top: Vec<String> = vocab_sorted.iter().take(15).map(|(t, _)| t.clone()).collect();

        let key_names: Vec<String> = sub.symbol_ids.iter().take(10)
            .filter_map(|&sid| {
                db.conn().query_row("SELECT name FROM symbols WHERE id = ?1", params![sid], |row| row.get::<_, String>(0)).ok()
            })
            .collect();

        let desc = if vocab_top.len() >= 3 {
            format!("{} subsystem: {}", sub.name, vocab_top[..5.min(vocab_top.len())].join(", "))
        } else {
            format!("{} subsystem ({} symbols)", sub.name, sub.symbol_ids.len())
        };

        concepts.push(ConceptNode {
            id: next_id,
            kind: ConceptKind::Subsystem,
            name: format!("Subsystem:{}", sub.name),
            vocabulary: vocab_top,
            key_symbol_ids: sub.symbol_ids.iter().take(15).copied().collect(),
            key_symbol_names: key_names,
            representative_symbol_id: best_rep.as_ref().map(|(id, _, _)| *id),
            representative_symbol_name: best_rep.as_ref().map(|(_, n, _)| n.clone()),
            boundary_symbol_ids: boundary_ids,
            file_paths: file_set.into_iter().collect(),
            description: desc,
        });
        next_id += 1;
    }

    concepts
}

fn build_concern_concepts(
    db: &GraphDb,
    subsystem_index: &SubsystemIndex,
    subsystem_concepts: &[ConceptNode],
) -> Vec<ConceptNode> {
    let mut concern_vocab: HashMap<String, HashSet<i64>> = HashMap::new();

    for concept in subsystem_concepts {
        for term in &concept.vocabulary {
            concern_vocab
                .entry(term.clone())
                .or_default()
                .extend(concept.key_symbol_ids.iter().copied());
        }
    }

    let mut cross_subsystem_terms: HashMap<String, HashSet<usize>> = HashMap::new();
    for concept in subsystem_concepts {
        let sub_id = subsystem_index.subsystems.iter()
            .position(|s| s.name == concept.name.strip_prefix("Subsystem:").unwrap_or(&concept.name))
            .unwrap_or(0);
        for term in &concept.vocabulary {
            cross_subsystem_terms
                .entry(term.clone())
                .or_default()
                .insert(sub_id);
        }
    }

    let mut concern_clusters: HashMap<String, Vec<i64>> = HashMap::new();
    for (term, sub_ids) in &cross_subsystem_terms {
        if sub_ids.len() >= 2 {
            let symbols = concern_vocab.get(term).cloned().unwrap_or_default();
            if symbols.len() >= 3 {
                concern_clusters.insert(term.clone(), symbols.into_iter().collect());
            }
        }
    }

    let mut concepts = Vec::new();
    let mut next_id = 1000i64;

    let mut sorted_concerns: Vec<_> = concern_clusters.iter().collect();
    sorted_concerns.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    for (term, symbol_ids) in sorted_concerns.iter().take(20) {
        let names: Vec<String> = symbol_ids.iter().take(10)
            .filter_map(|&sid| {
                db.conn().query_row("SELECT name FROM symbols WHERE id = ?1", params![sid], |row| row.get::<_, String>(0)).ok()
            })
            .collect();

        let file_paths: Vec<String> = symbol_ids.iter().take(20)
            .filter_map(|&sid| {
                db.conn().query_row(
                    "SELECT f.path FROM symbols s JOIN files f ON s.file_id = f.id WHERE s.id = ?1",
                    params![sid],
                    |row| row.get::<_, String>(0),
                ).ok()
            })
            .collect();

        let mut all_vocab: HashSet<String> = HashSet::new();
        all_vocab.insert(term.to_string());
        for &sid in symbol_ids.iter().take(10) {
            for t in symbol_vocabulary(db, sid) {
                if t.as_str() != term.as_str() {
                    all_vocab.insert(t);
                }
            }
        }
        let vocab: Vec<String> = all_vocab.into_iter().take(15).collect();

        concepts.push(ConceptNode {
            id: next_id,
            kind: ConceptKind::Concern,
            name: format!("Concern:{}", term),
            vocabulary: vocab,
            key_symbol_ids: symbol_ids.iter().take(15).copied().collect(),
            key_symbol_names: names,
            representative_symbol_id: symbol_ids.first().copied(),
            representative_symbol_name: None,
            boundary_symbol_ids: Vec::new(),
            file_paths,
            description: format!("cross-cutting concern '{}' across {} symbols", term, symbol_ids.len()),
        });
        next_id += 1;
    }

    concepts
}

fn build_error_surface_concepts(db: &GraphDb) -> Vec<ConceptNode> {
    let conn = db.conn();
    let mut concepts = Vec::new();
    let mut next_id = 2000i64;

    let error_symbols: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, name FROM symbols WHERE name LIKE '%error%' OR name LIKE '%Error%' OR name LIKE '%fail%' OR name LIKE '%Fail%' OR name LIKE '%panic%' OR name LIKE '%throw%' OR name LIKE '%catch%' OR name LIKE '%handle%Error%' OR name LIKE '%handle%Fail%'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .ok()
            .map(|r| r.flatten().collect())
            .unwrap_or_default()
    };

    if error_symbols.is_empty() {
        return concepts;
    }

    let mut error_vocab: HashMap<String, usize> = HashMap::new();
    let mut error_file_set: HashSet<String> = HashSet::new();

    for &(sid, ref name) in &error_symbols {
        for t in symbol_vocabulary(db, sid) {
            *error_vocab.entry(t).or_default() += 1;
        }
        let fp = db.conn().query_row(
            "SELECT f.path FROM symbols s JOIN files f ON s.file_id = f.id WHERE s.id = ?1",
            params![sid],
            |row| row.get::<_, String>(0),
        ).ok();
        if let Some(f) = fp {
            error_file_set.insert(f);
        }
    }

    let mut vocab_sorted: Vec<(String, usize)> = error_vocab.into_iter().collect();
    vocab_sorted.sort_by(|a, b| b.1.cmp(&a.1));
    let vocab: Vec<String> = vocab_sorted.iter().take(15).map(|(t, _)| t.clone()).collect();

    let symbol_ids: Vec<i64> = error_symbols.iter().take(20).map(|(id, _)| *id).collect();
    let names: Vec<String> = error_symbols.iter().take(10).map(|(_, n)| n.clone()).collect();

    concepts.push(ConceptNode {
        id: next_id,
        kind: ConceptKind::ErrorSurface,
        name: "Surface:error_handling".into(),
        vocabulary: vocab,
        key_symbol_ids: symbol_ids,
        key_symbol_names: names,
        representative_symbol_id: error_symbols.first().map(|(id, _)| *id),
        representative_symbol_name: error_symbols.first().map(|(_, n)| n.clone()),
        boundary_symbol_ids: Vec::new(),
        file_paths: error_file_set.into_iter().take(10).collect(),
        description: format!("error handling surface ({} error-related symbols)", error_symbols.len()),
    });

    concepts
}

fn build_test_surface_concepts(db: &GraphDb) -> Vec<ConceptNode> {
    let conn = db.conn();
    let mut concepts = Vec::new();
    let mut next_id = 3000i64;

    let test_symbols: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name FROM symbols s JOIN files f ON s.file_id = f.id WHERE f.path LIKE '%test%' OR f.path LIKE '%spec%' OR s.name LIKE '%test%' OR s.name LIKE '%Test%' OR s.name LIKE '%spec%' OR s.name LIKE '%Spec%'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .ok()
            .map(|r| r.flatten().collect())
            .unwrap_or_default()
    };

    if test_symbols.is_empty() {
        return concepts;
    }

    let mut test_vocab: HashMap<String, usize> = HashMap::new();
    let mut test_file_set: HashSet<String> = HashSet::new();

    for &(sid, _) in &test_symbols {
        for t in symbol_vocabulary(db, sid) {
            *test_vocab.entry(t).or_default() += 1;
        }
        let fp = db.conn().query_row(
            "SELECT f.path FROM symbols s JOIN files f ON s.file_id = f.id WHERE s.id = ?1",
            params![sid],
            |row| row.get::<_, String>(0),
        ).ok();
        if let Some(f) = fp {
            test_file_set.insert(f);
        }
    }

    let mut vocab_sorted: Vec<(String, usize)> = test_vocab.into_iter().collect();
    vocab_sorted.sort_by(|a, b| b.1.cmp(&a.1));
    let vocab: Vec<String> = vocab_sorted.iter().take(15).map(|(t, _)| t.clone()).collect();

    let symbol_ids: Vec<i64> = test_symbols.iter().take(20).map(|(id, _)| *id).collect();
    let names: Vec<String> = test_symbols.iter().take(10).map(|(_, n)| n.clone()).collect();

    concepts.push(ConceptNode {
        id: next_id,
        kind: ConceptKind::TestSurface,
        name: "Surface:test_suite".into(),
        vocabulary: vocab,
        key_symbol_ids: symbol_ids,
        key_symbol_names: names,
        representative_symbol_id: test_symbols.first().map(|(id, _)| *id),
        representative_symbol_name: test_symbols.first().map(|(_, n)| n.clone()),
        boundary_symbol_ids: Vec::new(),
        file_paths: test_file_set.into_iter().take(10).collect(),
        description: format!("test surface ({} test symbols)", test_symbols.len()),
    });

    concepts
}

fn build_public_api_concepts(db: &GraphDb) -> Vec<ConceptNode> {
    let conn = db.conn();
    let mut concepts = Vec::new();
    let mut next_id = 4000i64;

    let public_symbols: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, name FROM symbols WHERE visibility IN ('public', 'exported') ORDER BY id"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .ok()
            .map(|r| r.flatten().collect())
            .unwrap_or_default()
    };

    if public_symbols.len() < 5 {
        return concepts;
    }

    let file_groups: HashMap<String, Vec<(i64, String)>> = {
        let mut groups: HashMap<String, Vec<(i64, String)>> = HashMap::new();
        for &(sid, ref name) in &public_symbols {
            let dir = db.conn().query_row(
                "SELECT f.path FROM symbols s JOIN files f ON s.file_id = f.id WHERE s.id = ?1",
                params![sid],
                |row| row.get::<_, String>(0),
            ).ok().map(|p| {
                let parts: Vec<&str> = p.split('/').collect();
                if parts.len() >= 2 {
                    format!("{}/{}", parts[parts.len()-2], parts[parts.len()-1])
                } else {
                    p
                }
            }).unwrap_or_default();
            groups.entry(dir).or_default().push((sid, name.clone()));
        }
        groups
    };

    for (dir, symbols) in file_groups.iter() {
        if symbols.len() < 3 {
            continue;
        }

        let mut vocab: HashMap<String, usize> = HashMap::new();
        for &(sid, _) in symbols.iter().take(15) {
            for t in symbol_vocabulary(db, sid) {
                *vocab.entry(t).or_default() += 1;
            }
        }

        let mut vocab_sorted: Vec<(String, usize)> = vocab.into_iter().collect();
        vocab_sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let vocab_top: Vec<String> = vocab_sorted.iter().take(10).map(|(t, _)| t.clone()).collect();

        let sym_ids: Vec<i64> = symbols.iter().take(10).map(|(id, _)| *id).collect();
        let sym_names: Vec<String> = symbols.iter().take(10).map(|(_, n)| n.clone()).collect();

        concepts.push(ConceptNode {
            id: next_id,
            kind: ConceptKind::PublicApi,
            name: format!("PublicApi:{}", dir),
            vocabulary: vocab_top,
            key_symbol_ids: sym_ids,
            key_symbol_names: sym_names,
            representative_symbol_id: symbols.first().map(|(id, _)| *id),
            representative_symbol_name: symbols.first().map(|(_, n)| n.clone()),
            boundary_symbol_ids: Vec::new(),
            file_paths: vec![dir.clone()],
            description: format!("public API surface in {} ({} symbols)", dir, symbols.len()),
        });
        next_id += 1;

        if concepts.len() >= 30 {
            break;
        }
    }

    concepts
}

pub fn build_self_model(db: &GraphDb) -> Result<RepoSelfModel, String> {
    let subsystem_index = if let Ok(idx) = subsystems::load_subsystems(db) {
        if idx.subsystems.is_empty() {
            subsystems::detect_subsystems(db)?
        } else {
            idx
        }
    } else {
        subsystems::detect_subsystems(db)?
    };

    let mut all_concepts = Vec::new();

    let subsystem_concepts = build_subsystem_concepts(db, &subsystem_index);
    all_concepts.extend(subsystem_concepts.clone());

    let concern_concepts = build_concern_concepts(db, &subsystem_index, &subsystem_concepts);
    all_concepts.extend(concern_concepts);

    let error_concepts = build_error_surface_concepts(db);
    all_concepts.extend(error_concepts);

    let test_concepts = build_test_surface_concepts(db);
    all_concepts.extend(test_concepts);

    let public_api_concepts = build_public_api_concepts(db);
    all_concepts.extend(public_api_concepts);

    let mut vocab_index: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, concept) in all_concepts.iter().enumerate() {
        for term in &concept.vocabulary {
            vocab_index.entry(term.clone()).or_default().push(i);
        }
    }

    Ok(RepoSelfModel {
        concepts: all_concepts,
        subsystem_index,
        vocab_index,
    })
}

impl RepoSelfModel {
    pub fn search_concepts(&self, query: &str, top_k: usize) -> Vec<(usize, f64)> {
        let query_terms: HashSet<String> = tokenize_text(query).into_iter().collect();
        if query_terms.is_empty() {
            return Vec::new();
        }

        let mut scores: HashMap<usize, f64> = HashMap::new();

        for qt in &query_terms {
            if let Some(concept_indices) = self.vocab_index.get(qt) {
                for &ci in concept_indices {
                    let concept = &self.concepts[ci];
                    let vocab_set: HashSet<&str> = concept.vocabulary.iter().map(|s| s.as_str()).collect();
                    let overlap = query_terms.iter().filter(|t| vocab_set.contains(t.as_str())).count();

                    let kind_boost = match concept.kind {
                        ConceptKind::Subsystem => 1.0,
                        ConceptKind::Concern => 1.5,
                        ConceptKind::ErrorSurface => 1.2,
                        ConceptKind::TestSurface => 0.8,
                        ConceptKind::PublicApi => 1.0,
                    };

                    let size_norm = 1.0 / (concept.key_symbol_ids.len() as f64).sqrt().max(1.0);
                    let score = overlap as f64 * kind_boost * size_norm;
                    *scores.entry(ci).or_default() += score;
                }
            }
        }

        let mut scored: Vec<(usize, f64)> = scores.into_iter().collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    pub fn expand_query(&self, query: &str) -> Vec<(i64, f64)> {
        let matched_concepts = self.search_concepts(query, 5);
        let mut symbol_scores: HashMap<i64, f64> = HashMap::new();

        for (ci, concept_score) in &matched_concepts {
            let concept = &self.concepts[*ci];
            for &sid in &concept.key_symbol_ids {
                *symbol_scores.entry(sid).or_default() += concept_score;
            }
            if let Some(rep_id) = concept.representative_symbol_id {
                *symbol_scores.entry(rep_id).or_default() += concept_score * 2.0;
            }
            for &bid in &concept.boundary_symbol_ids {
                *symbol_scores.entry(bid).or_default() += concept_score * 0.5;
            }
        }

        let mut results: Vec<(i64, f64)> = symbol_scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }
}

pub fn store_self_model(db: &GraphDb, model: &RepoSelfModel) -> Result<(), String> {
    let conn = db.conn();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS concept_nodes (
            id INTEGER PRIMARY KEY,
            kind TEXT NOT NULL,
            name TEXT NOT NULL,
            vocabulary TEXT NOT NULL,
            key_symbol_ids TEXT NOT NULL,
            key_symbol_names TEXT NOT NULL,
            representative_symbol_id INTEGER,
            representative_symbol_name TEXT,
            boundary_symbol_ids TEXT NOT NULL,
            file_paths TEXT NOT NULL,
            description TEXT NOT NULL
        )",
        [],
    ).map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM concept_nodes", [])
        .map_err(|e| e.to_string())?;

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO concept_nodes (id, kind, name, vocabulary, key_symbol_ids, key_symbol_names, representative_symbol_id, representative_symbol_name, boundary_symbol_ids, file_paths, description)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
        ).map_err(|e| e.to_string())?;

        for concept in &model.concepts {
            stmt.execute(params![
                concept.id,
                concept.kind.as_str(),
                concept.name,
                serde_json::to_string(&concept.vocabulary).unwrap_or_default(),
                serde_json::to_string(&concept.key_symbol_ids).unwrap_or_default(),
                serde_json::to_string(&concept.key_symbol_names).unwrap_or_default(),
                concept.representative_symbol_id,
                concept.representative_symbol_name,
                serde_json::to_string(&concept.boundary_symbol_ids).unwrap_or_default(),
                serde_json::to_string(&concept.file_paths).unwrap_or_default(),
                concept.description,
            ]).map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;

    Ok(())
}

pub fn load_self_model(db: &GraphDb) -> Result<RepoSelfModel, String> {
    let subsystem_index = if let Ok(idx) = subsystems::load_subsystems(db) {
        if idx.subsystems.is_empty() {
            subsystems::detect_subsystems(db)?
        } else {
            idx
        }
    } else {
        subsystems::detect_subsystems(db)?
    };

    let conn = db.conn();
    let table_exists: bool = conn
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='concept_nodes'")
        .map_err(|e| e.to_string())?
        .query_row([], |row| row.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        > 0;

    if !table_exists {
        return build_self_model(db);
    }

    let mut concepts = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, kind, name, vocabulary, key_symbol_ids, key_symbol_names, representative_symbol_id, representative_symbol_name, boundary_symbol_ids, file_paths, description FROM concept_nodes ORDER BY id"
    ).map_err(|e| e.to_string())?;

    let rows: Vec<(i64, String, String, String, String, String, Option<i64>, Option<String>, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    for (id, kind_str, name, vocab_json, key_ids_json, key_names_json, rep_id, rep_name, boundary_json, files_json, desc) in &rows {
        let kind = ConceptKind::from_str(kind_str).unwrap_or(ConceptKind::Subsystem);
        let vocab: Vec<String> = serde_json::from_str(vocab_json).unwrap_or_default();
        let key_ids: Vec<i64> = serde_json::from_str(key_ids_json).unwrap_or_default();
        let key_names: Vec<String> = serde_json::from_str(key_names_json).unwrap_or_default();
        let boundary: Vec<i64> = serde_json::from_str(boundary_json).unwrap_or_default();
        let files: Vec<String> = serde_json::from_str(files_json).unwrap_or_default();

        concepts.push(ConceptNode {
            id: *id,
            kind,
            name: name.clone(),
            vocabulary: vocab,
            key_symbol_ids: key_ids,
            key_symbol_names: key_names,
            representative_symbol_id: *rep_id,
            representative_symbol_name: rep_name.clone(),
            boundary_symbol_ids: boundary,
            file_paths: files,
            description: desc.clone(),
        });
    }

    let mut vocab_index: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, concept) in concepts.iter().enumerate() {
        for term in &concept.vocabulary {
            vocab_index.entry(term.clone()).or_default().push(i);
        }
    }

    Ok(RepoSelfModel {
        concepts,
        subsystem_index,
        vocab_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::EdgeKind;
    use crate::symbol::{SymbolBuilder, SymbolKind, Visibility};

    fn setup_db() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();

        let f1 = db.upsert_file("src/auth/login.rs", "rust", "a", 1000, 50).unwrap();
        let f2 = db.upsert_file("src/auth/session.rs", "rust", "b", 1000, 40).unwrap();
        let f3 = db.upsert_file("src/api/routes.rs", "rust", "c", 1000, 60).unwrap();
        let f4 = db.upsert_file("src/error/handler.rs", "rust", "d", 1000, 50).unwrap();
        let f5 = db.upsert_file("tests/auth_test.rs", "rust", "e", 500, 30).unwrap();

        let s1 = SymbolBuilder::new(f1, "authenticate".into(), SymbolKind::Function, "fn auth(token: &str) -> Result<User, AuthError>".into(), "rust".into())
            .lines(1, 10).visibility(Visibility::Public).build();
        let s2 = SymbolBuilder::new(f1, "validate_token".into(), SymbolKind::Function, "fn validate(t: &str) -> bool".into(), "rust".into())
            .lines(12, 20).build();
        let s3 = SymbolBuilder::new(f2, "create_session".into(), SymbolKind::Function, "fn create(user: User) -> Session".into(), "rust".into())
            .lines(1, 10).visibility(Visibility::Public).build();
        let s4 = SymbolBuilder::new(f2, "destroy_session".into(), SymbolKind::Function, "fn destroy(s: Session)".into(), "rust".into())
            .lines(12, 20).build();
        let s5 = SymbolBuilder::new(f3, "handle_request".into(), SymbolKind::Function, "fn handle(req: Request) -> Response".into(), "rust".into())
            .lines(1, 10).visibility(Visibility::Public).build();
        let s6 = SymbolBuilder::new(f3, "route_path".into(), SymbolKind::Function, "fn route(path: &str) -> Handler".into(), "rust".into())
            .lines(12, 20).visibility(Visibility::Public).build();
        let s7 = SymbolBuilder::new(f4, "handle_error".into(), SymbolKind::Function, "fn handle_error(e: Error) -> Response".into(), "rust".into())
            .lines(1, 10).visibility(Visibility::Public).build();
        let s8 = SymbolBuilder::new(f4, "AuthError".into(), SymbolKind::Struct, "struct AuthError { msg: String }".into(), "rust".into())
            .lines(12, 20).visibility(Visibility::Public).build();
        let s9 = SymbolBuilder::new(f5, "test_authenticate".into(), SymbolKind::Function, "fn test_authenticate()".into(), "rust".into())
            .lines(1, 10).build();

        let id1 = db.insert_symbol(&s1).unwrap();
        let id2 = db.insert_symbol(&s2).unwrap();
        let id3 = db.insert_symbol(&s3).unwrap();
        let id4 = db.insert_symbol(&s4).unwrap();
        let id5 = db.insert_symbol(&s5).unwrap();
        let id6 = db.insert_symbol(&s6).unwrap();
        let id7 = db.insert_symbol(&s7).unwrap();
        let _id8 = db.insert_symbol(&s8).unwrap();
        let _id9 = db.insert_symbol(&s9).unwrap();

        let null_meta = serde_json::Value::Null;
        db.insert_edge(id1, id2, EdgeKind::Calls, 1.0, null_meta.clone()).unwrap();
        db.insert_edge(id3, id4, EdgeKind::Calls, 1.0, null_meta.clone()).unwrap();
        db.insert_edge(id1, id3, EdgeKind::Calls, 1.0, null_meta.clone()).unwrap();
        db.insert_edge(id5, id1, EdgeKind::Calls, 1.0, null_meta.clone()).unwrap();
        db.insert_edge(id5, id7, EdgeKind::Calls, 1.0, null_meta.clone()).unwrap();
        db.insert_edge(id6, id1, EdgeKind::Calls, 1.0, null_meta).unwrap();

        db
    }

    #[test]
    fn test_build_self_model() {
        let db = setup_db();
        let model = build_self_model(&db).unwrap();
        assert!(!model.concepts.is_empty(), "should have at least some concept nodes");
    }

    #[test]
    fn test_subsystem_concepts() {
        let db = setup_db();
        let model = build_self_model(&db).unwrap();
        let subsystems: Vec<_> = model.concepts.iter().filter(|c| matches!(c.kind, ConceptKind::Subsystem)).collect();
        assert!(!subsystems.is_empty(), "should have subsystem concepts");
        for s in &subsystems {
            assert!(!s.vocabulary.is_empty(), "subsystem should have vocabulary");
            assert!(!s.key_symbol_ids.is_empty(), "subsystem should have key symbols");
        }
    }

    #[test]
    fn test_error_surface_concept() {
        let db = setup_db();
        let model = build_self_model(&db).unwrap();
        let errors: Vec<_> = model.concepts.iter().filter(|c| matches!(c.kind, ConceptKind::ErrorSurface)).collect();
        assert!(!errors.is_empty(), "should detect error surface");
    }

    #[test]
    fn test_concept_search() {
        let db = setup_db();
        let model = build_self_model(&db).unwrap();
        if !model.vocab_index.is_empty() {
            let some_term = model.vocab_index.keys().next().unwrap().clone();
            let results = model.search_concepts(&some_term, 5);
            assert!(!results.is_empty(), "should find concepts for indexed term '{}'", some_term);
        }
    }

    #[test]
    fn test_expand_query() {
        let db = setup_db();
        let model = build_self_model(&db).unwrap();
        let expanded = model.expand_query("authentication and session management");
        assert!(!expanded.is_empty(), "should expand to symbols");
    }

    #[test]
    fn test_store_load_roundtrip() {
        let db = setup_db();
        let original = build_self_model(&db).unwrap();
        store_self_model(&db, &original).unwrap();
        let loaded = load_self_model(&db).unwrap();

        assert_eq!(original.concepts.len(), loaded.concepts.len(), "concept count should match");
        for (o, l) in original.concepts.iter().zip(loaded.concepts.iter()) {
            assert_eq!(o.name, l.name);
            assert_eq!(o.vocabulary, l.vocabulary);
        }
    }
}
