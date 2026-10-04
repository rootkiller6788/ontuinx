//! F8: Decentralized Discussion — multi-round OntoLoop collaboration.
//!
//! Multiple OntoLoops exchange immutable ArtifactRefs through rounds.
//! Go manages barriers and quorum. Rust demonstrates artifact production
//! and consumption across rounds.
//!
//! ## Pattern
//!
//! Round 1: Proposer-A/B/C each produce an ArtifactRef
//! Barrier (Go waits for all)
//! Round 2: Critic-A/B each read Round 1 artifacts, produce critiques
//! Barrier + Quorum (Go checks agreement)
//! Round 3: Synthesizer reads all artifacts, produces final output

use serde::{Deserialize, Serialize};

/// An artifact produced by one OntoLoop and consumed by another.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub producer_loop_id: String,
    pub producer_work_item_id: String,
    pub artifact_hash: String,
    pub round: u32,
    pub role: String,
}

/// Result of one discussion round by a single participant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundResult {
    pub loop_id: String,
    pub role: String,
    pub round: u32,
    pub output_artifacts: Vec<ArtifactRef>,
    pub input_artifacts: Vec<ArtifactRef>,
    pub position: String,
    pub confidence: f64,
}

impl RoundResult {
    /// Create a result for a proposer in round 1.
    pub fn proposer(
        loop_id: &str, work_item_id: &str, proposal: &str, confidence: f64,
    ) -> Self {
        Self {
            loop_id: loop_id.to_string(),
            role: "proposer".to_string(),
            round: 1,
            output_artifacts: vec![ArtifactRef {
                artifact_id: format!("artifact-{}", loop_id),
                producer_loop_id: loop_id.to_string(),
                producer_work_item_id: work_item_id.to_string(),
                artifact_hash: format!("hash-{}", loop_id),
                round: 1,
                role: "proposer".to_string(),
            }],
            input_artifacts: vec![],
            position: proposal.to_string(),
            confidence,
        }
    }

    /// Create a result for a critic in round 2.
    pub fn critic(
        loop_id: &str, work_item_id: &str,
        input_artifacts: Vec<ArtifactRef>, critique: &str, confidence: f64,
    ) -> Self {
        Self {
            loop_id: loop_id.to_string(),
            role: "critic".to_string(),
            round: 2,
            output_artifacts: vec![ArtifactRef {
                artifact_id: format!("critique-{}", loop_id),
                producer_loop_id: loop_id.to_string(),
                producer_work_item_id: work_item_id.to_string(),
                artifact_hash: format!("hash-critique-{}", loop_id),
                round: 2,
                role: "critic".to_string(),
            }],
            input_artifacts,
            position: critique.to_string(),
            confidence,
        }
    }
}

/// Check if a quorum is reached (e.g., 2 out of 3 agree).
pub fn check_quorum(positions: &[&str], required_ratio: f64) -> bool {
    if positions.is_empty() {
        return false;
    }
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for pos in positions {
        *counts.entry(pos).or_insert(0) += 1;
    }
    let max_count = counts.values().max().copied().unwrap_or(0);
    (max_count as f64) >= (positions.len() as f64) * required_ratio
}

