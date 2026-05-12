# aubeshim

`aubeshim` installs PATH shims that let existing `bun`, `npm`, `pnpm`, and
`yarn` commands use `aube` when the command shape is compatible.

The goal is to get aube's fast installs, strict layout, and run-time
auto-install checks without editing each project's scripts.

## Behavior

- `npm install` and `npm ci` run through `aube`.
- `npm install <package>` is translated to `aube add <package>`.
- `npm uninstall <package>` is translated to `aube remove <package>`.
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

From a checkout:

```sh
./install.sh
```

That copies `aubeshim` to `~/.local/bin` and replaces package-manager shims in
`~/.local/share/aubeshim/shims`.

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
