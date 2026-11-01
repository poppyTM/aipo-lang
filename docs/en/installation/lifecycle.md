# Aipo — installation and lifecycle

**Status:** normative
**Scope:** installation paths, ownership, rollback strategy, uninstall safety
**Blocking question:** What happens after an interrupted update?

## Installation paths

- Single binary executable: `aipo` (placed in `~/.local/bin` or `/usr/local/bin`), built with
  `cargo build --release -p aipo-cli`.
- Standard library root: compiled into the executable — there is no external stdlib directory (an
  absent `~/.local/share/aipo/stdlib/` is normal).

## Ownership and permissions

- Binary belongs to user or root depending on install scope.
- No background daemon or privileged background service is installed.

## Rollback strategy

- Standalone binaries are installed via atomic move (`mv aipo.new aipo`).
- If an update is interrupted, the previous working binary remains intact because the atomic
  replacement has not occurred.

## Uninstall safety

- Removing the `aipo` binary cleanly uninstalls the toolchain. The CLI keeps no persistent
  state: no daemon, no cache directory, no system libraries touched (test harnesses use the OS
  temp directory only).
