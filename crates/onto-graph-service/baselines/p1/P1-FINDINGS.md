# P1 Findings — 2026-07-28

## Result: PARTIAL — UI and INSTALLER disabled; DAEMON deferred to P2.5

### What worked

| Flag | Status | Notes |
|------|--------|-------|
| OFG_ENABLE_UI=0 | PASS | Golden: 27 nodes, 40 edges |
| OFG_ENABLE_INSTALLER=0 | PASS | Golden: 27 nodes, 40 edges |

### DAEMON=0: 196 compile errors, deferred to P2.5

main.c has 100+ cbm_daemon_* references woven through initialization.
Confirmed: plan risk assessment was correct.

### P1 Exit Criteria

- [x] OFG_ENABLE_UI=0 compiles, Golden matches
- [x] OFG_ENABLE_INSTALLER=0 compiles, Golden matches
- [x] ofg_config.h with 6 flags
- [ ] DAEMON=0 → P2.5
- [ ] TELEMETRY=0 → P2.5
