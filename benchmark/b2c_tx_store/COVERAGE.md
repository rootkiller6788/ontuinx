# B2-C Defect Coverage Matrix

## Baseline: 2/7 pass (tests 1-2 ok, 3-7 fail)

| # | Defect | Invariant | Tests | Manifestation |
|---|--------|-----------|-------|--------------|
| B1 | Journal `find_key` on stale index after delete | State consistency after rollback | T3, T5 | ASan UAF (wrong value if no crash) |
| B2 | Delete compaction breaks iteration order | Index continuity after delete | T6 | Wrong iteration output (wrong key/value returned) |
| B3 | Nested tx ops write directly to main state | Transaction isolation | T3, T4 | Wrong state after inner tx |
| B4 | Rollback replays journal in reverse order | Recovery ordering | T3 | Wrong final value if same key modified 2x in tx |
| B5 | New keys inserted in tx survive rollback | Atomicity of abort | T4 | Key persists after rollback |
| B6 | Deleted keys not re-inserted on rollback | Atomicity of abort | T5 | Key missing after rollback |
| B7 | Checksum not recomputed after rollback | Metadata consistency | T7 | Checksum unchanged after restore |

## Test Output Leakage Assessment

| Test | Pass/Fail text | Source location? | Internal names? |
|------|---------------|-----------------|-----------------|
| T1 | "PASS" | No | No |
| T2 | "PASS" | No | No |
| T3 | "FAIL" (no detail) + ASan crash | **YES — ASan stack** | journal, index |
| T4 | "FAIL" + assertion text | No | No |
| T5 | "FAIL" (no detail) + ASan crash | **YES — ASan stack** | journal, index |
| T6 | "FAIL" (wrong value) | No | No |
| T7 | "FAIL" (checksum unchanged) | No | checksum |

**Leakage risk**: T3 and T5 emit ASan stacks with source locations.
**Mitigation**: Generic mode must strip ALL raw test/ASan output from agent context.
