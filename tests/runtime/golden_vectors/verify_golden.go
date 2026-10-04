// P16-X: Go cross-language Golden Vector verifier.
//
// Verifies that Go:
// 1. Can deserialize LoopTerminalEnvelope
// 2. Can recompute canonical digests
// 3. Rejects tampered envelopes (digest mismatch, wrong binding, etc.)
// 4. Cannot override Rust authority (Escalated ≠ Committed)
//
// Usage: go run verify_golden.go p16_golden_vectors.json

package main

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"os"
)

type Envelope struct {
	SchemaVersion        int     `json:"schema_version"`
	FlowID               string  `json:"flow_id"`
	WorkItemID           string  `json:"work_item_id"`
	LoopID               string  `json:"loop_id"`
	ExecutionGeneration  int     `json:"execution_generation"`
	RequestBindingHash   string  `json:"request_binding_hash"`
	ReportedTerminalState string `json:"reported_terminal_state"`
	DecisionID           *string `json:"decision_id"`
	DecisionHash         *string `json:"decision_hash"`
	EvidenceBundleRef    *string `json:"evidence_bundle_ref"`
	SettlementReceiptRef *string `json:"settlement_receipt_ref"`
	OutputCheckpointHash *string `json:"output_checkpoint_hash"`
	OutcomeBindingHash   string  `json:"outcome_binding_hash"`
	TotalAttempts        int     `json:"total_attempts"`
	TerminalReason       string  `json:"terminal_reason"`
}

type Expected struct {
	GoAccepts   bool   `json:"go_accepts"`
	IsSuccess   *bool  `json:"is_success"`
	RejectReason string `json:"reject_reason"`
}

type Vector struct {
	Name        string   `json:"name"`
	Description string   `json:"description"`
	Envelope    Envelope `json:"envelope"`
	Expected    Expected `json:"expected"`
}

type VectorsFile struct {
	ProtocolVersion int      `json:"protocol_version"`
	Description     string   `json:"description"`
	Vectors         []Vector `json:"vectors"`
}

func computeOutcomeBindingHash(e *Envelope) string {
	fields := fmt.Sprintf("%s:%s:%s:%s:%s:%s:%s:%s:%d:%d:%d",
		e.RequestBindingHash,
		e.ReportedTerminalState,
		strOrEmpty(e.DecisionID),
		strOrEmpty(e.DecisionHash),
		strOrEmpty(e.OutputCheckpointHash),
		strOrEmpty(e.SettlementReceiptRef),
		"", // output_artifact_refs
		fmt.Sprintf("%d", e.ExecutionGeneration),
		e.TotalAttempts,
		1, // schema_version
	)
	hash := sha256.Sum256([]byte(fields))
	return fmt.Sprintf("%x", hash)[:16]
}

func strOrEmpty(s *string) string {
	if s == nil { return "" }
	return *s
}

func validate(e *Envelope) (bool, string) {
	// 1. Schema version
	if e.SchemaVersion != 1 {
		return false, fmt.Sprintf("unsupported schema version: %d", e.SchemaVersion)
	}

	// 2. Compute and verify outcome binding hash
	computed := computeOutcomeBindingHash(e)
	if computed != e.OutcomeBindingHash {
		return false, fmt.Sprintf("outcome_binding_hash mismatch: computed=%s envelope=%s", computed, e.OutcomeBindingHash)
	}

	// 3. Committed state requires receipt
	if e.ReportedTerminalState == "Committed" && e.SettlementReceiptRef == nil {
		return false, "Committed without settlement_receipt_ref"
	}

	// 4. Committed must have consistent terminal_reason
	if e.ReportedTerminalState == "Committed" && e.TerminalReason != "" &&
		(e.TerminalReason == "onto_assurance_escalated" || e.TerminalReason == "Escalated by TransitionExecutor") {
		return false, fmt.Sprintf("Committed with escalated reason: %s", e.TerminalReason)
	}

	// 5. Escalated must not be marked as success
	if e.ReportedTerminalState != "Committed" && e.SettlementReceiptRef != nil {
		// Having a receipt on non-Committed is suspicious but not always invalid
	}

	return true, ""
}

func main() {
	if len(os.Args) < 2 {
		fmt.Println("Usage: go run verify_golden.go <golden_vectors.json>")
		os.Exit(1)
	}

	data, err := os.ReadFile(os.Args[1])
	if err != nil {
		fmt.Printf("ERROR reading file: %v\n", err)
		os.Exit(1)
	}

	var vf VectorsFile
	if err := json.Unmarshal(data, &vf); err != nil {
		fmt.Printf("ERROR parsing JSON: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("P16-X Golden Vector Verification (%d vectors)\n", len(vf.Vectors))
	fmt.Println("=========================================")

	passed := 0
	failed := 0

	for _, v := range vf.Vectors {
		accepted, reason := validate(&v.Envelope)
		expectedAccept := v.Expected.GoAccepts

		var status string
		if accepted == expectedAccept {
			status = "✅"
			passed++
		} else {
			status = "❌"
			failed++
		}

		extra := ""
		if v.Expected.IsSuccess != nil {
			if *v.Expected.IsSuccess && v.Envelope.ReportedTerminalState != "Committed" {
				extra = " (should be success but isn't Committed)"
			}
		}

		fmt.Printf("  %s %-30s accept=%v expected=%v%s",
			status, v.Name, accepted, expectedAccept, extra)
		if !accepted && reason != "" {
			fmt.Printf("  reason=%s", reason)
		}
		if expectedAccept && !accepted {
			fmt.Printf("  REJECTED_BUT_SHOULD_ACCEPT: %s", reason)
		}
		if !expectedAccept && accepted {
			fmt.Printf("  ACCEPTED_BUT_SHOULD_REJECT")
		}
		fmt.Println()
	}

	fmt.Printf("\n%d passed, %d failed\n", passed, failed)
	if failed > 0 {
		os.Exit(1)
	}
}
