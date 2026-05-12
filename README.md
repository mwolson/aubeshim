# aubeshim

`aubeshim` installs PATH shims that let existing `npm` and `pnpm` commands use
`aube` when the command shape is compatible.

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

## Install

From a checkout:

```sh
./install.sh
```

That copies `aubeshim` to `~/.local/bin` and replaces package-manager shims in
`~/.local/share/aubeshim/shims`.

Add the shim directory after mise activation so it stays ahead of mise's own
`npm` and `pnpm` shims.

For zsh:

```sh
eval "$(aubeshim init zsh)"
```

For bash:

```sh
eval "$(aubeshim init bash)"
```

For fish:

```fish
aubeshim init fish | source
```

## Configuration

Environment variables can override tool discovery:

- `AUBESHIM_AUBE`: path to the aube binary.
- `AUBESHIM_REAL_NPM`: path to the real npm binary.
- `AUBESHIM_REAL_PNPM`: path to the real pnpm binary.
- `AUBESHIM_SHIM_DIR`: path to the installed shim directory.

By default, real package-manager discovery asks `mise which` first, then falls
back to PATH.
