package ontoflow

import (
	"testing"
)

// ── G9 Acceptance Tests ──
//
// G9-1: Three proposers produce distinct artifacts
// G9-2: Barrier waits for all proposers
// G9-3: Critics read Round 1 artifacts, produce Round 2 critiques
// G9-4: Quorum 2/3 detected
// G9-5: No quorum on even split
// G9-6: Full 3-round deliberation (Proposer→Critic→Synthesizer)

// ── G9-1: Three proposers produce distinct artifacts ──

func TestG9_1_ThreeProposersDistinctArtifacts(t *testing.T) {
	spec := &DiscussionSpec{
		DiscussionID: "d1",
		Rounds: []RoundSpec{
			{RoundNumber: 1, Role: RoleProposer, Participants: []string{"loop-A", "loop-B", "loop-C"}},
		},
		QuorumRatio: 0.66,
	}

	dm := NewDiscussionManager(spec)

	_ = dm.SubmitArtifact(&DiscussionArtifact{
		ArtifactID: "art-A", ProducerLoopID: "loop-A", Round: 1, Role: "proposer", Position: "use Rust", Confidence: 0.9,
	})
	_ = dm.SubmitArtifact(&DiscussionArtifact{
		ArtifactID: "art-B", ProducerLoopID: "loop-B", Round: 1, Role: "proposer", Position: "use Rust", Confidence: 0.85,
	})
	_ = dm.SubmitArtifact(&DiscussionArtifact{
		ArtifactID: "art-C", ProducerLoopID: "loop-C", Round: 1, Role: "proposer", Position: "use Python", Confidence: 0.6,
	})

	if !dm.IsRoundComplete(1) {
		t.Fatal("round 1 should be complete after 3 submissions")
	}

	artifacts := dm.GetRoundArtifacts(1)
	if len(artifacts) != 3 {
		t.Errorf("expected 3 artifacts, got %d", len(artifacts))
	}

	// Distinct artifact IDs
	ids := make(map[string]bool)
	for _, a := range artifacts {
		ids[a.ArtifactID] = true
	}
	if len(ids) != 3 {
		t.Error("all 3 artifacts should have distinct IDs")
	}
}

// ── G9-2: Barrier waits for all proposers ──

func TestG9_2_BarrierWaitsForAll(t *testing.T) {
	barrier := NewRoundBarrier(1, []string{"A", "B", "C", "D"})

	if barrier.IsComplete() {
		t.Error("barrier should not be complete initially")
	}
	if barrier.Remaining() != 4 {
		t.Errorf("expected 4 remaining, got %d", barrier.Remaining())
	}

	barrier.MarkComplete("A")
	barrier.MarkComplete("B")
	if barrier.Remaining() != 2 {
		t.Errorf("expected 2 remaining after A+B, got %d", barrier.Remaining())
	}
	if barrier.IsComplete() {
		t.Error("barrier should not be complete yet")
	}

	barrier.MarkComplete("C")
	barrier.MarkComplete("D")
	if !barrier.IsComplete() {
		t.Error("barrier should be complete")
	}
	if barrier.Remaining() != 0 {
		t.Errorf("expected 0 remaining, got %d", barrier.Remaining())
	}
}

// ── G9-3: Critics read Round 1, produce Round 2 ──

func TestG9_3_CriticsConsumeRound1(t *testing.T) {
	spec := &DiscussionSpec{
		DiscussionID: "d3",
		Rounds: []RoundSpec{
			{RoundNumber: 1, Role: RoleProposer, Participants: []string{"l-A", "l-B"}},
			{RoundNumber: 2, Role: RoleCritic, Participants: []string{"l-X", "l-Y"}},
		},
		QuorumRatio: 0.66,
	}

	dm := NewDiscussionManager(spec)

	// Round 1
	_ = dm.SubmitArtifact(&DiscussionArtifact{ArtifactID: "a1", ProducerLoopID: "l-A", Round: 1, Role: "proposer", Position: "Rust"})
	_ = dm.SubmitArtifact(&DiscussionArtifact{ArtifactID: "a2", ProducerLoopID: "l-B", Round: 1, Role: "proposer", Position: "Rust"})
	if !dm.IsRoundComplete(1) {
		t.Fatal("round 1 should be complete")
	}

	// Round 2: Critics consume Round 1 artifacts
	r1Artifacts := dm.GetRoundArtifacts(1)
	if len(r1Artifacts) != 2 {
		t.Fatalf("expected 2 round-1 artifacts for critics, got %d", len(r1Artifacts))
	}

	_ = dm.SubmitArtifact(&DiscussionArtifact{ArtifactID: "c1", ProducerLoopID: "l-X", Round: 2, Role: "critic", Position: "agree"})
	_ = dm.SubmitArtifact(&DiscussionArtifact{ArtifactID: "c2", ProducerLoopID: "l-Y", Round: 2, Role: "critic", Position: "agree"})
	if !dm.IsRoundComplete(2) {
		t.Error("round 2 should be complete")
	}
}

