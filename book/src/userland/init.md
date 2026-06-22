# Init Daemon

The init daemon (`userland/init/`) runs as PID 1 and is the first userland process
launched by the kernel.

## Service Manifests

Services are declared in an embedded TOML manifest (`SERVICE_TOML`). Each entry specifies
a name, binary path, and dependency ordering via an `after` list. The init daemon parses
these manifests at boot using `parse_services()` from `userland/init/src/lib.rs`.

## Startup Order

Services are started in topological order based on their `after` dependencies. The init
process `fork`s and `exec`s each service binary in sequence, waiting for each to signal
readiness before starting dependents.

## Zombie Reaping

Init adopts all orphaned child processes. It runs a `wait` loop to collect exit statuses
and reap zombie processes, preventing PID table exhaustion.

## Shutdown

On receiving a shutdown signal (`read_shutdown_signal`), init sends `SIGTERM` followed by
`SIGKILL` to all running services, waits for them to exit, and invokes the kernel
`shutdown` syscall.
