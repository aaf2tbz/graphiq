use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Imports,
    Calls,
    Contains,
    Extends,
    Implements,
    Overrides,
    References,
    Tests,
    ReExports,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Imports => "imports",
            EdgeKind::Calls => "calls",
            EdgeKind::Contains => "contains",
            EdgeKind::Extends => "extends",
            EdgeKind::Implements => "implements",
            EdgeKind::Overrides => "overrides",
            EdgeKind::References => "references",
            EdgeKind::Tests => "tests",
            EdgeKind::ReExports => "re_exports",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "imports" => Some(EdgeKind::Imports),
            "calls" => Some(EdgeKind::Calls),
            "contains" => Some(EdgeKind::Contains),
            "extends" => Some(EdgeKind::Extends),
            "implements" => Some(EdgeKind::Implements),
            "overrides" => Some(EdgeKind::Overrides),
            "references" => Some(EdgeKind::References),
            "tests" => Some(EdgeKind::Tests),
            "re_exports" => Some(EdgeKind::ReExports),
            _ => None,
        }
    }

    pub fn path_weight(&self) -> f64 {
        match self {
            EdgeKind::Calls => 1.00,
            EdgeKind::Contains => 0.90,
            EdgeKind::Implements => 0.80,
            EdgeKind::Extends => 0.80,
            EdgeKind::Overrides => 0.75,
            EdgeKind::Tests => 0.55,
            EdgeKind::Imports => 0.50,
            EdgeKind::References => 0.40,
            EdgeKind::ReExports => 0.45,
        }
    }

    pub fn is_dependency(&self) -> bool {
        matches!(
            self,
            EdgeKind::Calls | EdgeKind::Imports | EdgeKind::References | EdgeKind::Contains
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: i64,
    pub source_id: i64,
    pub target_id: i64,
    pub kind: EdgeKind,
    pub weight: f64,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlastDirection {
    Forward,
    Backward,
    Both,
}

impl BlastDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            BlastDirection::Forward => "forward",
            BlastDirection::Backward => "backward",
            BlastDirection::Both => "both",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlastEntry {
    pub symbol_id: i64,
    pub symbol_name: String,
    pub symbol_kind: String,
    pub file_path: String,
    pub distance: usize,
    pub path: Vec<EdgeKind>,
}

#[derive(Debug, Clone)]
pub struct BlastRadius {
    pub origin_name: String,
    pub origin_kind: String,
    pub origin_file: String,
    pub forward: Vec<BlastEntry>,
    pub backward: Vec<BlastEntry>,
    pub max_depth: usize,
}

impl BlastRadius {
    pub fn forward_count(&self) -> usize {
        self.forward.len()
    }

    pub fn backward_count(&self) -> usize {
        self.backward.len()
    }

    pub fn total_count(&self) -> usize {
        self.forward.len() + self.backward.len()
    }
}

#[derive(Debug, Clone)]
pub struct FileEdge {
    pub id: i64,
    pub source_file_id: i64,
    pub target_file_id: i64,
    pub kind: String,
    pub metadata: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_kind_roundtrip() {
        for kind in [
            EdgeKind::Imports,
            EdgeKind::Calls,
            EdgeKind::Contains,
            EdgeKind::Extends,
            EdgeKind::Implements,
            EdgeKind::Overrides,
            EdgeKind::References,
            EdgeKind::Tests,
            EdgeKind::ReExports,
        ] {
            assert_eq!(EdgeKind::from_str(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn test_path_weights() {
        assert!(EdgeKind::Calls.path_weight() > EdgeKind::References.path_weight());
        assert!(EdgeKind::Contains.path_weight() > EdgeKind::Imports.path_weight());
        assert!((EdgeKind::Calls.path_weight() - 1.0).abs() < f64::EPSILON);
        assert!((EdgeKind::References.path_weight() - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn test_is_dependency() {
        assert!(EdgeKind::Calls.is_dependency());
        assert!(EdgeKind::Imports.is_dependency());
        assert!(!EdgeKind::Tests.is_dependency());
        assert!(!EdgeKind::Implements.is_dependency());
    }
}
