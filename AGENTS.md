# Agent Instructions

## Project overview

aubeshim is a small Rust command dispatcher. When installed as `npm`, `pnpm`,
or `yarn` on PATH, it routes commands to `aube` where the command shape is
compatible and falls back to the real package manager otherwise.

## Conventions

- Single-binary Rust crate with most behavior in `src/lib.rs`.
- Keep dependencies minimal.
- Keep comments sparse and focused on non-obvious command compatibility.
- Prefer top-down control flow: caller first, then callees.
- When writing bash scripts: `#!/bin/bash`, 4 spaces for indentation, and
  fail-fast dependency checks.

## Dev loop

Run the normal Rust format, clippy, and test checks before handing off changes.
