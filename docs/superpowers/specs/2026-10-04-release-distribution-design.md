# Release and distribution: one-command installs for v0.1.0 — design

Date: 2026-10-04. Status: approved in conversation, implementation follows.
Builds on the built CLI and TUI (`docs/build-spec.md`,
`docs/superpowers/specs/2026-09-22-tui-design.md`). Adds no CLI feature.

## Decision

Vernier is built from source today: clone, `cargo build --release`. Launch
needs a one-command install on every common platform. A `v*` tag runs one
hand-written workflow, `.github/workflows/release.yml`, that builds five
targets on native runners, creates the GitHub Release, publishes to npm and
updates a Homebrew tap. Two small installer scripts download from the
release. The workflow is read end to end; no generator (cargo-dist) owns it.

cargo-dist was rejected: its npm installer downloads the binary in a
postinstall script, which fails under `--ignore-scripts`, pnpm's default
script policy, and offline or proxied CI, and its generated workflow is not
meant to be edited by hand.

## Names

| Channel | Name | Install |
| --- | --- | --- |
| npm | `@go-vernier/cli` | `npx @go-vernier/cli analyze .` or `npm i -g @go-vernier/cli` |
| Homebrew | `go-vernier/tap/vernier` | `brew install go-vernier/tap/vernier` |
| Shell | `install.sh` release asset | `curl -fsSL https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.sh \| sh` |
| PowerShell | `install.ps1` release asset | `irm https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.ps1 \| iex` |
| Source | — | `cargo install --git https://github.com/Go-Vernier/Vernier-OSS vernier-cli` |

The installed command is `vernier` everywhere. The unscoped npm name
`vernier` and the crates.io names `vernier`, `vernier-cli` and `vernier-core`
belong to unrelated projects. Vernier is not published to crates.io in this
work; the crate names do not change.

## Targets

| Target | Runner | Archive |
| --- | --- | --- |
| `aarch64-apple-darwin` | `macos-latest` | `.tar.gz` |
| `x86_64-apple-darwin` | `macos-15-intel` | `.tar.gz` |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` + `musl-tools` | `.tar.gz` |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` + `musl-tools` | `.tar.gz` |
| `x86_64-pc-windows-msvc` | `windows-latest` | `.zip` |

Every target builds on a native runner, so the tree-sitter grammars (C) are
never cross-compiled. Linux binaries are static musl: they run on glibc and
musl distributions alike, including Alpine-based Node images. HTTP goes
through ureq with rustls, so there is no OpenSSL to link.

## Release workflow

`release.yml` runs on `push` of a `v*` tag; that is the only event that
publishes. Every other trigger is a dry run: `workflow_dispatch` (no inputs),
and `pull_request` when the change touches `release.yml`, `npm/**` or
`scripts/**`. The pull request trigger exists because GitHub only dispatches
workflows present on the default branch, so it is how the pipeline is proven
before it is merged.

1. **check.** `node scripts/release.mjs version [tag]` reads the
   `[workspace.package] version` from `Cargo.toml`. On a tag push it fails
   unless the tag equals `v` + that version. It always fails unless
   `CHANGELOG.md` has a `## [<version>]` section. Outputs: `version`,
   `publish` (true only on a tag push), `prerelease` (true when the version
   has a `-` suffix, such as `0.2.0-rc.1`).
2. **build** (matrix, the five targets). `cargo build --release --locked -p
   vernier-cli --target <t>`. Smoke test: the built binary runs `--version`
   (output must contain the version) and `analyze .`. Packs
   `vernier-<target>.tar.gz` (or `.zip`) holding `vernier[.exe]`, `LICENSE`
   and `README.md`, and writes `vernier-<target>.<ext>.sha256` beside it in
   `sha256sum` format. Asset names carry no version, so
   `releases/latest/download/<name>` always resolves to the newest release.
   The build also uploads the bare binary as `bin-<target>/vernier[.exe]`
   for the npm packages.
3. **smoke** (matrix: ubuntu-latest, macos-latest, windows-latest). Downloads the
   build artifacts. Generates the npm packages, `npm pack`s the main package
   and the runner's platform package, installs both tarballs into a temporary
   project with `--omit=optional`, and runs `npx vernier --version` and
   `npx vernier analyze test/fixtures/edges-http-app`. On Linux and macOS it also runs
   `install.sh` with `VERNIER_BASE_URL=file://<artifacts dir>` into a
   temporary directory and runs the installed binary. On Linux it runs the
   x64 musl binary in an `alpine` container. On Windows it serves the
   artifacts over a local HTTP server and runs `install.ps1` with
   `VERNIER_BASE_URL` under both PowerShell 7 and Windows PowerShell 5.1.
4. **release** (skipped on dry run). Writes `SHA256SUMS` over every archive,
   extracts the version's section of `CHANGELOG.md` as the release notes, and
   creates the GitHub Release for the tag with the archives, the `.sha256`
   files, `SHA256SUMS`, `install.sh` and `install.ps1`. A prerelease version
   makes a GitHub prerelease. If the release already exists, the assets are
   re-uploaded over it.
