# aubeshim

`aubeshim` installs PATH shims that let existing `bun`, `npm`, `pnpm`, and
`yarn` commands use `aube` when the command shape is compatible.

The goal is to get aube's fast installs, strict layout, and run-time
auto-install checks without editing each project's scripts.

## Behavior

- `npm install` and `npm ci` run through `aube`.
- `npm install <package>` is translated to `aube add <package>`.
- `npm uninstall <package>` is translated to `aube remove <package>`.
- Global package operations using `-g` or `--global` fall back to the real
  package manager for `add`, `install`, and `remove` variants.
- Global `outdated` operations using `-g` or `--global` run `mise outdated
--bump -C "$HOME"` so globally managed npm tools are checked through mise.
- `npm view`, `npm show`, and `npm info` with `--json` fall back to the real
  npm so tools such as mise can consume npm's registry metadata format.
- npm script commands such as `npm run build`, `npm test`, and `npm start` run
  through `aube`.
- npm-only commands such as `npm pkg`, `npm search`, and `npm whoami` fall back
  to the real npm.
- `pnpm` commands pass through to `aube`, since aube already presents a
  pnpm-compatible command surface.
- `yarn` commands route common package-manager commands and script names to
  `aube`; Yarn-specific management commands fall back to the real Yarn binary.
- `bun` routes package-manager commands such as `bun install`, `bun add`, and
  `bun run` to `aube`; runtime commands and unknown commands fall back to the
  real Bun binary.

## Install

The recommended install path is Cargo:

```sh
cargo install aubeshim
aubeshim install --force
```

That installs the `aubeshim` binary with Cargo and creates `bun`, `npm`,
`pnpm`, and `yarn` shims in `~/.local/share/aubeshim/shims`.

From a source checkout, use the development installer instead:

```sh
./install.sh
```

That builds the checkout, copies `aubeshim` to `~/.local/bin`, and replaces
shims in `~/.local/share/aubeshim/shims`.

Add the shim directory after mise activation so it stays ahead of mise's own
package-manager shims.

For zsh:

```sh
eval "$(aubeshim activate zsh)"
```

For bash:

```sh
eval "$(aubeshim activate bash)"
```

For fish:

```fish
aubeshim activate fish | source
```

For POSIX profile files:

```sh
eval "$(aubeshim activate sh)"
```

## Configuration

Environment variables can override tool discovery:

- `AUBESHIM_AUBE`: path to the aube binary.
- `AUBESHIM_REAL_BUN`: path to the real Bun binary.
- `AUBESHIM_REAL_NPM`: path to the real npm binary.
- `AUBESHIM_REAL_PNPM`: path to the real pnpm binary.
- `AUBESHIM_REAL_YARN`: path to the real Yarn binary.
- `AUBESHIM_SHIM_DIR`: path to the installed shim directory.

By default, real package-manager discovery asks `mise which` first, then falls
back to PATH.

If `mise` is installed, aubeshim requires mise 2026.5.6 or newer so aube-aware
tool discovery is available.

## Global npm Tools

Use mise for globally managed npm CLIs:

```sh
mise use -g npm:prettier@latest
mise use -g npm:@anthropic-ai/claude-code@latest
```

With mise's default `npm.package_manager = "auto"` setting, mise uses `aube`
for npm package installs when `aube` is available. aubeshim keeps that workflow
working by sending npm registry metadata commands such as `npm view ... --json`
to the real npm binary, then leaving mise to install and expose the resulting
global tool on PATH.

Plain package-manager global install and removal operations also fall back to
the real package manager because aube does not support those global command
shapes yet. Examples include `npm install -g`, `pnpm add -g`, `bun add -g`, and
`yarn add -g`.
