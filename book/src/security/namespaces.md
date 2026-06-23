> For detailed reference documentation, see [docs/security/](../../../docs/security/)

# Namespaces

Namespaces provide process-level isolation of system resources.

## Supported Namespaces

- **PID**: Process ID isolation with local init remapping
- **Mount**: Filesystem mount point isolation
- **Network**: Network stack isolation (interfaces, routes, sockets)
- **User**: UID/GID translation between namespaces

## Usage

Namespaces are created via `clone()` with appropriate flags or unshared
via `unshare()`. Each namespace provides a separate view of the resource
it isolates.

For full details, see `docs/security/namespaces.md`.
