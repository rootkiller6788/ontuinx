package ontoflow

import (
	"context"
	"fmt"
)

// ── Deliberation ──
//
// G9: Multi-round decentralized discussion between OntoLoops.
// Go manages rounds, barriers, and quorum. OntoLoops exchange immutable
// ArtifactRefs — they never share mutable Checkpoints.
//
// Pattern:
//   Round 1: Proposer-A/B/C → produce proposals
//   Barrier (wait for all)
//   Round 2: Critic-A/B → read Round 1 artifacts → produce critiques
//   Barrier + Quorum
//   Round 3: Synthesizer → read all → produce final

// ── Discussion Types ──

// DiscussionRole defines the role of a participant in a round.
type DiscussionRole string

const (
	RoleProposer    DiscussionRole = "proposer"
	RoleCritic      DiscussionRole = "critic"
	RoleSynthesizer DiscussionRole = "synthesizer"
	RoleObserver    DiscussionRole = "observer"
)

// DiscussionSpec defines a complete deliberation flow.
type DiscussionSpec struct {
	DiscussionID string
	Rounds       []RoundSpec
	QuorumRatio  float64 // e.g., 0.66 for 2/3 majority
}

// RoundSpec defines one round of deliberation.
type RoundSpec struct {
	RoundNumber uint32
	Role        DiscussionRole
	Participants []string // WorkItem/loop IDs
	ReadFromRound *uint32 // which round's artifacts to read (nil = no input)
}

// ── Artifact Exchange ──

// DiscussionArtifact is an immutable artifact exchanged between rounds.
type DiscussionArtifact struct {
	ArtifactID       string `json:"artifact_id"`
	ProducerLoopID   string `json:"producer_loop_id"`
	ProducerWorkItemID string `json:"producer_work_item_id"`
	ArtifactHash     string `json:"artifact_hash"`
	Round            uint32 `json:"round"`
	Role             string `json:"role"`
	Position         string `json:"position"`    // e.g., "use Rust", "use Python"
	Confidence       float64 `json:"confidence"` // 0.0–1.0
}

// RoundResult collects all artifacts from one round.
type RoundResult struct {
	Round      uint32
	Role       DiscussionRole
	Artifacts  []*DiscussionArtifact
	Complete   bool // all participants finished
}

// ── Barrier ──

// RoundBarrier waits for all participants in a round to complete.
type RoundBarrier struct {
	round        uint32
	expected     int
	completed    int
	participants map[string]bool
}

// NewRoundBarrier creates a barrier for a round.
func NewRoundBarrier(round uint32, participants []string) *RoundBarrier {
	pm := make(map[string]bool)
	for _, p := range participants {
		pm[p] = false
	}
	return &RoundBarrier{
		round:        round,
		expected:     len(participants),
		participants: pm,
	}
}

// MarkComplete marks a participant as done.
func (b *RoundBarrier) MarkComplete(participantID string) {
	if _, exists := b.participants[participantID]; exists {
		if !b.participants[participantID] {
			b.participants[participantID] = true
			b.completed++
		}
	}
}

// IsComplete returns true when all participants have completed.
func (b *RoundBarrier) IsComplete() bool {
	return b.completed >= b.expected
}

// Remaining returns the number of participants not yet complete.
func (b *RoundBarrier) Remaining() int {
	return b.expected - b.completed
}

// ── Quorum ──

// QuorumCheck verifies that a threshold of participants agree on a position.
type QuorumCheck struct {
	Positions []string
	Threshold float64 // e.g., 0.66 = 2 out of 3
}

// NewQuorumCheck creates a quorum check from artifact positions.
func NewQuorumCheck(artifacts []*DiscussionArtifact, threshold float64) *QuorumCheck {
	positions := make([]string, len(artifacts))
	for i, a := range artifacts {
		positions[i] = a.Position
	}
	return &QuorumCheck{Positions: positions, Threshold: threshold}
}

// HasQuorum returns true if any position meets the threshold.
func (q *QuorumCheck) HasQuorum() bool {
	if len(q.Positions) == 0 {
		return false
	}

	counts := make(map[string]int)
	for _, pos := range q.Positions {
		counts[pos]++
	}

	maxCount := 0
	for _, c := range counts {
		if c > maxCount {
			maxCount = c
		}
	}

	return float64(maxCount) >= float64(len(q.Positions))*q.Threshold
}

