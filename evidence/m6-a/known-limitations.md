# M6-A Known Limitations

## Scope

Single-machine, local versioned filesystem, Staged Effect, process crash recovery.

## Not Covered

- Node-level failures (machine power loss, disk corruption)
- Network filesystems (NFS, distributed FS)
- Object storage (S3, GCS)
- OntoFlow-based recovery scheduling
- Hardlink safety (link count > 1)
- Special files (FIFO, socket, block/char device)
- `current` symlink external tampering
- Irreversible or Compensatable effects
- Cross-machine concurrent publish
- Multi-tenant workspace isolation

## Path Safety

Basic path safety complete (absolute, `..`, symlink rejected). Production hardening:
- Hardlink detection
- FIFO/socket/device rejection
- `current` symlink integrity monitoring

## Known Test Gaps

- `unknown_current_hash_freezes`: code path exists, no dedicated automated test
- Transaction persistence ordering (PUBLISHING before publish, PUBLISHED after receipt): code path exists, inline assertions, no standalone fault-injection test
- Receipt persist failure during `execute_publish()`: behavior documented, tested in reconciler path but not in coordinator path

## Deliberately Excluded

- M6-B Transactional Database Effect
- M6-C Compensatable Effect
- M6-D Irreversible Effect
- OntoFlow orchestration
- Distributed state machines
- Web console
