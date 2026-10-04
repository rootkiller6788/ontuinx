package ontoflow

import (
	"context"
	"fmt"
)

// ── Authority Outcome Resolver ──
//
// G4: Go calls Rust's AuthorityProjectionPort (via gRPC) to verify Worker
// reports before accepting Committed. Go does NOT read OntoAssure DB tables.
//
// Correct path:
//   Worker returns LoopTerminalEnvelope → OutcomeReported
//   → Go calls AuthorityProjectionPort::resolve_loop_outcome()
//   → Rust returns VerifiedLoopOutcome
//   → If Committed → WorkItem Committed
//   → If NotCommitted/DecisionNotFound/BindingMismatch → Escalated/ProtocolFailed

// ── gRPC Client Interface ──

// AuthorityProjectionClient is the gRPC client for Rust's AuthorityProjectionPort.
// In production: generated from proto definition.
type AuthorityProjectionClient interface {
	ResolveLoopOutcome(ctx context.Context, req *ResolveLoopOutcomeRequest) (*VerifiedLoopOutcome, error)
}

// ResolveLoopOutcomeRequest matches Rust's ResolveLoopOutcomeRequest.
type ResolveLoopOutcomeRequest struct {
	FlowID               string `json:"flow_id"`
	WorkItemID           string `json:"work_item_id"`
	LoopID               string `json:"loop_id"`
	ExecutionGeneration  uint64 `json:"execution_generation"`
	TerminalEnvelopeHash string `json:"terminal_envelope_hash"`
}

// VerifiedLoopOutcome matches Rust's VerifiedLoopOutcome.
type VerifiedLoopOutcome struct {
	LoopID               string `json:"loop_id"`
	Outcome              string `json:"outcome"` // committed | not_committed | decision_not_found | binding_mismatch
	DecisionID           string `json:"decision_id"`
	DecisionHash         string `json:"decision_hash"`
	OutputCheckpointHash string `json:"output_checkpoint_hash"`
	SettlementReceiptRef string `json:"settlement_receipt_ref,omitempty"`
	AuthorityBindingHash string `json:"authority_binding_hash"`
}

// Authority outcome constants.
const (
	AuthorityOutcomeCommitted      = "committed"
	AuthorityOutcomeNotCommitted   = "not_committed"
	AuthorityOutcomeDecisionNotFound = "decision_not_found"
	AuthorityOutcomeBindingMismatch  = "binding_mismatch"
)

func (v *VerifiedLoopOutcome) IsCommitted() bool {
	return v.Outcome == AuthorityOutcomeCommitted
}

// ── Outcome Resolver ──

// OutcomeResolver verifies Worker reports via AuthorityProjectionPort.
// This is the ONLY path to mark a WorkItem as Committed.
type OutcomeResolver struct {
	authorityClient AuthorityProjectionClient
}

// NewOutcomeResolver creates a resolver with the given gRPC client.
func NewOutcomeResolver(client AuthorityProjectionClient) *OutcomeResolver {
	return &OutcomeResolver{authorityClient: client}
}

// Resolve verifies a Worker's LoopTerminalEnvelope against the OntoAssure authority.
//
// Steps:
// 1. Call AuthorityProjectionPort::resolve_loop_outcome()
// 2. Verify response: loop_id match, decision_id present, outcome=committed
// 3. Return the verified outcome or an error
func (r *OutcomeResolver) Resolve(
	ctx context.Context,
	req *LoopInvocationRequest,
	env *LoopTerminalEnvelope,
) (*VerifiedLoopOutcome, error) {
	// Only resolve if the Worker reported Committed
	if env.ReportedTerminalState != TerminalStateCommitted {
		return nil, fmt.Errorf("cannot resolve non-committed state: %s", env.ReportedTerminalState)
	}

	// Must have a decision_id to verify
	if env.DecisionID == nil {
		return nil, fmt.Errorf("committed envelope has no decision_id — cannot verify")
	}

	resolveReq := &ResolveLoopOutcomeRequest{
		FlowID:               req.FlowID,
		WorkItemID:           req.WorkItemID,
		LoopID:               req.LoopID,
		ExecutionGeneration:  req.ExecutionGeneration,
		TerminalEnvelopeHash: env.OutcomeBindingHash,
	}

	verified, err := r.authorityClient.ResolveLoopOutcome(ctx, resolveReq)
	if err != nil {
		return nil, fmt.Errorf("authority resolution failed for %s: %w", req.LoopID, err)
	}

	// Verify the response matches what we asked for
	if verified.LoopID != req.LoopID {
		return nil, fmt.Errorf("authority returned wrong loop_id: asked=%s got=%s",
			req.LoopID, verified.LoopID)
	}

	return verified, nil
}

