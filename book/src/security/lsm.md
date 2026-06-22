# Linux Security Modules (LSM)

The LSM framework provides pluggable security hooks throughout the kernel.

## Architecture

LSM hooks are placed at critical kernel entry points:
- File operations (open, read, write, execute)
- Process operations (fork, exec, kill)
- Socket operations (bind, connect, listen)
- IPC operations (message queues, semaphores)

## Implementations

- **Unix DAC**: Default discretionary access control
- **MAC**: Mandatory access control framework (extensible)

For full details, see `docs/security/lsm.md`.
