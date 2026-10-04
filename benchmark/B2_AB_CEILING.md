# B2-A/B Ceiling Control Results

## B2-A (kv_config, 3 C bugs)
- **Agent**: deepseek-chat
- **Finding**: Agent fixes all 3 bugs in 50-380s regardless of feedback mode
- **P16**: Always finds 3 issues (correct detection), Committed after fix
- **Budget**: 240/360/480s all insufficient to trigger multi-attempt
- **10s budget**: P16 runs on buggy template (3 findings, Continue), agent still fixes all

## B2-B (kvstore, 3 C bugs)  
- **Agent**: deepseek-chat
- **Finding**: Agent fixes all 3 bugs in 147-278s regardless of feedback mode
- **Budget**: 300/360/420s all insufficient, agent finishes before budget triggers
- **10s budget**: P16 runs on buggy template (3 findings, Continue), agent still fixes all

## Conclusion
Both tasks show ceiling effect with deepseek-chat. Structured/Generic feedback
mode comparison cannot measure incremental value when agent autonomously fixes
all bugs without P16 guidance. Tasks reserved as infrastructure validation
controls, not for Finding efficacy measurement.

## Infrastructure Validated
- F3: Assurance feedback delivery (Structured/Generic projection)
- F4: Attempt soft budget with graceful P16 finalization
- AgentEvidenceMode: audit/presentation separation
- 10s budget → buggy template → P16 → Continue → fix → Commit chain
