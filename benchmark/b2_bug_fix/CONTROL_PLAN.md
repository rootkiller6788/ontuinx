# B2-A Control Group Design

## Groups

| Group | Description | Mechanism |
|-------|-------------|-----------|
| **A** | Single attempt, no retry | Grade buggy template directly |
| **B** | Empty feedback retry | Tell agent "check again" without specific findings |
| **C** | P16 feedback retry | P16 Findings → Agent fix → P16 verify |

## Results (Calibration)

| Group | Tests | ASan | Attempts |
|-------|-------|------|----------|
| A (baseline) | 0/7 | FAIL | 1 |
| C (P16) | 7/7 | PASS | 2 |

## Group B Implementation

Requires P16 modification:
1. On first Continue, replace Findings with empty message
2. Let agent retry without specific guidance
3. Compare with Group C to isolate Finding value

## Expected comparison

```
C - A = P16 total benefit
C - B = P16 Finding incremental value  
B - A = benefit of extra retry alone
```
