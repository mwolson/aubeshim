# Agent Instructions

## Project overview

aubeshim is a small Rust command dispatcher. When installed as `bun`, `npm`,
`pnpm`, or `yarn` on PATH, it routes commands to `aube` where the command shape
is compatible and falls back to the real package manager otherwise.

## Conventions

- Single-binary Rust crate with most behavior in `src/lib.rs`.
- Keep dependencies minimal.
- Keep comments sparse and focused on non-obvious command compatibility.
- Prefer top-down control flow: caller first, then callees.
- When writing bash scripts: `#!/bin/bash`, 4 spaces for indentation, and
  fail-fast dependency checks.

## Dev loop

Run the normal Rust format, clippy, and test checks before handing off changes.

```sh
bun run hooks:check
```

The hook glob filters on changed files, so run the underlying checks directly
before a release:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Migration notes

When a project fails under `aube` or aubeshim after working with npm-style
hoisting, first check whether the project imports transitive dependencies that
are not declared in its own `package.json`. Prefer adding those direct
dependencies before reaching for a hoisted linker setting.

Use a hoisted linker profile as a compatibility fallback for projects whose
dependency trees are too messy to clean up quickly, especially Expo or React
Native apps. Keep `aube.allowBuilds` separate from linker decisions: it handles
lifecycle script approval, while hoisting handles module resolution shape.

Treat lockfile ownership as a separate compatibility question. A hoisted aube
install can make a local project work while still writing a `package-lock.json`
that npm rejects or that a later frozen aube install cannot reproduce. For
projects with npm coworkers, verify `npm ci` after any aube-authored
`package-lock.json`, or keep npm responsible for writing the lockfile. If a
project is ready to lean fully into aube, consider importing to a native
`aube-lock.yaml` instead of mixing aube lockfile writes with npm workflows.

## Aube bug reports

Use `https://github.com/mwolson/tmp-aube-issues` for minimal public repros of
aube issues found while testing aubeshim migrations. The local checkout lives
under this repo's ignored `tmp/min-repros` directory when present.

Before creating or updating a repro:

1. Retest with the current `aube --version` and record the exact version.
2. Check upstream aube issues and discussions for a matching report. If a broad
   existing discussion already covers the behavior, cite it instead of making a
   duplicate minimal repro unless the new case adds a clearly useful angle.
3. Read the relevant official docs to make sure the behavior is not intentional,
   especially:

   ```text
   https://aube.en.dev/package-manager/install.html
   https://aube.en.dev/package-manager/workspaces.html
   https://aube.en.dev/package-manager/lockfiles.html
   https://aube.en.dev/package-manager/node-modules.html
   ```

Repro conventions:

- Keep each case in its own directory with a short, descriptive name.
- Keep manifests as small as possible. Remove unrelated scripts, dev
  dependencies, tool config, app metadata, and private package names once the
  repro still triggers.
- Reduce dependencies aggressively. Keep removing package dependencies until the
  issue no longer reproduces, then keep the smallest dependency set that still
  makes the problem happen. If the lockfile shape matters, prune unrelated
  lockfile entries too while preserving the failing behavior.
- Preserve only the lockfile state needed to show the problem. If the repro
  depends on an intentionally stale lockfile, document that in the README or
  script output.
- For npm lockfile repros, make sure the pre-aube lockfile is a lockfile npm can
  actually produce or accept. Avoid hand-pruning `package-lock.json` into a
  state that makes aube fail but could not come from an npm workflow. Record the
  npm version and exact command arguments used to produce or validate the
  lockfile. If the current npm does not reproduce a real project's lock shape,
  try relevant older npm versions before giving up on minimization.
- Add or update the root README case list with the observed aube version for
  each issue, so later cases can be compared across releases.
- Include a `repro.sh` for each case. Use `#!/bin/bash`, `set -euo pipefail`,
  fail fast when required tools are missing, and use normal test semantics: exit
  zero when aube behaves correctly and non-zero when the issue is observed.
- Do not commit `node_modules` or generated runtime artifacts. Keep
  `node_modules/` ignored in the repro repo.
- Do not commit discussion drafts or report prose to the repro repo unless the
  user explicitly asks. Present draft upstream text in the chat as markdown
  blocks. Wrap the whole draft in four-backtick `markdown` fences so nested code
  blocks are preserved. Inside the draft, use list items for metadata such as
  repo and case, and use fenced code blocks for commands.

For lockfile reports, test the full lifecycle when relevant: the repair or
rewrite command, the resulting lockfile shape, a clean frozen aube install, and
the original package manager's response when compatibility is part of the claim.

## Releasing

1. Check for uncommitted changes and fetch tags:

   ```sh
   git status
   git fetch --tags
   ```

2. Run the full checks listed above. The pre-commit hook alone can skip Rust
   checks when the changed file set does not match its globs.

3. If the release changes the version, update `Cargo.toml` and `package.json`,
   run `cargo update -p aubeshim`, and commit the version bump separately with
   message `chore: bump version to <version>`.

4. Push `main`, create and push the release tag:

   ```sh
   git tag v<version>
   git push origin v<version>
   ```

5. Watch the tag-triggered `Publish` workflow. It builds release tarballs,
   creates a draft GitHub release, and publishes the crate:

   ```sh
   gh run list --limit 1
   gh run watch <run-id> --exit-status
   ```

6. If the workflow fails, fix `main`, delete the failed local and remote tag,
   retag the fixed commit, and push the tag again.

7. Review commits since the previous tag, update the draft release notes with
   user-visible changes first and maintenance details afterward, then publish
   the draft release.
