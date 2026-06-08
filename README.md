# aubeshim

`aubeshim` installs PATH shims that let existing `bun`, `bunx`, `npm`, `npx`,
`pnpm`, `pnpx`, `pnx`, and `yarn` commands use [aube](https://aube.en.dev) when
the command shape is compatible.

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
- npm-shimmed `aube` commands set `AUBE_NODE_LINKER=hoisted` for that invocation
  unless a node-linker env var is already set. This matches npm's hoisted
  `node_modules` shape without writing `.npmrc`.
- `pnpm` commands pass through to `aube`, since aube already presents a
  pnpm-compatible command surface.
- `yarn` routes common package-manager commands and script names to `aube`;
  Yarn-specific management commands fall back to the real Yarn binary.
- `bun` routes package-manager commands such as `bun install`, `bun add`, and
  `bun run` to `aube`; runtime commands and unknown commands fall back to the
  real Bun binary.
- One-off runner shims route compatible commands to `aube dlx`. That includes
  `bunx`, `npx`, `pnpx`, `pnx`, `bun x`, `bun dlx`, and `pnpm dlx`.
- One-off runner no-install modes use `aube exec --no-install`. That includes
  `bunx --no-install`, `bun dlx --no-install`, and `npx --no-install`.
- Runner flags that need exact package-manager behavior fall back to the real
  tool. Examples include `bunx --bun`, `npx --workspace`, and
  `pnpx --allow-build`.
- `--version` and `-v` print the real package manager version. In repos where
  aubeshim is configured to shim, they also print the aubeshim and aube versions
  in a parenthesized hint.

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

That installs the `aubeshim` binary with Cargo and creates `bun`, `bunx`, `npm`,
`npx`, `pnpm`, `pnpx`, `pnx`, and `yarn` shims in
`~/.local/share/aubeshim/shims`.

From a source checkout, use the development installer instead:

```sh
./install.sh
```

That builds the checkout, copies `aubeshim` to `~/.local/bin`, and replaces
shims in `~/.local/share/aubeshim/shims`.

Activate aubeshim after `mise activate` or any other tool manager that rewrites
`PATH`. mise installs its own package-manager shims, so aubeshim must activate
last for `bun`, `bunx`, `npm`, `npx`, `pnpm`, `pnpx`, `pnx`, and `yarn` to
resolve to aubeshim.

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

`aubeshim` reads a TOML config file from:

- `AUBESHIM_CONFIG`, when set.
- `$XDG_CONFIG_HOME/aubeshim/config.toml`, when `XDG_CONFIG_HOME` is set.
- `~/.config/aubeshim/config.toml`, otherwise.

The config file controls whether a directory uses the aube shim or passes
through to the real package manager:

```toml
enabled = true
default = true

ignore = [
  "~/devel/work/broken-expo",
  "~/devel/work/legacy/**",
]

shim = [
  "~/devel/work/*",
  "~/devel/projects/**",
]
```

`enabled` controls whether aubeshim does any shimming at all and defaults to
`true`. When `enabled = false`, every invocation passes through to the real
`bun`, `bunx`, `npm`, `npx`, `pnpm`, `pnpx`, `pnx`, or `yarn`.

`ignore` is a list of directory globs that should pass through to the real
package manager. `shim` is a list of directory globs that should use `aube`.
`default` controls what happens when no directory glob matches and defaults to
`true`.

Precedence is:

1. `enabled`
2. `ignore`
3. `shim`
4. `default`

Globs match the current working directory or any ancestor directory. This means
a command run from `packages/app` still matches a glob for the package,
workspace, or parent directory that contains it. Use absolute paths or `~` so
the config keeps working no matter where the command starts.

`*` matches within a single path component. Because globs are checked against
the current directory and its ancestors, `~/devel/work/*` matches commands run
inside any immediate child directory of `~/devel/work`. `**` is recursive and
can match zero or more path components, so use it for directories that may live
under nested paths, such as `~/devel/projects/**`. A trailing `/**` also matches
the base directory itself.

For a config managed by `~/dotfiles`, symlink it into the default location:

```sh
mkdir -p ~/.config/aubeshim
ln -s ~/dotfiles/config/aubeshim/config.toml ~/.config/aubeshim/config.toml
```

Environment variables can override tool discovery:

- `AUBESHIM_CONFIG`: path to the aubeshim config file.
- `AUBESHIM_AUBE`: path to the aube binary.
- `AUBESHIM_REAL_BUN`: path to the real Bun binary.
- `AUBESHIM_REAL_BUNX`: path to the real bunx binary.
- `AUBESHIM_REAL_NPM`: path to the real npm binary.
- `AUBESHIM_REAL_NPX`: path to the real npx binary.
- `AUBESHIM_REAL_PNPM`: path to the real pnpm binary.
- `AUBESHIM_REAL_PNPX`: path to the real pnpx binary.
- `AUBESHIM_REAL_PNX`: path to the real pnx binary.
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

## Compatibility Notes

Known package-manager interop findings that need later upstream reports are
tracked in [aube-issues](https://github.com/mwolson/tmp-aube-issues).
