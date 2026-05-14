# Issues To Report

These notes track package manager compatibility findings from local aubeshim
testing. They are written for later upstream reports against aube, not as
aubeshim implementation tasks unless stated otherwise.

## npm

Context:

- Project: `~/devel/alairo/iow/amp/amp-mobile`.
- App type: Expo and React Native.
- Lockfile: npm `package-lock.json`.
- aube version observed: `1.12.0`.
- aubeshim version observed: `0.4.1`.

Findings:

- aube can consume a valid npm generated `package-lock.json` for this project. A
  clean frozen aube install from an npm generated lock installed `expo-router`,
  ran `patch-package`, passed typecheck, passed tests, and assembled Android
  debug.
- aube cannot safely rewrite the npm lock for this project. Starting from the
  original lock and current manifest changes, aube resolved and linked
  `expo-router` during the immediate install, but wrote a `package-lock.json`
  with only the root `expo-router` dependency spec and no
  `packages["node_modules/expo-router"]` entry.
- A later clean frozen aube install from that aube-written lock omitted
  `expo-router` from `node_modules`, and typecheck failed with missing
  `expo-router` imports.
- Hoisted layout did not fix lockfile authoring. The immediate hoisted install
  linked `expo-router`, but the written `package-lock.json` still lacked the
  `node_modules/expo-router` package entry.
- Real npm rejected the aube-written hoisted lock as out of sync. It reported
  missing `expo-router`, missing `expo-router` transitives such as
  `@expo/server`, and invalid resolved versions for some peer-related entries.
- Pinning `expo-router` exactly and adding a package override did not fix the
  omitted lock entry.
- Importing the npm lock to native `aube-lock.yaml` did install `expo-router`
  correctly, so the issue appears specific to aube's npm lockfile rewrite path,
  not to the project manifest alone.

Related project cleanup that was not the lockfile bug:

- The isolated layout exposed real undeclared direct dependencies in
  `amp-mobile`: `expo-application`, `@react-navigation/elements`,
  `@expo/config-plugins`, and React Native community CLI packages.
- Expo autolinking generated a bad Android import for `ExpoModulesPackage` until
  the project provided a `react-native.config.ts` override. That may be an Expo
  autolinking interaction with isolated layouts rather than an npm lock writer
  issue.
- `aube.allowBuilds` worked for lifecycle approval. It removed ignored-build
  warnings for `@firebase/util`, `electron`, `lefthook`, and `protobufjs`.

Likely report shape:

- Minimal repro should focus on a direct dependency present in the root npm
  manifest where aube writes only the root dependency spec and omits the
  corresponding `packages["node_modules/<name>"]` lock entry.
- Include a follow-up check that real npm rejects the resulting lock and that a
  clean frozen aube install omits the package.

## bun

Context:

- Project: `~/devel/github/t3code`.
- App type: Bun workspace monorepo.
- Lockfile: Bun text `bun.lock`.
- aube version observed: `1.12.0`.
- aubeshim version observed: `0.4.1`.

Findings:

- Real Bun `1.3.9` accepts the checked-in `bun.lock`, performs a frozen install,
  and links workspace dependencies correctly. For example,
  `packages/client-runtime/node_modules/@t3tools/contracts` points to
  `packages/contracts`.
- aube can consume the checked-in `bun.lock` without rewriting it during a
  frozen install, but the materialized workspace layout is wrong. Under
  `packages/client-runtime`, `node_modules/@t3tools/contracts` pointed back to
  `packages/client-runtime` instead of `packages/contracts`.
- Because of that workspace link, `bun typecheck` after an aube isolated install
  failed with missing or wrong exports from `@t3tools/contracts`.
- Hoisted layout did not fix the workspace link. The same dependency still
  pointed back to the wrong workspace package, and typecheck failed.
- aube rewrote `bun.lock` during fix-lockfile mode even without an intentional
  manifest change. The resulting lock differed substantially from Bun's lock,
  including peer dependency materialization and workspace metadata changes.
- A later clean frozen aube install from the aube-written Bun lock failed,
  reporting that the lock was out of date for workspace dependencies.
- Real Bun rejected the aube-written `bun.lock` as invalid package information.
  The observed parse failure was around `@effect/platform-node` peer dependency
  resolution for `ioredis`, after which Bun ignored the lock and refused to
  continue because the lock was frozen.

Other observations:

- `bun run` script translation through aubeshim can work when `node_modules` was
  installed by real Bun.
- Nested package scripts emitted repeated warnings that `packageManager: bun` is
  unsupported by aube.
- aube built `node-pty`, but ignored build scripts for `esbuild`,
  `msgpackr-extract`, `msw`, and `sharp`. Those lifecycle approvals are a
  separate concern from the workspace link and lockfile rewrite bugs.

Likely report shape:

- Minimal repro should use a Bun workspace where package A depends on package B
  via `workspace:*`. After a frozen aube install from Bun's lockfile, package
  A's local `node_modules` should be checked to ensure package B links to the
  correct workspace directory.
- A separate repro should show that aube's Bun lock rewrite produces a lock that
  real Bun cannot parse and that a later frozen aube install cannot use.