5. **npm** (after release; on dry run, after smoke, with `npm publish
   --dry-run`). Publishes the five platform packages, then the main package.
   A prerelease is published under the `next` dist-tag, not `latest`.
6. **homebrew** (after release; skipped for prereleases; on dry run, prints
   the formula and checks its Ruby syntax only). Generates
   `Formula/vernier.rb` and commits it to the tap.
7. **verify** (after npm and homebrew; skipped on dry run and for
   prereleases; matrix: ubuntu-latest, macos-latest, windows-latest). Runs
   the published one-liners on
   clean runners: `curl | sh` (Linux, macOS), `irm | iex` (Windows),
   `npx @go-vernier/cli --version` (all), `brew install
   go-vernier/tap/vernier` (macOS). Each must print the version.

The GitHub Release is created before npm and Homebrew. If a later job fails,
the release stands and that job is re-run alone; no rebuild is needed.

## npm packages

`npm/cli/` (committed) is the main package `@go-vernier/cli`:

- `package.json`: version `0.0.0-dev` (stamped at publish), `bin: {"vernier":
  "bin/vernier.js"}`, `optionalDependencies` on the five platform packages at
  the same placeholder version (stamped together), `engines.node >= 18`,
  `os`/`cpu` unset, no lifecycle scripts, `files: ["bin", "lib"]`.
- `lib/platform.js`: `packageFor(platform, arch)` maps `darwin`/`linux` ×
  `arm64`/`x64` and `win32`/`x64` to a package name, else `null`;
  `binaryName(platform)` is `vernier.exe` on `win32`, else `vernier`.
- `bin/vernier.js`: resolves `<package>/bin/<binary>` with `require.resolve`,
  runs it with `spawnSync(binary, process.argv.slice(2), { stdio: "inherit"
  })`. Exits with the child's status. When the child died of a signal, the
  launcher sends itself that signal. When the platform is unsupported or the
  package is missing, it prints to stderr one message naming
  `process.platform`/`process.arch` and pointing at the shell and PowerShell
  installers, and exits 1.
- `README.md`: install and the three most-used commands; links to the repo.
- `test/`: `node --test` tests for the mapping and the launcher; not
  published.

`scripts/targets.mjs` lists the five targets once (triple, npm `os`/`cpu`,
archive type) for every packaging script.

`scripts/npm-packages.mjs <version> <bins-dir> <out-dir>` writes
`<out-dir>/cli` (a copy of `npm/cli` with the version stamped into
`version` and every `optionalDependencies` entry) and one
`<out-dir>/cli-<os>-<cpu>` per target. Every target's binary must be in
`<bins-dir>`, and `<out-dir>` must be empty or absent. A
platform package's `package.json` has `name`, `version`, `description`,
`license`, `repository`, `os`, `cpu`, `files: ["bin"]`, and
`preferUnplugged: true`; its `bin/` holds the binary, mode 0755. There is no
`libc` field: the static musl binary runs on both libcs.

Publishing uses the `NPM_TOKEN` secret with `npm publish --access public
--provenance`. Platform packages go first, the main package last, so the
main package never names a version that does not exist. A package whose
version already exists on the registry (`npm view <name>@<version>
version` succeeds) is skipped, so the job can be re-run. Moving to npm
trusted publishing (OIDC) once the packages exist is noted in
`docs/releasing.md`, not done here.

## Homebrew

The tap is the public repository `Go-Vernier/homebrew-tap`.
`scripts/homebrew-formula.mjs <version> <artifacts-dir>` prints
`Formula/vernier.rb`: `desc`, `homepage`, `version`, `license "MIT"`,
`on_macos`/`on_linux` × `on_arm`/`on_intel` blocks each with the release
asset `url` and its `sha256`, `def install; bin.install "vernier"; end`,
and `test do assert_match version.to_s, shell_output("#{bin}/vernier
--version") end`. Windows is not in the formula.

The homebrew job clones the tap with the `HOMEBREW_TAP_TOKEN` secret (a
fine-grained token, contents write on the tap only), writes the formula,
and commits `vernier <version>` to its default branch. If the file is
unchanged it commits nothing.

## Installer scripts

`scripts/install.sh` is POSIX `sh` and passes `shellcheck`.

- Target: `uname -s` `Darwin`/`Linux` × `uname -m` `arm64`/`aarch64`/
  `x86_64`/`amd64`. Anything else fails with a message naming the pair.
- Version: `VERNIER_VERSION` (`v0.1.0` or `0.1.0`) selects
  `releases/download/v<version>/`; unset means `releases/latest/download/`.
  `VERNIER_BASE_URL` overrides the whole base URL (used by tests; not in the
  README).
- Download with `curl -fsSL`, else `wget -qO-`; neither is an error. The
  `.sha256` file is checked with `sha256sum -c`, else `shasum -a 256 -c`; a
  mismatch removes the download and fails.
