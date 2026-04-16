#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Motif {
    Connector,
    Orchestrator,
    Guard,
    Transform,
    Sink,
    Source,
    Hub,
    Leaf,
}

impl Motif {
    pub fn as_str(&self) -> &'static str {
        match self {
            Motif::Connector => "connector",
            Motif::Orchestrator => "orchestrator",
            Motif::Guard => "guard",
            Motif::Transform => "transform",
            Motif::Sink => "sink",
            Motif::Source => "source",
            Motif::Hub => "hub",
            Motif::Leaf => "leaf",
        }
    }

    pub fn fts_terms(&self) -> &'static str {
        match self {
            Motif::Connector => "connects joins links bridges routes dispatch",
            Motif::Orchestrator => "orchestrates coordinates manages organizes controls",
            Motif::Guard => "guards checks validates protects filters gates",
            Motif::Transform => "transforms converts maps adapts translates",
            Motif::Sink => "consumes receives handles processes stores persists",
            Motif::Source => "produces generates emits creates provides fetches",
            Motif::Hub => "central hub nexus core dispatch router fan",
            Motif::Leaf => "endpoint terminal leaf final concrete",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MotifEvidence {
    pub has_call_in: bool,
    pub has_call_out: bool,
    pub call_in_count: usize,
    pub call_out_count: usize,
    pub has_contains_out: bool,
    pub contains_count: usize,
    pub has_implements_out: bool,
    pub has_extends_out: bool,
    pub has_imports_in: bool,
    pub imports_in_count: usize,
    pub has_tests_in: bool,
    pub is_container: bool,
}

pub fn detect_motifs(evidence: &MotifEvidence) -> Vec<Motif> {
    let mut motifs = Vec::new();

    if evidence.has_call_in && evidence.has_call_out {
        if evidence.call_in_count >= 3 && evidence.call_out_count >= 3 {
            motifs.push(Motif::Hub);
        }
        motifs.push(Motif::Connector);
    }

    if evidence.call_out_count >= 3 && evidence.has_call_in {
        motifs.push(Motif::Orchestrator);
    }

    if evidence.has_call_in && !evidence.has_call_out && evidence.call_in_count >= 2 {
        motifs.push(Motif::Sink);
    }

    if evidence.has_call_out && !evidence.has_call_in && evidence.call_out_count >= 2 {
        motifs.push(Motif::Source);
    }

    if !evidence.has_call_in && !evidence.has_call_out && !evidence.is_container {
        motifs.push(Motif::Leaf);
    }

    if evidence.has_implements_out || evidence.has_extends_out {
        motifs.push(Motif::Transform);
    }

    if evidence.has_tests_in && evidence.has_call_in {
        motifs.push(Motif::Guard);
    }

    motifs.truncate(4);
    motifs
}

pub fn motifs_to_hints(motifs: &[Motif]) -> String {
    let terms: Vec<&str> = motifs.iter().map(|m| m.fts_terms()).collect();
    let names: Vec<&str> = motifs.iter().map(|m| m.as_str()).collect();
    let mut parts = Vec::new();
    parts.push(terms.join(" "));
    parts.push(format!("motif: {}", names.join(", ")));
    parts.join(". ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_motif() {
        let ev = MotifEvidence {
            has_call_in: true,
            has_call_out: true,
            call_in_count: 2,
            call_out_count: 2,
            has_contains_out: false,
            contains_count: 0,
            has_implements_out: false,
            has_extends_out: false,
            has_imports_in: false,
            imports_in_count: 0,
            has_tests_in: false,
            is_container: false,
        };
        let motifs = detect_motifs(&ev);
        assert!(motifs.contains(&Motif::Connector));
    }

    #[test]
    fn test_hub_motif() {
        let ev = MotifEvidence {
            has_call_in: true,
            has_call_out: true,
            call_in_count: 5,
            call_out_count: 4,
            has_contains_out: false,
            contains_count: 0,
            has_implements_out: false,
            has_extends_out: false,
            has_imports_in: false,
            imports_in_count: 0,
            has_tests_in: false,
            is_container: false,
        };
        let motifs = detect_motifs(&ev);
        assert!(motifs.contains(&Motif::Hub));
        assert!(motifs.contains(&Motif::Connector));
    }

    #[test]
    fn test_sink_motif() {
        let ev = MotifEvidence {
            has_call_in: true,
            has_call_out: false,
            call_in_count: 3,
            call_out_count: 0,
            has_contains_out: false,
            contains_count: 0,
            has_implements_out: false,
            has_extends_out: false,
            has_imports_in: false,
            imports_in_count: 0,
            has_tests_in: false,
            is_container: false,
        };
        let motifs = detect_motifs(&ev);
        assert!(motifs.contains(&Motif::Sink));
    }

    #[test]
    fn test_motif_cap() {
        let ev = MotifEvidence {
            has_call_in: true,
            has_call_out: true,
            call_in_count: 5,
            call_out_count: 5,
            has_contains_out: true,
            contains_count: 3,
            has_implements_out: true,
            has_extends_out: true,
            has_imports_in: true,
            imports_in_count: 3,
            has_tests_in: true,
            is_container: true,
        };
        let motifs = detect_motifs(&ev);
        assert!(motifs.len() <= 4);
    }

    #[test]
    fn test_motifs_to_hints() {
        let hints = motifs_to_hints(&[Motif::Connector, Motif::Hub]);
        assert!(hints.contains("connects"));
        assert!(hints.contains("hub"));
        assert!(hints.contains("motif:"));
    }
}
