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
