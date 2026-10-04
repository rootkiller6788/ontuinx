# P0 Baseline Summary — 2026-07-28

## Status: COMPLETE (Test execution BLOCKED by ASan overhead)

### Deliverables

| Item | Status | Detail |
|------|--------|--------|
| Git baseline | ✅ | commit `3daac44`, tag `upstream-baseline` |
| License | ✅ | MIT (Copyright 2025 DeusData) |
| Build | ✅ | `make -f Makefile.cbm cbm` passes |
| Test binary build | ✅ | 588 MB, ASan+UBSan debug |
| Test execution | ⚠️ | ASan too slow for interactive P0 |
| C fixture | ✅ | 3 files, 27 nodes, 40 edges, ~8.6s |
| Python fixture | ✅ | 1 file, 32 nodes, 64 edges, ~10.0s |
| TypeScript fixture | ✅ | 4 files, 27 nodes, 70 edges, ~10.0s |
| Java fixture | ✅ | 4 files, 54 nodes, 114 edges, ~8.2s |
| Golden data | ✅ | `baselines/p0/golden.json` |
| Dependency manifest | ✅ | `baselines/p0/manifest.json` |

### Exit Criteria Check

- [x] Original project builds stably
- [ ] Core tests all pass — BLOCKED (P1 to add non-ASan test target)
- [x] ≥4 priority languages have fixtures
- [x] Node/edge counts recorded for replay comparison
