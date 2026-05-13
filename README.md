# aubeshim

`aubeshim` installs PATH shims that let existing `bun`, `npm`, `pnpm`, and
`yarn` commands use [aube](https://aube.en.dev) when the command shape is
compatible.

The goal is to get aube's fast installs, strict layout, and run-time
auto-install checks without editing each project's scripts.

For developers with many JavaScript checkouts using several different package
managers, that can mean hundreds of gigabytes of duplicate dependencies saved.

Note: `aubeshim` is a third-party project and is not associated with en.dev, the
organization behind aube and mise.

## Behavior

- Local npm installs and scripts run through `aube`. That includes
  `npm install`, `npm ci`, `npm run build`, `npm test`, and `npm start`.
- npm package edits are normalized to aube's command names:
  `npm install <package>` becomes `aube add <package>`, and
  `npm uninstall <package>` becomes `aube remove <package>`.
- `pnpm` commands pass through to `aube`, since aube already presents a
  pnpm-compatible command surface.
- `yarn` routes common package-manager commands and script names to `aube`;
  Yarn-specific management commands fall back to the real Yarn binary.
- `bun` routes package-manager commands such as `bun install`, `bun add`, and
  `bun run` to `aube`; runtime commands and unknown commands fall back to the
  real Bun binary.

Global npm tools are managed through mise:

- Global `outdated` operations using `-g` or `--global` run `mise outdated` with
  `--bump -C "$HOME"`.
- Global package add/install operations using `-g` or `--global` run
  `mise use -g npm:<package>`.
- Global package remove operations using `-g` or `--global` run
  `mise unuse -g npm:<package>`.

Commands that need npm's exact registry or account behavior fall back to the
real npm:

- `npm view`, `npm show`, and `npm info` with `--json` fall back so tools such
  as mise can consume npm's registry metadata format.
- `npm publish` and `npm unpublish` fall back to preserve npm's registry, auth,
  access, provenance, OTP, tag, workspace, and lifecycle semantics.
- npm-only commands such as `npm pkg`, `npm search`, and `npm whoami` fall back
  to the real npm.

## Install

The recommended install path is Cargo:

```sh
cargo install aubeshim
aubeshim install --force
```

That installs the `aubeshim` binary with Cargo and creates `bun`, `npm`, `pnpm`,
and `yarn` shims in `~/.local/share/aubeshim/shims`.

From a source checkout, use the development installer instead:

```sh
./install.sh
```

That builds the checkout, copies `aubeshim` to `~/.local/bin`, and replaces
shims in `~/.local/share/aubeshim/shims`.

Activate aubeshim after `mise activate` or any other tool manager that rewrites
`PATH`. mise installs its own package-manager shims, so aubeshim must activate
last for `bun`, `npm`, `pnpm`, and `yarn` to resolve to aubeshim.

For zsh:

```sh
eval "$(mise activate zsh --shims)"
eval "$(aubeshim activate zsh)"
```

For bash:

```sh
eval "$(mise activate bash --shims)"
eval "$(aubeshim activate bash)"
```

For fish:

```fish
mise activate fish --shims | source
aubeshim activate fish | source
```

For POSIX profile files:

```sh
eval "$(aubeshim activate sh)"
```

`aubeshim activate` removes existing aubeshim shim-dir entries before prepending
the shim directory, so it is safe to run more than once.

For zsh, put the activation in `.zshrc` for interactive terminals. If
non-interactive zsh processes also need the shims, for example editors or agents
that invoke zsh, add a guarded activation to `.zshenv` too:

```sh
if (( $+commands[aubeshim] )); then
  eval "$(aubeshim activate zsh)"
fi
```

If `.zshrc` later runs `mise activate`, keep the `.zshrc` aubeshim activation
after it so aubeshim remains first in `PATH`.

On Linux desktops, adding the POSIX activation to `.profile` can help GUI
applications launched by the session inherit the shims too, even if your
interactive shell is zsh. Many desktop sessions use `.profile` through `sh`
semantics rather than zsh startup files, depending on how your display manager
starts the user session:

```sh
if command -v mise >/dev/null 2>&1; then
  eval "$(mise activate sh --shims)"
fi
if command -v aubeshim >/dev/null 2>&1; then
  eval "$(aubeshim activate sh)"
fi
```

Keep the shell-specific activation in `.zshrc`, `.bashrc`, or equivalent for
interactive terminals, since `.profile` is not sourced by every shell startup
path.

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

aubeshim keeps that workflow working by sending npm registry metadata commands
such as `npm view ... --json` to the real npm binary, then leaving mise to
install and expose the resulting global tool on PATH.

Use `npm outdated -g`, `pnpm outdated -g`, `bun outdated -g`, or
`yarn outdated -g` to check those tools through mise. aubeshim translates those
commands to `mise outdated --bump -C "$HOME"` and passes package arguments as
`npm:<package>`.

Direct global add/install/remove commands for named packages also use mise.
Examples include `npm install -g prettier`, `pnpm add -g eslint`,
`bun add -g typescript`, and `yarn remove -g cowsay`.
