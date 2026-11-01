# Public GitHub package fetch security

**Status:** implemented for the opt-in `github-http` feature
**Scope:** `aipo package fetch-github`, mixed-root `package lock --fetch-github`, their explicit
recursive GitHub graphs, opt-in environment-backed authentication, cache verification and
conservative pruning
**Network policy:** public GET by default; an explicitly named environment variable may supply a
validated bearer credential; no automatic downloads
**Offline policy:** `--package-cache`, `package cache verify` and `package cache prune` read
verified cache entries only and never fall back to network

## Trust boundary

The GitHub response is untrusted input. The trusted boundary is the local `aipo-package` parser,
cache verifier, graph resolver and output writer. A response becomes a package only after its
manifest, entry path, content digest, source revision, coordinate, version and dependency edges
pass validation. A fetched graph is written only after the complete root-plus-transitive graph
resolves.

The cache directory is caller-owned. The command never chooses a global cache and never writes
outside the explicit `--cache` and `--out` directories. A token is read only when the caller
explicitly passes `--github-token-env <name>`; its value is never persisted or logged.

## Required controls

- The HTTP feature is disabled in the default CLI build; cache verification remains available.
- The source accepts only `owner/repository`, an exact lowercase 40-hex commit and a safe
  relative subpath.
- Requests use HTTPS, a fixed GitHub raw host, a bounded timeout and a 4 MiB response limit.
- Requests omit `Authorization` by default; when `--github-token-env` is supplied, only a
  validated bearer credential is sent to the fixed GitHub raw host. Redirects remain disabled.
- Missing, empty or control-character-bearing environment credentials fail with `AIPO_PKG_FETCH`
  before any request; token values are not included in diagnostics.
- Cache paths use a SHA-256 key instead of repository text; symlinks are rejected.
- Cache writes use a private temporary directory and atomic rename.
- Cached manifest and entry bytes are parsed and checked against the stored SHA-256 digest.
- Offline cache reads do not create missing roots and reject symlinked or non-regular files.
- Corrupt cache entries fail closed; the command does not silently repair or overwrite them.
- The output directory must be empty. Existing files are never overwritten.
- Every recursively fetched source is cached and digest-verified independently.
- Mixed-root locking uses the same cache and transport policy; only `package lock --fetch-github`
  and `package fetch-github` can trigger network access.
- `aipo package cache verify` audits all existing entries, reports independent failures, and
  never creates, repairs, deletes or fetches data.
- `aipo package cache prune` is dry-run by default, requires an explicit lockfile and `--apply`
  for deletion, retains referenced sources, and deletes nothing when verification reports any
  invalid entry.
- A remote graph is bounded to 256 packages per fetch operation.
- Remote artifacts may declare only pinned GitHub dependencies; local `path` dependencies inside a
  remote artifact fail closed because no caller-owned filesystem root is available.
- Package lifecycle code is never executed.

## Operational ownership

The user chooses the cache root and owns its retention and permissions. The cache has no
automatic or time-based eviction, and pruning never repairs data. Run
`aipo package cache verify <cache-dir>` before `aipo package cache prune <cache-dir> --lock
<lockfile> --apply`; use a dedicated directory when the host workspace is shared.

## Non-goals

This slice does not support OAuth, token acquisition, credential storage, keychains, automatic
credential discovery, private-registry authentication, automatic or time-based cache eviction,
repair, signatures, provenance attestations, registry publication or automatic dependency
resolution by `run`, `check`, `build`, `disasm`, plain `package lock` or `package audit`.
Authentication is limited to an explicitly named environment variable and the explicit
`fetch-github` and `package lock --fetch-github` operations. Source commands may read the same
cache only when the user supplies `--package-cache`; a missing entry never causes a fetch. Cache
verification and pruning never trigger a fetch or read a token.