- Installs to `VERNIER_INSTALL_DIR`, default `$HOME/.local/bin`, creating it.
  Prints the installed path and `vernier --version`. If the directory is not
  on `PATH`, prints the line to add.
- Work happens in a `mktemp -d` directory removed on exit.

`scripts/install.ps1`:

- Supports x64, and Windows on ARM through x64 emulation.
- `VERNIER_VERSION`, `VERNIER_BASE_URL` and `VERNIER_INSTALL_DIR` (default
  `$env:LOCALAPPDATA\Programs\vernier\bin`) as above.
- Runs inside a script block, so `irm | iex` leaves no variables or
  preference changes behind in the user's session. Enables TLS 1.2, which
  Windows PowerShell 5.1 on older Windows does not use by default.
- Downloads with `Invoke-WebRequest`, verifies with `Get-FileHash -Algorithm
  SHA256` against the `.sha256` file, extracts with `Expand-Archive`.
- Adds the directory to the user `PATH` (never the machine `PATH`) when
  absent, and to the current session.

## CI changes

`ci.yml`:

- The test matrix gains `windows-latest`. The smoke step stays.
- A `packaging` job on ubuntu-latest: the `node --test` suites under
  `npm/cli/test` and `scripts/test` (including `install.sh` against a fake
  release), `shellcheck scripts/install.sh`, a PowerShell parse of
  `install.ps1`, and `actionlint` over the workflows.

`.gitattributes`: `* text=auto eol=lf`, with `*.ps1 text eol=crlf`, so
Windows checkouts keep the LF fixtures in `test/` byte-identical.

Windows failures that CI surfaces are fixed in the code before the first
tag. A fix larger than a path-separator or command-invocation correction is
brought back for a decision before it is made.

## Version

`[workspace.package] version` in `Cargo.toml` is the source of truth and
moves to `0.1.0`; `Cargo.lock` follows. The root `package.json` (private,
not published) moves to `0.1.0` with it. `npm/cli/package.json` stays
`0.0.0-dev` in the repository.

## Launch kit

- **README.** An **Install** section directly after the intro with the five
  ways in the Names table. The "Not yet published" line goes. The
  "status: phase 0" badge is replaced by a latest-release badge and an npm
  version badge. A demo GIF (`docs/demo.gif`) sits under the tagline once
  recorded; until then the README does not reference it.
- **Demo.** `docs/demo.tape` is a VHS script that runs `vernier tui` on
  `instana/robot-shop` and walks a blast radius. `.github/workflows/demo.yml`
  (`workflow_dispatch` only, so it runs once merged to `main`) builds the binary, clones robot-shop, runs VHS,
  and uploads `demo.gif` as an artifact for a maintainer to commit.
- **CHANGELOG.md.** Keep a Changelog format. `## [0.1.0] - <release date>`
  lists what ships: service discovery, static edges, the runtime join
  (OpenTelemetry, Datadog), the blast radius (`--pr`, `--diff`, `--files`,
  `--history`), the terminal, JSON and HTML reports, and the TUI.
- **docs/releasing.md.** One-time setup (npm org and token, tap repository
  and token), cutting a release (bump version, changelog section, dry run,
  tag), and recovery (re-running a failed job; publishing a fix release —
  npm versions cannot be reused).

## Maintainer setup (outside the repository)

1. Create the npm organisation `go-vernier`; add an automation token as the
   repository secret `NPM_TOKEN`.
2. Create the public repository `Go-Vernier/homebrew-tap`; add a
   fine-grained token with contents write on it as `HOMEBREW_TAP_TOKEN`.
3. Merge to `main` once the pull request's release dry run passes; then
   push `v0.1.0` from `main`.

## Testing

| What | Where |
| --- | --- |
| Rust on Linux, macOS, Windows | `ci.yml` test matrix |
| Launcher mapping, arguments, exit codes, signals | `node --test` in `ci.yml` packaging |
| npm package generation, formula, changelog notes | `node --test` in `ci.yml` packaging |
| `install.sh` install, upgrade, checksum and download failures | `node --test` in `ci.yml` packaging |
| Script and workflow lint | `shellcheck`, PowerShell parse, `actionlint` in `ci.yml` packaging |
| Each release binary runs | `release.yml` build smoke |
| npm packages install and run, three OSes | `release.yml` smoke |
| `install.sh` installs from artifacts | `release.yml` smoke |
| Published one-liners work | `release.yml` verify |

The development machine has no Rust toolchain; Rust and Windows changes are
verified by CI on the pushed `release-distribution` branch.

## Out of scope

- A Markdown output format and a GitHub Action that comments the blast
  radius on pull requests (follow-up).
- A GitHub Pages gallery of HTML reports for the tested repositories.
- Launch-post drafts.
- crates.io publishing and crate renames; homebrew-core submission.
- Code signing and notarisation of the macOS and Windows binaries.
