use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EvidenceKind {
    Direct,
    Structural,
    Reinforcing,
    Boundary,
    Incidental,
}

impl EvidenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvidenceKind::Direct => "direct",
            EvidenceKind::Structural => "structural",
            EvidenceKind::Reinforcing => "reinforcing",
            EvidenceKind::Boundary => "boundary",
            EvidenceKind::Incidental => "incidental",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "direct" => Some(EvidenceKind::Direct),
            "structural" => Some(EvidenceKind::Structural),
            "reinforcing" => Some(EvidenceKind::Reinforcing),
            "boundary" => Some(EvidenceKind::Boundary),
            "incidental" => Some(EvidenceKind::Incidental),
            _ => None,
        }
    }

    pub fn retrieval_weight(&self) -> f64 {
        match self {
            EvidenceKind::Direct => 1.0,
            EvidenceKind::Boundary => 1.0,
            EvidenceKind::Reinforcing => 0.85,
            EvidenceKind::Structural => 0.7,
            EvidenceKind::Incidental => 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceProfile {
    pub kind: EvidenceKind,
    pub multiplicity: u32,
    pub cross_module: bool,
    pub cross_visibility: bool,
    pub motif_name: Option<String>,
}

impl EvidenceProfile {
    pub fn incidental() -> Self {
        Self {
            kind: EvidenceKind::Incidental,
            multiplicity: 1,
            cross_module: false,
            cross_visibility: false,
            motif_name: None,
        }
    }
}

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
    SharesConstant,
    ReferencesConstant,
    SharesType,
    SharesErrorType,
    SharesDataShape,
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
            EdgeKind::SharesConstant => "shares_constant",
            EdgeKind::ReferencesConstant => "references_constant",
            EdgeKind::SharesType => "shares_type",
            EdgeKind::SharesErrorType => "shares_error_type",
            EdgeKind::SharesDataShape => "shares_data_shape",
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
            "shares_constant" => Some(EdgeKind::SharesConstant),
            "references_constant" => Some(EdgeKind::ReferencesConstant),
            "shares_type" => Some(EdgeKind::SharesType),
            "shares_error_type" => Some(EdgeKind::SharesErrorType),
            "shares_data_shape" => Some(EdgeKind::SharesDataShape),
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
            EdgeKind::ReferencesConstant => 0.60,
            EdgeKind::SharesErrorType => 0.55,
            EdgeKind::Tests => 0.55,
            EdgeKind::Imports => 0.50,
            EdgeKind::ReExports => 0.45,
            EdgeKind::SharesType => 0.40,
            EdgeKind::References => 0.40,
            EdgeKind::SharesDataShape => 0.35,
            EdgeKind::SharesConstant => 0.30,
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
            EdgeKind::SharesConstant,
            EdgeKind::ReferencesConstant,
            EdgeKind::SharesType,
            EdgeKind::SharesErrorType,
            EdgeKind::SharesDataShape,
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

    #[test]
    fn test_evidence_kind_roundtrip() {
        for kind in [
            EvidenceKind::Direct,
            EvidenceKind::Structural,
            EvidenceKind::Reinforcing,
            EvidenceKind::Boundary,
            EvidenceKind::Incidental,
        ] {
            assert_eq!(EvidenceKind::from_str(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn test_evidence_retrieval_weights() {
        assert!(
            EvidenceKind::Direct.retrieval_weight() > EvidenceKind::Incidental.retrieval_weight()
        );
        assert!(
            EvidenceKind::Boundary.retrieval_weight() >= EvidenceKind::Structural.retrieval_weight()
        );
        assert!((EvidenceKind::Incidental.retrieval_weight() - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_incidental_profile() {
        let p = EvidenceProfile::incidental();
        assert_eq!(p.kind, EvidenceKind::Incidental);
        assert_eq!(p.multiplicity, 1);
        assert!(!p.cross_module);
    }
}