// AcceptOutcome applies the verified outcome to the WorkItem.
//
// OutcomeReported → AuthorityVerifying → Committed (if verified)
// OutcomeReported → AuthorityVerifying → Escalated (if rejected)
func (r *OutcomeResolver) AcceptOutcome(
	ctx context.Context,
	req *LoopInvocationRequest,
	env *LoopTerminalEnvelope,
	wi *WorkItemRuntimeState,
) error {
	// Step 1: Transition to AuthorityVerifying
	if wi.Phase != WIPhaseOutcomeReported {
		return &TransitionError{
			wi.WorkItemID, string(wi.Phase), string(WIPhaseAuthorityVerifying),
			"can only verify OutcomeReported WorkItems",
		}
	}

	if err := TransitionWorkItem(wi, WIPhaseAuthorityVerifying); err != nil {
		return err
	}

	// Step 2: Resolve via AuthorityProjectionPort
	verified, err := r.Resolve(ctx, req, env)
	if err != nil {
		wi.LastError = err.Error()
		_ = TransitionWorkItem(wi, WIPhaseEscalated)
		return err
	}

	// Step 3: Apply the verified outcome
	switch verified.Outcome {
	case AuthorityOutcomeCommitted:
		wi.VerifiedOutcome = AuthorityOutcomeCommitted
		wi.DecisionID = verified.DecisionID
		return TransitionWorkItem(wi, WIPhaseCommitted)

	case AuthorityOutcomeNotCommitted:
		wi.VerifiedOutcome = AuthorityOutcomeNotCommitted
		wi.LastError = "authority rejected: not committed"
		return TransitionWorkItem(wi, WIPhaseEscalated)

	case AuthorityOutcomeDecisionNotFound:
		wi.VerifiedOutcome = AuthorityOutcomeDecisionNotFound
		wi.LastError = "no decision record found"
		return TransitionWorkItem(wi, WIPhaseEscalated)

	case AuthorityOutcomeBindingMismatch:
		wi.VerifiedOutcome = AuthorityOutcomeBindingMismatch
		wi.LastError = fmt.Sprintf("binding mismatch: authority hash=%s", verified.AuthorityBindingHash)
		return TransitionWorkItem(wi, WIPhaseEscalated)

	default:
		return fmt.Errorf("unknown authority outcome: %s", verified.Outcome)
	}
}

// ── In-Memory Authority Client for G4 testing ──

// InMemoryAuthorityClient is a mock AuthorityProjectionClient for testing.
// Stores decision records and verifies them on Resolve.
type InMemoryAuthorityClient struct {
	decisions map[string]*InMemoryDecision // keyed by loop_id
}

type InMemoryDecision struct {
	LoopID               string
	DecisionID           string
	DecisionHash         string
	OutputCheckpointHash string
	SettlementReceiptRef string
	IsCommitted          bool
}

func NewInMemoryAuthorityClient() *InMemoryAuthorityClient {
	return &InMemoryAuthorityClient{
		decisions: make(map[string]*InMemoryDecision),
	}
}

// RecordDecision stores a decision record (simulating OntoAssure persistence).
func (c *InMemoryAuthorityClient) RecordDecision(
	loopID, decisionID, decisionHash, checkpointHash, receiptRef string,
) {
	c.decisions[loopID] = &InMemoryDecision{
		LoopID:               loopID,
		DecisionID:           decisionID,
		DecisionHash:         decisionHash,
		OutputCheckpointHash: checkpointHash,
		SettlementReceiptRef: receiptRef,
		IsCommitted:          true,
	}
}

// RecordRejectedDecision stores a rejected decision.
func (c *InMemoryAuthorityClient) RecordRejectedDecision(loopID, decisionID, reason string) {
	c.decisions[loopID] = &InMemoryDecision{
		LoopID:      loopID,
		DecisionID:  decisionID,
		IsCommitted: false,
	}
}

func (c *InMemoryAuthorityClient) ResolveLoopOutcome(
	ctx context.Context, req *ResolveLoopOutcomeRequest,
) (*VerifiedLoopOutcome, error) {
	dec, exists := c.decisions[req.LoopID]
	if !exists {
		return &VerifiedLoopOutcome{
			LoopID:  req.LoopID,
			Outcome: AuthorityOutcomeDecisionNotFound,
		}, nil
	}

	if dec.LoopID != req.LoopID {
		return &VerifiedLoopOutcome{
			LoopID:  req.LoopID,
			Outcome: AuthorityOutcomeBindingMismatch,
		}, nil
	}

	if !dec.IsCommitted {
		return &VerifiedLoopOutcome{
			LoopID:      req.LoopID,
			Outcome:     AuthorityOutcomeNotCommitted,
			DecisionID:  dec.DecisionID,
		}, nil
	}

	bindingHash := fmt.Sprintf("authority-%s-%s", dec.LoopID, dec.DecisionID)

	return &VerifiedLoopOutcome{
		LoopID:               req.LoopID,
		Outcome:              AuthorityOutcomeCommitted,
		DecisionID:           dec.DecisionID,
		DecisionHash:         dec.DecisionHash,
		OutputCheckpointHash: dec.OutputCheckpointHash,
		SettlementReceiptRef: dec.SettlementReceiptRef,
		AuthorityBindingHash: bindingHash,
	}, nil
}
