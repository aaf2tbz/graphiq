use std::collections::HashMap;

use crate::db::GraphDb;
use crate::self_model::{self, ConceptKind, RepoSelfModel};
use crate::subsystems::{self, SubsystemIndex};

const GENERIC_NAMES: &[&str] = &[
    "get", "set", "push", "pop", "remove", "add", "delete", "update", "create",
    "new", "init", "start", "stop", "run", "execute", "process", "handle",
    "parse", "format", "to_string", "from", "into", "default", "clone", "eq",
    "drop", "send", "sync", "copy", "main", "test", "iter", "next", "len",
    "is_empty", "as_ref", "as_mut", "deref", "index", "call",
];

fn is_generic_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    GENERIC_NAMES.iter().any(|g| lower == *g)
}

fn humanize_subsystem_name(raw: &str) -> String {
    let parts: Vec<&str> = raw.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return raw.to_string();
    }

    let pkg = humanize_package_name(parts[0]);
    if parts.len() == 1 {
        return pkg;
    }

    let file = humanize_file_name(parts[1]);
    if file == pkg {
        pkg
    } else {
        format!("{} / {}", pkg, file)
    }
}

fn humanize_package_name(name: &str) -> String {
    let stripped = name
        .trim_start_matches("signet-")
        .trim_start_matches("connector-")
        .trim_start_matches("graphiq-");
    let words: Vec<&str> = stripped.split(&['-', '_'][..]).collect();
    words
        .iter()
        .map(|w| {
            let chars: Vec<char> = w.chars().collect();
            if chars.is_empty() {
                return String::new();
            }
            if w.chars().all(|c| c.is_uppercase()) && w.len() <= 4 {
                w.to_string()
            } else {
                let mut s = chars[0].to_uppercase().to_string();
                if chars.len() > 1 {
                    s.push_str(&w[1..].to_lowercase());
                }
                s
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn humanize_file_name(name: &str) -> String {
    let stem = name.trim_end_matches(".rs").trim_end_matches(".ts").trim_end_matches(".js");
    let words: Vec<&str> = stem.split(&['-', '_'][..]).collect();
    words
        .iter()
        .map(|w| {
            let chars: Vec<char> = w.chars().collect();
            if chars.is_empty() {
                return String::new();
            }
            if w.chars().all(|c| c.is_uppercase()) && w.len() <= 4 {
                w.to_string()
            } else {
                let mut s = chars[0].to_uppercase().to_string();
                if chars.len() > 1 {
                    s.push_str(&w[1..].to_lowercase());
                }
                s
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

struct BriefingData {
    files: i64,
    symbols: i64,
    edges: i64,
    languages: HashMap<String, i64>,
    kinds: HashMap<String, i64>,
    edge_types: HashMap<String, i64>,
    subsystems: SubsystemIndex,
    model: RepoSelfModel,
}

fn gather_data(db: &GraphDb) -> Result<BriefingData, String> {
    let conn = db.conn();
    let stats = db.stats().map_err(|e| e.to_string())?;

    let mut languages = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT language, COUNT(*) FROM files GROUP BY language")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        for (lang, count) in rows {
            languages.insert(lang, count);
        }
    }

    let mut kinds = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT kind, COUNT(*) FROM symbols GROUP BY kind")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        for (kind, count) in rows {
            kinds.insert(kind, count);
        }
    }

    let mut edge_types = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT kind, COUNT(*) FROM edges GROUP BY kind")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        for (kind, count) in rows {
            edge_types.insert(kind, count);
        }
    }

    let subsystems = subsystems::detect_subsystems(db)?;
    let model = self_model::build_self_model(db)?;

    Ok(BriefingData {
        files: stats.files,
        symbols: stats.symbols,
        edges: stats.edges,
        languages,
        kinds,
        edge_types,
        subsystems,
        model,
    })
}

pub fn generate_briefing(db: &GraphDb) -> Result<String, String> {
    let data = gather_data(db)?;
    let mut out = String::new();

    out.push_str("# Codebase Briefing\n\n");

    out.push_str(&format!(
        "**{} files, {} symbols, {} edges**\n\n",
        data.files, data.symbols, data.edges
    ));

    write_languages(&mut out, &data);
    write_symbol_types(&mut out, &data);
    write_edge_types(&mut out, &data);
    write_architecture(&mut out, &data, None);
    write_public_api(&mut out, &data, None);
    write_concepts(&mut out, &data);
    write_hub_symbols(&mut out, &data, None);

    Ok(out)
}

pub fn generate_briefing_compact(db: &GraphDb) -> Result<String, String> {
    let data = gather_data(db)?;
    let mut out = String::new();

    out.push_str("# Codebase Briefing (compact)\n\n");

    out.push_str(&format!(
        "**{} files, {} symbols, {} edges**\n\n",
        data.files, data.symbols, data.edges
    ));

    write_languages(&mut out, &data);
    write_architecture(&mut out, &data, Some(15));
    write_public_api(&mut out, &data, Some(15));

    Ok(out)
}

fn write_languages(out: &mut String, data: &BriefingData) {
    let mut langs: Vec<_> = data.languages.iter().collect();
    langs.sort_by(|a, b| b.1.cmp(a.1));
    out.push_str("## Languages\n\n");
    for (lang, count) in &langs {
        out.push_str(&format!("- **{}**: {} files\n", lang, count));
    }
    out.push('\n');
}

fn write_symbol_types(out: &mut String, data: &BriefingData) {
    let mut kinds: Vec<_> = data.kinds.iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(a.1));
    out.push_str("## Symbol Types\n\n");
    for (kind, count) in &kinds {
        out.push_str(&format!("- **{}**: {}\n", kind, count));
    }
    out.push('\n');
}

fn write_edge_types(out: &mut String, data: &BriefingData) {
    let mut types: Vec<_> = data.edge_types.iter().collect();
    types.sort_by(|a, b| b.1.cmp(a.1));
    out.push_str("## Relationship Types\n\n");
    for (kind, count) in &types {
        out.push_str(&format!("- **{}**: {}\n", kind, count));
    }
    out.push('\n');
}

fn write_architecture(out: &mut String, data: &BriefingData, limit: Option<usize>) {
    let mut subs: Vec<_> = data.subsystems.subsystems.iter().collect();
    subs.sort_by(|a, b| b.symbol_ids.len().cmp(&a.symbol_ids.len()));

    if let Some(n) = limit {
        subs.truncate(n);
    }

    subs.retain(|s| s.symbol_ids.len() >= 20);

    out.push_str("## Architecture\n\n");
    for sub in &subs {
        let name = humanize_subsystem_name(&sub.name);
        let boundary_pct = if sub.internal_edge_count + sub.boundary_edge_count > 0 {
            (sub.boundary_edge_count as f64 / (sub.internal_edge_count + sub.boundary_edge_count) as f64 * 100.0) as i64
        } else {
            0
        };
        out.push_str(&format!(
            "### {} ({} symbols, {} boundary)\n\n",
            name, sub.symbol_ids.len(), boundary_pct
        ));

        let mut names: Vec<&str> = sub.symbol_names.iter().map(|s| s.as_str()).take(30).collect();
        names.retain(|n| !is_generic_name(n));
        names.truncate(20);
        if !names.is_empty() {
            out.push_str(&format!("Key symbols: {}\n\n", names.join(", ")));
        }
    }
}

fn write_public_api(out: &mut String, data: &BriefingData, limit: Option<usize>) {
    let public: Vec<_> = data.model.concepts.iter()
        .filter(|c| matches!(c.kind, ConceptKind::PublicApi))
        .collect();

    if public.is_empty() {
        return;
    }

    let mut api_entries: Vec<(String, String)> = Vec::new();
    for concept in &public {
        for name in &concept.key_symbol_names {
            if !is_generic_name(name) {
                api_entries.push((name.clone(), concept.name.clone()));
            }
        }
    }

    if let Some(n) = limit {
        api_entries.truncate(n);
    }

    out.push_str("## Public API\n\n");
    for (name, source) in &api_entries {
        out.push_str(&format!("- **{}** (from {})\n", name, source));
    }
    out.push('\n');
}

fn write_concepts(out: &mut String, data: &BriefingData) {
    let concerns: Vec<_> = data.model.concepts.iter()
        .filter(|c| matches!(c.kind, ConceptKind::Concern | ConceptKind::ErrorSurface))
        .collect();

    if concerns.is_empty() {
        return;
    }

    out.push_str("## Cross-Cutting Concerns\n\n");
    for concept in &concerns {
        let kind_label = match concept.kind {
            ConceptKind::Concern => "concern",
            ConceptKind::ErrorSurface => "error surface",
            _ => unreachable!(),
        };
        out.push_str(&format!(
            "### {} ({})\n\n{}\n\nKey: {}\n\n",
            concept.name,
            kind_label,
            concept.description,
            concept.key_symbol_names.iter().take(8).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
}

fn write_hub_symbols(out: &mut String, data: &BriefingData, limit: Option<usize>) {
    let mut symbol_degree: HashMap<i64, usize> = HashMap::new();
    for sub in &data.subsystems.subsystems {
        for &sid in &sub.symbol_ids {
            let entry = symbol_degree.entry(sid).or_default();
            *entry += sub.internal_edge_count + sub.boundary_edge_count;
        }
    }

    let mut hubs: Vec<(i64, usize, String)> = Vec::new();
    for sub in &data.subsystems.subsystems {
        for (&sid, name) in sub.symbol_ids.iter().zip(sub.symbol_names.iter()) {
            if !is_generic_name(name) {
                if let Some(&deg) = symbol_degree.get(&sid) {
                    if deg > 0 {
                        hubs.push((sid, deg, name.clone()));
                    }
                }
            }
        }
    }

    hubs.sort_by(|a, b| b.1.cmp(&a.1));
    if let Some(n) = limit {
        hubs.truncate(n);
    } else {
        hubs.truncate(20);
    }

    if hubs.is_empty() {
        return;
    }

    out.push_str("## Hub Symbols (most connected)\n\n");
    for (_, _, name) in &hubs {
        out.push_str(&format!("- {}\n", name));
    }
    out.push('\n');
}