// MajorityPosition returns the position with the most support, and its count.
func (q *QuorumCheck) MajorityPosition() (string, int) {
	counts := make(map[string]int)
	for _, pos := range q.Positions {
		counts[pos]++
	}

	maxPos := ""
	maxCount := 0
	for pos, count := range counts {
		if count > maxCount {
			maxPos = pos
			maxCount = count
		}
	}
	return maxPos, maxCount
}

// ── Discussion Manager ──

// DiscussionManager orchestrates multi-round deliberation.
type DiscussionManager struct {
	spec    *DiscussionSpec
	rounds  map[uint32]*RoundResult  // round → artifacts
	barriers map[uint32]*RoundBarrier
}

// NewDiscussionManager creates a discussion manager.
func NewDiscussionManager(spec *DiscussionSpec) *DiscussionManager {
	dm := &DiscussionManager{
		spec:     spec,
		rounds:   make(map[uint32]*RoundResult),
		barriers: make(map[uint32]*RoundBarrier),
	}
	for _, round := range spec.Rounds {
		dm.barriers[round.RoundNumber] = NewRoundBarrier(round.RoundNumber, round.Participants)
		dm.rounds[round.RoundNumber] = &RoundResult{
			Round:      round.RoundNumber,
			Role:       round.Role,
			Artifacts:  []*DiscussionArtifact{},
			Complete:   false,
		}
	}
	return dm
}

// SubmitArtifact submits an artifact from a participant.
func (dm *DiscussionManager) SubmitArtifact(artifact *DiscussionArtifact) error {
	round, exists := dm.rounds[artifact.Round]
	if !exists {
		return fmt.Errorf("unknown round %d", artifact.Round)
	}

	round.Artifacts = append(round.Artifacts, artifact)

	barrier := dm.barriers[artifact.Round]
	barrier.MarkComplete(artifact.ProducerLoopID)

	if barrier.IsComplete() {
		round.Complete = true
	}

	return nil
}

// GetRoundArtifacts returns all artifacts from a specific round.
func (dm *DiscussionManager) GetRoundArtifacts(round uint32) []*DiscussionArtifact {
	if r, exists := dm.rounds[round]; exists {
		return r.Artifacts
	}
	return nil
}

// IsRoundComplete returns true if the round barrier is satisfied.
func (dm *DiscussionManager) IsRoundComplete(round uint32) bool {
	if r, exists := dm.rounds[round]; exists {
		return r.Complete
	}
	return false
}

// CheckQuorum checks if a round's artifacts meet the quorum threshold.
func (dm *DiscussionManager) CheckQuorum(round uint32) (*QuorumCheck, error) {
	artifacts := dm.GetRoundArtifacts(round)
	if len(artifacts) == 0 {
		return nil, fmt.Errorf("no artifacts in round %d", round)
	}

	qc := NewQuorumCheck(artifacts, dm.spec.QuorumRatio)
	return qc, nil
}

// RunDiscussion executes the full deliberation flow.
// Returns all round results in order.
func (dm *DiscussionManager) RunDiscussion(
	ctx context.Context,
	artifactSubmitter func(ctx context.Context, round uint32, role DiscussionRole, participantID string) (*DiscussionArtifact, error),
) ([]*RoundResult, error) {
	var results []*RoundResult

	for _, round := range dm.spec.Rounds {
		for _, participantID := range round.Participants {
			artifact, err := artifactSubmitter(ctx, round.RoundNumber, round.Role, participantID)
			if err != nil {
				return nil, fmt.Errorf("round %d participant %s: %w", round.RoundNumber, participantID, err)
			}
			if err := dm.SubmitArtifact(artifact); err != nil {
				return nil, err
			}
		}

		if !dm.IsRoundComplete(round.RoundNumber) {
			return nil, fmt.Errorf("round %d did not complete", round.RoundNumber)
		}

		results = append(results, dm.rounds[round.RoundNumber])
	}

	return results, nil
}