// ── G9-4: Quorum 2/3 detected ──

func TestG9_4_QuorumTwoOfThree(t *testing.T) {
	artifacts := []*DiscussionArtifact{
		{Position: "agree"},
		{Position: "agree"},
		{Position: "disagree"},
	}
	qc := NewQuorumCheck(artifacts, 0.66)
	if !qc.HasQuorum() {
		t.Error("2/3 agree should satisfy 0.66 threshold")
	}

	pos, count := qc.MajorityPosition()
	if pos != "agree" || count != 2 {
		t.Errorf("majority should be 'agree' (2), got '%s' (%d)", pos, count)
	}
}

// ── G9-5: No quorum on even split ──

func TestG9_5_NoQuorumEvenSplit(t *testing.T) {
	artifacts := []*DiscussionArtifact{
		{Position: "agree"},
		{Position: "disagree"},
	}
	qc := NewQuorumCheck(artifacts, 0.66)
	if qc.HasQuorum() {
		t.Error("1/2 should NOT satisfy 0.66 threshold")
	}
}

// ── G9-6: Full 3-round deliberation ──

func TestG9_6_FullThreeRoundDeliberation(t *testing.T) {
	spec := &DiscussionSpec{
		DiscussionID: "full-delib",
		Rounds: []RoundSpec{
			{RoundNumber: 1, Role: RoleProposer, Participants: []string{"p-A", "p-B", "p-C"}},
			{RoundNumber: 2, Role: RoleCritic, Participants: []string{"c-X", "c-Y"}},
			{RoundNumber: 3, Role: RoleSynthesizer, Participants: []string{"s-final"}},
		},
		QuorumRatio: 0.66,
	}

	dm := NewDiscussionManager(spec)

	// Round 1: 3 proposers
	_ = dm.SubmitArtifact(&DiscussionArtifact{
		ArtifactID: "a1", ProducerLoopID: "p-A", Round: 1, Role: "proposer", Position: "approach-X", Confidence: 0.9,
	})
	_ = dm.SubmitArtifact(&DiscussionArtifact{
		ArtifactID: "a2", ProducerLoopID: "p-B", Round: 1, Role: "proposer", Position: "approach-X", Confidence: 0.8,
	})
	_ = dm.SubmitArtifact(&DiscussionArtifact{
		ArtifactID: "a3", ProducerLoopID: "p-C", Round: 1, Role: "proposer", Position: "approach-Y", Confidence: 0.5,
	})
	if !dm.IsRoundComplete(1) {
		t.Fatal("round 1 incomplete")
	}

	// Quorum on Round 1: 2/3 agree on "approach-X"
	qc1, _ := dm.CheckQuorum(1)
	if !qc1.HasQuorum() {
		t.Error("round 1: 2/3 should satisfy quorum")
	}

	// Round 2: 2 critics
	_ = dm.SubmitArtifact(&DiscussionArtifact{
		ArtifactID: "c1", ProducerLoopID: "c-X", Round: 2, Role: "critic", Position: "support-X", Confidence: 0.9,
	})
	_ = dm.SubmitArtifact(&DiscussionArtifact{
		ArtifactID: "c2", ProducerLoopID: "c-Y", Round: 2, Role: "critic", Position: "support-X", Confidence: 0.85,
	})
	if !dm.IsRoundComplete(2) {
		t.Fatal("round 2 incomplete")
	}

	// Quorum on Round 2: 2/2 agree
	qc2, _ := dm.CheckQuorum(2)
	if !qc2.HasQuorum() {
		t.Error("round 2: 2/2 should satisfy quorum")
	}

	// Round 3: Synthesizer
	_ = dm.SubmitArtifact(&DiscussionArtifact{
		ArtifactID: "s1", ProducerLoopID: "s-final", Round: 3, Role: "synthesizer", Position: "approach-X with refinement", Confidence: 0.95,
	})
	if !dm.IsRoundComplete(3) {
		t.Fatal("round 3 incomplete")
	}

	// All rounds complete — verify artifact counts
	for r := uint32(1); r <= 3; r++ {
		arts := dm.GetRoundArtifacts(r)
		expectedCount := map[uint32]int{1: 3, 2: 2, 3: 1}
		if len(arts) != expectedCount[r] {
			t.Errorf("round %d: expected %d artifacts, got %d", r, expectedCount[r], len(arts))
		}
	}
}
