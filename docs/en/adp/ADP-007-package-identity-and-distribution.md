# ADP-007 — Package identity and distribution

**Status:** accepted
**Date:** 2026-09-24
**Related:** Fechamento Arquitetural §13, §14; `docs/crates/crate-contracts.md`; `docs/canon/Aipo — Waves, Vertical Slices, Gauntlet Loops`
**Authority:** subordinate to the canon; this ADP fixes the first implementation without creating a platform service of its own.

## A. Package coordinate

A package coordinate has exactly two dot-separated segments:

```text
namespace.package
```

Example: `acme.http`. The coordinate is the canonical identity used in the manifest, lockfile, SourceMap and diagnostics. Segments use the ASCII lowercase grammar `[a-z][a-z0-9_]*`; this decision does not yet accept submodules (`acme.http.client`).

The origin is explicit in the resolution chain: `acme.http` is not confused with the built-in module `http` or with a local package of the same name.

## B. Qualified imports

The import surface uses the full coordinate:

```text
import acme.http
import acme.http as h
```

The alias remains local to the file that declares it. The original coordinate remains available for provenance, lockfile and diagnostics. Built-in modules remain simple (`math`, `task`, `io`, `time`).

The first slice supports only the entry module of each package. The submodule form is left for a later decision, so as not to blur the boundary between coordinate and internal path.

## C. Distribution

Aipo owns the package protocol; GitHub is only the initial distribution backend. The source model separates origin, revision and artifact:

- `path` for local development;
- GitHub source with an `owner/repository` slug, an exact 40-hex commit SHA and a relative subpath;
- registry source as a future adapter, without coupling the resolver to GitHub.

The manifest accepts `path` and pinned GitHub dependencies. The canonical form for GitHub is `type = "github"`, with `repository`, `revision`, `subpath` and an exact SemVer version. The lockfile records the coordinate, exact version, source, revision, checksum, targets and relevant capabilities. Branches, tags, URLs, credentials and mutable revisions are rejected. A `GitHubFetcher` and an offline in-memory store allow provenance to be tested without a network; the verified-cache API and the read-only `package cache verify` are available in the default build, while the `http` feature adds public GET, timeout, response limit, blocked redirects and an atomic local cache. Authentication is opt-in via `--github-token-env <name>` in the fetch/lock forms; the value comes from an explicitly named environment variable, is validated as a bearer token, and is never persisted, logged or included in diagnostics.

`aipo package fetch-github` and the explicit form `aipo package lock <dir> --fetch-github --cache <dir>` are the only operations that enable GitHub fetching. `fetch-github` recursively walks the pinned GitHub dependencies and writes the complete lockfile into the output snapshot. The mixed-lock form keeps local inputs in the project, fetches only the pinned GitHub edges and writes a single lockfile. Both optionally accept `--github-token-env <name>` to use a bearer credential supplied by the environment; without that flag, requests remain public and carry no `Authorization`. Local dependencies remain available in the local resolver; a path dependency declared inside a remote artifact is rejected because there is no trusted root filesystem in which to interpret it. `run`, `check`, `build` and `disasm` accept `--package-cache` to consume the mixed graph without a network; without the flag, these commands are local. `aipo package cache verify <dir>` audits all existing entries without creating, repairing, removing or fetching data. `aipo package cache prune <dir> --lock <lockfile>` is a dry run by default; only `--apply` removes verified entries that the lockfile does not reference, and any verification error prevents any removal. `package lock` without the explicit form and `package audit` remain local. OAuth, token discovery/persistence, automatic downloads, lifecycle and registry publication remain outside this slice. There are no arbitrary install scripts or lifecycle execution in V1.

## D. Capabilities

Capabilities declared by packages are an upper bound. The project policy and the host policy can reduce that set; they never silently widen it. The transitive union is auditable and must fit within the limit declared by the root.

`aipo-host` remains the source of the grammar and the narrowing rules. `aipo-stdlib` publishes the API surface; the effective service is installed and verified in the host, as already happens with `clock` and Poppy. The first new services are `env.read` and the host-only slice `fs.read_text`/`fs.roots`: each provider lives in the VM's `HostContext`, installation does not grant capabilities, and the roots/paths policy belongs to the provider.

## E. Consequences

- `import` no longer accepts only an identifier and now carries a qualified path.
- Parser, AST/HIR, resolver, linker, formatter, source maps and diagnostics must preserve the full coordinate.
- The package resolver must be kept separate from the runtime `ModuleGraph`; the latter remains the initialization graph of already-resolved modules.
- The local slice does not implement artifact signing or a web platform. The `http` feature allows explicit GitHub fetch, with optional authentication that is strictly opt-in via an environment variable name; signing, distributed cache and registries remain behind their own fixtures and security decisions.

## F. Next slices

1. `aipo-package`: manifest, coordinate, lock and deterministic `path` resolver — done.
2. Capabilities and dependency auditing, with `env.read` as the first per-VM proof — done.
3. Capability-aware stdlib modules, starting with `fs.read_text`/`fs.roots` and the async-first contract — done in the host-only slice.
4. Pinned GitHub source with provenance, digest and an offline seam — done in slice P04-G04.
5. Public HTTP adapter, local cache and explicit `fetch-github` command — done in slice P04-G05; authentication remains opt-in and outside the manifest.
6. Recursive GitHub dependencies in manifest and lockfile, fetched only through the explicit command — done in slice P04-G06.
7. Offline consumption through a verified cache for GitHub snapshots, with no network — done in slice P04-G07.
8. Local root with local and pinned GitHub dependencies, explicit mixed lock and offline replay — done in slice P04-G08.
9. Read-only verification of the entire cache, with no creation, repair, removal or network — done in slice P04-G09.
10. Explicit prune by lockfile, dry run by default and deletion only with `--apply` — done in slice P04-G10.
11. Opt-in GitHub authentication via an explicit environment variable, with no persistence or logs — done in slice P04-G11.
12. A registry of our own only if the scale, privacy or signing triggers are reached.
