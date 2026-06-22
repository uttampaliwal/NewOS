# Desktop Environment

Turnix includes a Wayland-like compositor and desktop shell for graphical output.

## Compositor

The compositor (`userland/compositor/`) initializes via the DRM/KMS subsystem, setting up
framebuffers and input event handling. It runs an event loop that receives input events,
manages window state, and performs page-flipping to present rendered frames. The compositor
communicates with clients through a custom Wayland-inspired protocol.

## Display Manager

The display manager (`userland/display-manager/`) coordinates graphical session startup.
It launches the compositor, manages display hardware state, and handles session transitions
between users.

## Desktop Shell

The desktop shell (`userland/desktop-shell/`) provides the top-level window management and
taskbar. It receives window lists from the compositor and renders a minimal desktop
interface.

## Graphics Pipeline

All graphical output goes through the DRM/KMS driver in the kernel. The compositor maps
framebuffer regions via the `mmap_framebuffer` syscall and uses `drm_page_flip` for
vsync-synchronized presentation.