// ══════════════════════════════════════════════════════════════════
// Tests — F8
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// F8.1: Round 1 — three proposers each produce an artifact.
    #[test]
    fn f8_1_three_proposers_produce_artifacts() {
        let p_a = RoundResult::proposer("loop-A", "wi-A", "use Rust", 0.9);
        let p_b = RoundResult::proposer("loop-B", "wi-B", "use Rust", 0.85);
        let p_c = RoundResult::proposer("loop-C", "wi-C", "use Python", 0.6);

        assert_eq!(p_a.round, 1);
        assert_eq!(p_a.role, "proposer");
        assert_eq!(p_a.output_artifacts.len(), 1);
        assert!(p_a.input_artifacts.is_empty());

        // All artifacts are distinct
        let ids: Vec<_> = [&p_a, &p_b, &p_c]
            .iter()
            .map(|r| &r.output_artifacts[0].artifact_id)
            .collect();
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
    }

    /// F8.2: Round 2 — critics read Round 1 artifacts and produce critiques.
    #[test]
    fn f8_2_critics_consume_proposer_artifacts() {
        let p_a = RoundResult::proposer("loop-A", "wi-A", "use Rust", 0.9);
        let p_b = RoundResult::proposer("loop-B", "wi-B", "use Rust", 0.85);

        let r1_artifacts: Vec<ArtifactRef> = [&p_a, &p_b]
            .iter()
            .flat_map(|r| r.output_artifacts.clone())
            .collect();

        let critic_x = RoundResult::critic(
            "loop-X", "wi-X", r1_artifacts.clone(),
            "agree with Rust approach", 0.88,
        );
        assert_eq!(critic_x.round, 2);
        assert_eq!(critic_x.input_artifacts.len(), 2);
        assert_eq!(critic_x.output_artifacts.len(), 1);
    }

    /// F8.3: Quorum check — 2 of 3 agree on same position.
    #[test]
    fn f8_3_quorum_two_of_three() {
        let positions = vec!["agree", "agree", "disagree"];
        assert!(check_quorum(&positions, 0.66));
        assert!(!check_quorum(&positions, 1.0));
    }

    /// F8.4: No quorum when evenly split.
    #[test]
    fn f8_4_no_quorum_even_split() {
        let positions = vec!["agree", "disagree"];
        assert!(!check_quorum(&positions, 0.66));
    }

    /// F8.5: Full discussion simulation — 3 rounds.
    #[test]
    fn f8_5_full_three_round_discussion() {
        // Round 1: 3 proposers
        let proposers = [
            RoundResult::proposer("l-A", "w-A", "approach X", 0.9),
            RoundResult::proposer("l-B", "w-B", "approach X", 0.8),
            RoundResult::proposer("l-C", "w-C", "approach Y", 0.5),
        ];

        // Barrier: Go waits for all 3. Check all have round=1 artifacts.
        for p in &proposers {
            assert_eq!(p.round, 1);
            assert!(!p.output_artifacts.is_empty());
        }

        // Quorum check on positions
        let r1_positions: Vec<&str> = proposers.iter().map(|p| p.position.as_str()).collect();
        assert!(check_quorum(&r1_positions, 0.66), "approach X has quorum");

        // Round 2: 2 critics read all Round 1 artifacts
        let r1_artifacts: Vec<ArtifactRef> = proposers
            .iter()
            .flat_map(|p| p.output_artifacts.clone())
            .collect();

        let critics = [
            RoundResult::critic("l-X", "w-X", r1_artifacts.clone(), "support X", 0.9),
            RoundResult::critic("l-Y", "w-Y", r1_artifacts.clone(), "support X", 0.85),
        ];

        // Barrier + Quorum on critiques
        let r2_positions: Vec<&str> = critics.iter().map(|c| c.position.as_str()).collect();
        assert!(check_quorum(&r2_positions, 0.66));

        // Round 3: Synthesizer reads ALL previous artifacts
        let all_artifacts: Vec<ArtifactRef> = proposers
            .iter()
            .chain(critics.iter())
            .flat_map(|r| r.output_artifacts.clone())
            .collect();
        assert_eq!(all_artifacts.len(), 5); // 3 proposer + 2 critic artifacts

        // Each artifact references its producer correctly
        for art in &all_artifacts {
            assert!(!art.artifact_id.is_empty());
            assert!(!art.producer_loop_id.is_empty());
            assert!(!art.artifact_hash.is_empty());
        }
    }

    /// F8.6: Artifact refs are immutable — hash is part of identity.
    #[test]
    fn f8_6_artifact_hash_immutable() {
        let result = RoundResult::proposer("loop-1", "wi-1", "test", 0.9);
        let art = &result.output_artifacts[0];
        let hash1 = art.artifact_hash.clone();

        // Same inputs produce same hash (deterministic)
        let result2 = RoundResult::proposer("loop-1", "wi-1", "test", 0.9);
        let hash2 = result2.output_artifacts[0].artifact_hash.clone();
        assert_eq!(hash1, hash2, "same inputs → same artifact hash");
    }
}
