# Release and Distribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `v0.1.0` tag builds five prebuilt binaries and makes `vernier` installable with one command through npm, Homebrew, `curl | sh` and PowerShell.

**Architecture:** One hand-written GitHub Actions workflow (`release.yml`) builds on native runners, smoke-tests every channel, then creates the GitHub Release, publishes the npm packages and updates the Homebrew tap. The packaging logic lives in small, tested Node scripts under `scripts/` (no dependencies) and a launcher package under `npm/cli/`. Two installer scripts download from the release.

**Tech Stack:** GitHub Actions; Rust (stable, `--locked`) with musl on Linux; Node 18+ (`node:test`, no npm dependencies); POSIX `sh`; PowerShell 5.1 and 7; Homebrew formula (Ruby); VHS for the demo.

**Spec:** `docs/superpowers/specs/2026-10-04-release-distribution-design.md`

## Global Constraints

- The installed command is `vernier` on every channel.
- npm: main package `@go-vernier/cli`; platform packages `@go-vernier/cli-darwin-arm64`, `@go-vernier/cli-darwin-x64`, `@go-vernier/cli-linux-arm64`, `@go-vernier/cli-linux-x64`, `@go-vernier/cli-win32-x64`. No lifecycle (install) scripts in any package. `engines.node >= 18`.
- Homebrew: tap `Go-Vernier/homebrew-tap`, formula `Formula/vernier.rb`, install `brew install go-vernier/tap/vernier`.
- Targets (runner): `aarch64-apple-darwin` (`macos-latest`), `x86_64-apple-darwin` (`macos-15-intel`), `x86_64-unknown-linux-musl` (`ubuntu-latest`), `aarch64-unknown-linux-musl` (`ubuntu-24.04-arm`), `x86_64-pc-windows-msvc` (`windows-latest`).
- Release asset names carry no version: `vernier-<triple>.tar.gz` (`.zip` on Windows), each with `vernier-<triple>.<ext>.sha256` in `sha256sum` format. Archives hold `vernier[.exe]`, `LICENSE`, `README.md` at the top level.
- Version source of truth: `[workspace.package] version` in `Cargo.toml`, `0.1.0`. `npm/cli/package.json` stays `0.0.0-dev` in the repository.
- Only a `v*` tag push publishes. `pull_request` (touching `release.yml`, `npm/**`, `scripts/**`) and `workflow_dispatch` are dry runs.
- Secrets: `NPM_TOKEN`, `HOMEBREW_TAP_TOKEN`.
- Installer environment: `VERNIER_VERSION`, `VERNIER_INSTALL_DIR`, `VERNIER_BASE_URL` (testing only, not in the README).
- Defaults: `install.sh` → `$HOME/.local/bin`; `install.ps1` → `$env:LOCALAPPDATA\Programs\vernier\bin`, user `PATH` only.
- Not in scope: crates.io, crate renames, homebrew-core, code signing, a Markdown output or GitHub Action, a report gallery, launch posts.
- Repository style: short comments that say what a file is for, no comment noise; commit messages in the repository's `type: summary` form, ending with the `Co-Authored-By` line.
- The development machine has no Rust toolchain. Rust changes are verified by CI (Task 11). Node, `sh` and Ruby are available locally.

## Review Focus

1. **The Linux binary on Alpine** (`npx` inside `node:alpine`, a common CI image): a reasonable person expects it to run. Pinned by the Alpine container step in the release smoke job (Task 9).
2. **`irm | iex` under Windows PowerShell 5.1**, the default shell on Windows: a reasonable person expects it to install over TLS 1.2 without PowerShell 7. Pinned by running `install.ps1` under both `pwsh` and `powershell` in the release smoke job (Task 9).
3. **Re-running the installer to upgrade**: a reasonable person expects the new binary to replace the old one in place. Pinned by the "upgrade" test in `scripts/test/install.test.mjs` (Task 5).
4. **A bad download** (checksum mismatch, or a version that does not exist): a reasonable person expects a clear error, a non-zero exit, and nothing installed. Pinned by the "checksum" and "missing release" tests (Task 5).
5. **Scripts in CI that check `vernier`'s exit code through `npx`**: a reasonable person expects the launcher to pass the exit status, arguments and signals through unchanged, and to explain a missing platform package (Windows on ARM, `--omit=optional`). Pinned by `npm/cli/test/launcher.test.js` (Task 1).

## File Structure

| File | Responsibility |
| --- | --- |
| `scripts/targets.mjs` | The five targets, and their npm and asset names. Single source for every script. |
| `npm/cli/package.json` | The `@go-vernier/cli` manifest (version placeholder). |
| `npm/cli/lib/platform.js` | Runtime map from `process.platform`/`process.arch` to a platform package. |
| `npm/cli/bin/vernier.js` | The launcher: find the binary, run it, pass status and signals through. |
| `npm/cli/README.md` | The npm page. |
| `npm/cli/test/*.test.js` | Mapping and launcher tests (not published). |
| `scripts/npm-packages.mjs` | Writes all six npm packages for a version. |
| `scripts/release.mjs` | Reads the release version from `Cargo.toml`, checks a tag, prints release notes from `CHANGELOG.md`. |
| `scripts/homebrew-formula.mjs` | Prints the Homebrew formula for a version. |
| `scripts/install.sh` | The macOS and Linux installer. |
| `scripts/install.ps1` | The Windows installer. |
| `scripts/test/*.test.mjs` | Tests for the scripts above and for the repository's release metadata. |
| `.github/workflows/release.yml` | Build, smoke, release, npm, Homebrew, verify. |
| `.github/workflows/ci.yml` | Adds Windows and a packaging job. |
| `.github/workflows/demo.yml` | Records the README GIF. |
| `.gitattributes` | LF line endings everywhere, CRLF for `.ps1`. |
| `docs/demo.tape` | The VHS script for the GIF. |
| `CHANGELOG.md` | Release notes, one section per version. |
| `docs/releasing.md` | The maintainer's checklist. |
| `README.md` | Install section, badges. |

Run every Node test with: `node --test npm/cli/test/*.test.js scripts/test/*.test.mjs` (also `npm run test:packaging` once Task 1 adds it).

---

### Task 1: The launcher package `@go-vernier/cli`

**Files:**
- Create: `scripts/targets.mjs`
- Create: `npm/cli/package.json`
- Create: `npm/cli/lib/platform.js`
- Create: `npm/cli/bin/vernier.js`
- Create: `npm/cli/README.md`
- Create: `npm/cli/test/platform.test.js`
- Create: `npm/cli/test/launcher.test.js`
- Modify: `package.json` (add the `test:packaging` script)

**Interfaces:**
- Produces (`scripts/targets.mjs`, ESM):
  - `TARGETS: Array<{ triple: string, os: "darwin"|"linux"|"win32", cpu: "arm64"|"x64", archive: "tar.gz"|"zip" }>`
  - `npmDir(t) -> string` — `cli-<os>-<cpu>`
  - `npmName(t) -> string` — `@go-vernier/cli-<os>-<cpu>`
  - `assetName(t) -> string` — `vernier-<triple>.<archive>`
  - `binaryName(t) -> string` — `vernier.exe` for win32, else `vernier`
- Produces (`npm/cli/lib/platform.js`, CommonJS):
  - `PACKAGES: { [key: "<platform> <arch>"]: string }`
  - `packageFor(platform: string, arch: string) -> string | null`
  - `binaryName(platform: string) -> string`

- [ ] **Step 1: Write `scripts/targets.mjs`**

```js
// The five release targets and what each one is called on npm and in a
// GitHub release. Keep in step with the build matrix in
// .github/workflows/release.yml and PACKAGES in npm/cli/lib/platform.js.

export const TARGETS = [
  { triple: "aarch64-apple-darwin", os: "darwin", cpu: "arm64", archive: "tar.gz" },
  { triple: "x86_64-apple-darwin", os: "darwin", cpu: "x64", archive: "tar.gz" },
  { triple: "aarch64-unknown-linux-musl", os: "linux", cpu: "arm64", archive: "tar.gz" },
  { triple: "x86_64-unknown-linux-musl", os: "linux", cpu: "x64", archive: "tar.gz" },
  { triple: "x86_64-pc-windows-msvc", os: "win32", cpu: "x64", archive: "zip" },
];

export const npmDir = (t) => `cli-${t.os}-${t.cpu}`;
export const npmName = (t) => `@go-vernier/${npmDir(t)}`;
export const assetName = (t) => `vernier-${t.triple}.${t.archive}`;
export const binaryName = (t) => (t.os === "win32" ? "vernier.exe" : "vernier");
```

- [ ] **Step 2: Write the failing mapping test `npm/cli/test/platform.test.js`**

```js
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { PACKAGES, packageFor, binaryName } = require("../lib/platform");
const manifest = require("../package.json");

test("each supported platform maps to its package", () => {
  assert.equal(packageFor("darwin", "arm64"), "@go-vernier/cli-darwin-arm64");
  assert.equal(packageFor("darwin", "x64"), "@go-vernier/cli-darwin-x64");
  assert.equal(packageFor("linux", "arm64"), "@go-vernier/cli-linux-arm64");
  assert.equal(packageFor("linux", "x64"), "@go-vernier/cli-linux-x64");
  assert.equal(packageFor("win32", "x64"), "@go-vernier/cli-win32-x64");
});

test("unsupported platforms have no package", () => {
  const unsupported = [
    ["win32", "arm64"],
    ["win32", "ia32"],
    ["freebsd", "x64"],
    ["linux", "ia32"],
    ["linux", "s390x"],
    ["darwin", "ia32"],
  ];
  for (const [platform, arch] of unsupported) {
    assert.equal(packageFor(platform, arch), null, `${platform} ${arch}`);
  }
});

test("the binary is vernier.exe on Windows only", () => {
  assert.equal(binaryName("win32"), "vernier.exe");
  assert.equal(binaryName("darwin"), "vernier");
  assert.equal(binaryName("linux"), "vernier");
});

test("the manifest depends optionally on exactly the mapped packages", () => {
  assert.deepEqual(
    Object.keys(manifest.optionalDependencies).sort(),
    Object.values(PACKAGES).sort(),
  );
  for (const version of Object.values(manifest.optionalDependencies)) {
    assert.equal(version, manifest.version);
  }
});

test("the manifest has no install scripts", () => {
  assert.equal(manifest.scripts, undefined);
});
```

- [ ] **Step 3: Run it to see it fail**

Run: `node --test npm/cli/test/platform.test.js`
Expected: FAIL with `Cannot find module '../lib/platform'`.

- [ ] **Step 4: Write `npm/cli/lib/platform.js`**

```js
"use strict";
// Which npm package carries the prebuilt vernier binary for this machine.
// Keep in step with TARGETS in scripts/targets.mjs.

const PACKAGES = {
  "darwin arm64": "@go-vernier/cli-darwin-arm64",
  "darwin x64": "@go-vernier/cli-darwin-x64",
  "linux arm64": "@go-vernier/cli-linux-arm64",
  "linux x64": "@go-vernier/cli-linux-x64",
  "win32 x64": "@go-vernier/cli-win32-x64",
};

function packageFor(platform, arch) {
  const key = `${platform} ${arch}`;
  return Object.hasOwn(PACKAGES, key) ? PACKAGES[key] : null;
}

function binaryName(platform) {
  return platform === "win32" ? "vernier.exe" : "vernier";
}

module.exports = { PACKAGES, packageFor, binaryName };
```

- [ ] **Step 5: Write `npm/cli/package.json`**

```json
{
  "name": "@go-vernier/cli",
  "version": "0.0.0-dev",
  "description": "Which services can this change reach? See the blast radius of a pull request before you merge it.",
  "license": "MIT",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/Go-Vernier/Vernier-OSS.git",
    "directory": "npm/cli"
  },
  "homepage": "https://github.com/Go-Vernier/Vernier-OSS#readme",
  "bugs": "https://github.com/Go-Vernier/Vernier-OSS/issues",
  "keywords": [
    "vernier",
    "microservices",
    "dependency-graph",
    "static-analysis",
    "opentelemetry",
    "pull-request",
    "impact-analysis",
    "blast-radius",
    "cli"
  ],
  "bin": {
    "vernier": "bin/vernier.js"
  },
  "files": [
    "bin",
    "lib"
  ],
  "engines": {
    "node": ">=18"
  },
  "optionalDependencies": {
    "@go-vernier/cli-darwin-arm64": "0.0.0-dev",
    "@go-vernier/cli-darwin-x64": "0.0.0-dev",
    "@go-vernier/cli-linux-arm64": "0.0.0-dev",
    "@go-vernier/cli-linux-x64": "0.0.0-dev",
    "@go-vernier/cli-win32-x64": "0.0.0-dev"
  }
}
```

- [ ] **Step 6: Run the mapping test to see it pass**

Run: `node --test npm/cli/test/platform.test.js`
Expected: PASS, 5 tests.

- [ ] **Step 7: Write the failing launcher test `npm/cli/test/launcher.test.js`**

```js
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { packageFor } = require("../lib/platform");

const CLI = path.resolve(__dirname, "..");
const pkg = packageFor(process.platform, process.arch);
const posix = process.platform !== "win32" && pkg !== null;
const skipUnlessPosix = posix ? false : "needs macOS or Linux on a supported CPU";

// A temporary project with @go-vernier/cli installed. When `script` is given,
// the platform package is installed too, its bin/vernier being that shell
// script.
function project(script) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "vernier-launcher-"));
  const main = path.join(root, "node_modules", "@go-vernier", "cli");
  fs.cpSync(path.join(CLI, "bin"), path.join(main, "bin"), { recursive: true });
  fs.cpSync(path.join(CLI, "lib"), path.join(main, "lib"), { recursive: true });
  if (script !== undefined) {
    const bin = path.join(root, "node_modules", pkg, "bin");
    fs.mkdirSync(bin, { recursive: true });
    fs.writeFileSync(path.join(bin, "vernier"), `#!/bin/sh\n${script}\n`, { mode: 0o755 });
  }
  return path.join(main, "bin", "vernier.js");
}

const run = (launcher, args = []) =>
  spawnSync(process.execPath, [launcher, ...args], { encoding: "utf8" });

test("passes arguments through and exits with the binary's status", { skip: skipUnlessPosix }, () => {
  const out = run(project('printf "%s|" "$@"; exit 3'), ["analyze", "a b", "--json"]);
  assert.equal(out.stdout, "analyze|a b|--json|");
  assert.equal(out.status, 3);
});

test("exits 0 when the binary does", { skip: skipUnlessPosix }, () => {
  assert.equal(run(project("exit 0")).status, 0);
});

test("dies of the signal that killed the binary", { skip: skipUnlessPosix }, () => {
  const out = run(project("kill -TERM $$"));
  assert.equal(out.signal, "SIGTERM");
});

test("explains a missing platform package", { skip: pkg === null ? "unsupported platform" : false }, () => {
  const out = run(project(undefined));
  assert.equal(out.status, 1);
  assert.match(out.stderr, /@go-vernier\/cli-\S+ is not installed/);
  assert.match(out.stderr, /install\.sh/);
  assert.match(out.stderr, /install\.ps1/);
});
```

- [ ] **Step 8: Run it to see it fail**

Run: `node --test npm/cli/test/launcher.test.js`
Expected: FAIL — `ENOENT` copying `npm/cli/bin`.

- [ ] **Step 9: Write `npm/cli/bin/vernier.js`**

```js
#!/usr/bin/env node
"use strict";
// Runs the prebuilt vernier binary from the platform package npm installed
// beside this one.

const { spawnSync } = require("node:child_process");
const { packageFor, binaryName } = require("../lib/platform");

const INSTALL_ELSEWHERE = [
  "Install vernier another way:",
  "  curl -fsSL https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.sh | sh",
  "  irm https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.ps1 | iex",
].join("\n");

function fail(message) {
  process.stderr.write(`vernier: ${message}\n${INSTALL_ELSEWHERE}\n`);
  process.exit(1);
}

const pkg = packageFor(process.platform, process.arch);
if (pkg === null) {
  fail(`there is no prebuilt binary for ${process.platform} ${process.arch}.`);
}

let binary;
try {
  binary = require.resolve(`${pkg}/bin/${binaryName(process.platform)}`);
} catch {
  fail(`${pkg} is not installed (were optional dependencies skipped?).`);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  fail(`could not run ${binary}: ${result.error.message}`);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
} else {
  process.exit(result.status ?? 1);
}
```

Then make it executable: `chmod +x npm/cli/bin/vernier.js`.

- [ ] **Step 10: Run the launcher test to see it pass**

Run: `node --test npm/cli/test/launcher.test.js`
Expected: PASS, 4 tests.

- [ ] **Step 11: Write `npm/cli/README.md`**

````markdown
# @go-vernier/cli

**Which services can this change reach?** Vernier reads a repository, finds
its services, maps which service calls which, and shows the blast radius of a
pull request before you merge it.

```bash
npx @go-vernier/cli analyze .              # services, edges and findings
npx @go-vernier/cli analyze . --pr 481     # blast radius of one pull request
npx @go-vernier/cli tui .                  # explore it interactively
```

Or install it: `npm install -g @go-vernier/cli`, then run `vernier`.

This package runs a prebuilt binary for macOS (Apple silicon, Intel), Linux
(x64, arm64) and Windows (x64), installed as an optional dependency. Nothing
runs at install time, and nothing is downloaded at run time.

Documentation, other ways to install, and the source:
<https://github.com/Go-Vernier/Vernier-OSS>
````

- [ ] **Step 12: Add the test script to the root `package.json`**

In `package.json`, replace

```json
  "scripts": {
    "corpus": "sh scripts/corpus.sh"
  }
```

with

```json
  "scripts": {
    "corpus": "sh scripts/corpus.sh",
    "test:packaging": "node --test npm/cli/test/*.test.js scripts/test/*.test.mjs"
  }
```

Run: `node --test npm/cli/test/*.test.js`
Expected: PASS, 9 tests. (`npm run test:packaging` needs `scripts/test/` from Task 2 before it can pass.)

- [ ] **Step 13: Commit**

```bash
git add scripts/targets.mjs npm/cli package.json
git commit -m "feat(npm): the @go-vernier/cli launcher package

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Generate the six npm packages for a release

**Files:**
- Create: `scripts/npm-packages.mjs`
- Test: `scripts/test/npm-packages.test.mjs`

**Interfaces:**
- Consumes: `TARGETS`, `npmDir`, `npmName`, `binaryName` from `scripts/targets.mjs`; `PACKAGES` from `npm/cli/lib/platform.js`; `npm/cli/` (Task 1).
- Produces:
  - `normalizeVersion(version: string) -> string` — strips a leading `v`; throws `"<input>" is not a release version` unless `MAJOR.MINOR.PATCH[-PRERELEASE]`.
  - `writePackages(version: string, binsDir: string, outDir: string) -> string[]` — returns `[<outDir>/cli, <outDir>/cli-<os>-<cpu> ×5]`.
  - CLI: `node scripts/npm-packages.mjs <version> <bins-dir> <out-dir>`; reads `<bins-dir>/bin-<triple>/vernier[.exe]`; prints one package directory per line.

- [ ] **Step 1: Write the failing test `scripts/test/npm-packages.test.mjs`**

```js
import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { TARGETS, binaryName, npmDir, npmName } from "../targets.mjs";
import { normalizeVersion, writePackages } from "../npm-packages.mjs";

const require = createRequire(import.meta.url);
const { PACKAGES } = require("../../npm/cli/lib/platform.js");

const temp = (prefix) => mkdtempSync(path.join(tmpdir(), prefix));
const readJson = (file) => JSON.parse(readFileSync(file, "utf8"));

// What the release build uploads: bin-<triple>/vernier[.exe] per target.
function fakeBins() {
  const dir = temp("vernier-bins-");
  for (const t of TARGETS) {
    mkdirSync(path.join(dir, `bin-${t.triple}`));
    writeFileSync(path.join(dir, `bin-${t.triple}`, binaryName(t)), `binary for ${t.triple}`);
  }
  return dir;
}

test("the launcher's platform table names every release target", () => {
  assert.deepEqual(Object.values(PACKAGES).sort(), TARGETS.map(npmName).sort());
});

test("versions lose a leading v and must be release versions", () => {
  assert.equal(normalizeVersion("v0.1.0"), "0.1.0");
  assert.equal(normalizeVersion("0.1.0"), "0.1.0");
  assert.equal(normalizeVersion("1.2.3-rc.1"), "1.2.3-rc.1");
  for (const bad of ["", "0.1", "latest", "v0.1.0 ", "0.1.0\n", "01.2"]) {
    assert.throws(() => normalizeVersion(bad), /is not a release version/, JSON.stringify(bad));
  }
});

test("the main package is stamped with the version everywhere", () => {
  const out = path.join(temp("vernier-npm-"), "out");
  writePackages("v0.1.0", fakeBins(), out);
  const main = readJson(path.join(out, "cli", "package.json"));
  assert.equal(main.name, "@go-vernier/cli");
  assert.equal(main.version, "0.1.0");
  assert.deepEqual(Object.keys(main.optionalDependencies).sort(), TARGETS.map(npmName).sort());
  for (const version of Object.values(main.optionalDependencies)) assert.equal(version, "0.1.0");
  assert.ok(existsSync(path.join(out, "cli", "bin", "vernier.js")));
  assert.ok(existsSync(path.join(out, "cli", "lib", "platform.js")));
  assert.ok(existsSync(path.join(out, "cli", "README.md")));
  assert.ok(existsSync(path.join(out, "cli", "LICENSE")));
  assert.ok(!existsSync(path.join(out, "cli", "test")), "tests are not published");
});

test("each platform package carries its executable binary and nothing to run at install", () => {
  const out = path.join(temp("vernier-npm-"), "out");
  writePackages("0.1.0", fakeBins(), out);
  for (const t of TARGETS) {
    const dir = path.join(out, npmDir(t));
    const pkg = readJson(path.join(dir, "package.json"));
    assert.equal(pkg.name, npmName(t));
    assert.equal(pkg.version, "0.1.0");
    assert.deepEqual(pkg.os, [t.os]);
    assert.deepEqual(pkg.cpu, [t.cpu]);
    assert.equal(pkg.libc, undefined, "the static musl binary runs on any libc");
    assert.equal(pkg.scripts, undefined);
    assert.equal(pkg.bin, undefined);
    assert.equal(pkg.repository.url, "git+https://github.com/Go-Vernier/Vernier-OSS.git");
    const bin = path.join(dir, "bin", binaryName(t));
    assert.equal(readFileSync(bin, "utf8"), `binary for ${t.triple}`);
    if (process.platform !== "win32") assert.equal(statSync(bin).mode & 0o777, 0o755);
    assert.ok(existsSync(path.join(dir, "LICENSE")));
    assert.ok(existsSync(path.join(dir, "README.md")));
  }
});

test("a missing binary is an error, not a smaller release", () => {
  const bins = fakeBins();
  rmSync(path.join(bins, "bin-x86_64-pc-windows-msvc"), { recursive: true });
  assert.throws(
    () => writePackages("0.1.0", bins, path.join(temp("vernier-npm-"), "out")),
    /no binary for x86_64-pc-windows-msvc/,
  );
});

test("a non-empty output directory is refused, never cleared", () => {
  const out = temp("vernier-npm-");
  writeFileSync(path.join(out, "keep.txt"), "mine");
  assert.throws(() => writePackages("0.1.0", fakeBins(), out), /must be empty/);
  assert.equal(readFileSync(path.join(out, "keep.txt"), "utf8"), "mine");
});
```

- [ ] **Step 2: Run it to see it fail**

Run: `node --test scripts/test/npm-packages.test.mjs`
Expected: FAIL with `Cannot find module '.../scripts/npm-packages.mjs'`.

- [ ] **Step 3: Write `scripts/npm-packages.mjs`**

```js
// Writes the npm packages for one release into <out-dir>:
//   cli/               @go-vernier/cli, the launcher, with the version stamped in
//   cli-<os>-<cpu>/    one per target, holding that target's binary
//
//   node scripts/npm-packages.mjs <version> <bins-dir> <out-dir>
//
// <bins-dir>/bin-<triple>/vernier[.exe] is what the release build uploads.

import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { TARGETS, binaryName, npmDir, npmName } from "./targets.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MAIN = path.join(ROOT, "npm", "cli");
const LICENSE = path.join(ROOT, "LICENSE");
const REPOSITORY = { type: "git", url: "git+https://github.com/Go-Vernier/Vernier-OSS.git" };

export function normalizeVersion(version) {
  const v = String(version).replace(/^v/, "");
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?$/.test(v)) {
    throw new Error(`"${version}" is not a release version`);
  }
  return v;
}

const writeJson = (file, value) => writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);

export function writePackages(version, binsDir, outDir) {
  const v = normalizeVersion(version);
  const binaries = TARGETS.map((t) => {
    const file = path.join(binsDir, `bin-${t.triple}`, binaryName(t));
    if (!existsSync(file)) throw new Error(`no binary for ${t.triple} at ${file}`);
    return { t, file };
  });
  if (existsSync(outDir) && readdirSync(outDir).length > 0) {
    throw new Error(`${outDir} must be empty`);
  }
  mkdirSync(outDir, { recursive: true });

  const main = path.join(outDir, "cli");
  cpSync(MAIN, main, {
    recursive: true,
    filter: (src) => !["test", "node_modules"].includes(path.relative(MAIN, src).split(path.sep)[0]),
  });
  copyFileSync(LICENSE, path.join(main, "LICENSE"));
  const manifest = JSON.parse(readFileSync(path.join(main, "package.json"), "utf8"));
  manifest.version = v;
  manifest.optionalDependencies = Object.fromEntries(TARGETS.map((t) => [npmName(t), v]));
  writeJson(path.join(main, "package.json"), manifest);

  const dirs = [main];
  for (const { t, file } of binaries) {
    const dir = path.join(outDir, npmDir(t));
    mkdirSync(path.join(dir, "bin"), { recursive: true });
    const bin = path.join(dir, "bin", binaryName(t));
    copyFileSync(file, bin);
    chmodSync(bin, 0o755);
    copyFileSync(LICENSE, path.join(dir, "LICENSE"));
    writeFileSync(
      path.join(dir, "README.md"),
      `# ${npmName(t)}\n\nThe \`vernier\` binary for ${t.os} ${t.cpu}. Install ` +
        "[`@go-vernier/cli`](https://www.npmjs.com/package/@go-vernier/cli) instead; " +
        "it picks the right binary for you.\n",
    );
    writeJson(path.join(dir, "package.json"), {
      name: npmName(t),
      version: v,
      description: `The vernier binary for ${t.os} ${t.cpu}. Install @go-vernier/cli instead.`,
      license: "MIT",
      repository: REPOSITORY,
      os: [t.os],
      cpu: [t.cpu],
      files: ["bin"],
      preferUnplugged: true,
    });
    dirs.push(dir);
  }
  return dirs;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [version, binsDir, outDir] = process.argv.slice(2);
  if (!outDir) {
    console.error("usage: node scripts/npm-packages.mjs <version> <bins-dir> <out-dir>");
    process.exit(2);
  }
  try {
    for (const dir of writePackages(version, binsDir, outDir)) console.log(dir);
  } catch (error) {
    console.error(`npm-packages: ${error.message}`);
    process.exit(1);
  }
}
```

- [ ] **Step 4: Run the test to see it pass**

Run: `node --test scripts/test/npm-packages.test.mjs`
Expected: PASS, 6 tests.

- [ ] **Step 5: Check `npm pack` keeps the binary executable**

```bash
tmp=$(mktemp -d)
mkdir -p "$tmp/bins"
for t in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl; do
  mkdir -p "$tmp/bins/bin-$t" && printf '#!/bin/sh\necho hi\n' > "$tmp/bins/bin-$t/vernier"
done
mkdir -p "$tmp/bins/bin-x86_64-pc-windows-msvc" && echo x > "$tmp/bins/bin-x86_64-pc-windows-msvc/vernier.exe"
node scripts/npm-packages.mjs 0.1.0 "$tmp/bins" "$tmp/out"
(cd "$tmp" && npm pack "$tmp/out/cli-linux-x64" >/dev/null && tar -tvzf go-vernier-cli-linux-x64-0.1.0.tgz)
```

Expected: the listing shows `package/bin/vernier` with mode `-rwxr-xr-x`. If it does not, stop: the launcher would need to restore the mode, which is a design change.

- [ ] **Step 6: Commit**

```bash
git add scripts/npm-packages.mjs scripts/test/npm-packages.test.mjs
git commit -m "feat(npm): generate the main and platform packages for a release

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Release version and notes (`scripts/release.mjs`)

**Files:**
- Create: `scripts/release.mjs`
- Test: `scripts/test/release.test.mjs`

**Interfaces:**
- Produces:
  - `workspaceVersion(cargoToml: string) -> string` — throws `Cargo.toml has no [workspace.package] version`.
  - `releaseNotes(changelog: string, version: string) -> string` — the section body, trimmed, ending in one `\n`; throws `CHANGELOG.md has no "## [<version>]" section` or `CHANGELOG.md's <version> section is empty`.
  - CLI, run from the repository root:
    - `node scripts/release.mjs version [tag]` — prints the version; fails if `tag` is given and is not `v<version>`, or if `CHANGELOG.md` has no section for it.
    - `node scripts/release.mjs notes <version>` — prints the notes.
    - Errors print `release: <message>` and exit 1.

- [ ] **Step 1: Write the failing test `scripts/test/release.test.mjs`**

```js
import test from "node:test";
import assert from "node:assert/strict";
import { releaseNotes, workspaceVersion } from "../release.mjs";

const CHANGELOG = `# Changelog

Notes for every release.

## [Unreleased]

- next thing

## [0.2.0] - 2026-11-01

### Added

- two

## [0.1.0] - 2026-10-04

- one

[0.2.0]: https://github.com/Go-Vernier/Vernier-OSS/releases/tag/v0.2.0
[0.1.0]: https://github.com/Go-Vernier/Vernier-OSS/releases/tag/v0.1.0
`;

test("a section ends at the next version heading", () => {
  assert.equal(releaseNotes(CHANGELOG, "0.2.0"), "### Added\n\n- two\n");
});

test("the last section ends before the link definitions", () => {
  assert.equal(releaseNotes(CHANGELOG, "0.1.0"), "- one\n");
});

test("a version with no section fails", () => {
  assert.throws(() => releaseNotes(CHANGELOG, "0.3.0"), /no "## \[0\.3\.0\]" section/);
});

test("a prerelease section does not stand in for the release", () => {
  const changelog = "# Changelog\n\n## [0.3.0-rc.1] - 2026-12-01\n\n- rc\n";
  assert.throws(() => releaseNotes(changelog, "0.3.0"), /no "## \[0\.3\.0\]" section/);
  assert.equal(releaseNotes(changelog, "0.3.0-rc.1"), "- rc\n");
});

test("an empty section fails", () => {
  assert.throws(() => releaseNotes("## [0.4.0] - 2027-01-01\n\n## [0.3.0]\n\n- x\n", "0.4.0"), /0\.4\.0 section is empty/);
});

test("CRLF line endings read the same", () => {
  assert.equal(releaseNotes(CHANGELOG.replaceAll("\n", "\r\n"), "0.1.0"), "- one\n");
});

test("the version comes from [workspace.package], not a dependency", () => {
  const cargo = `[workspace]
members = ["a"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
serde = { version = "1" }
`;
  assert.equal(workspaceVersion(cargo), "0.1.0");
  assert.throws(() => workspaceVersion("[workspace]\nmembers = []\n"), /no \[workspace\.package\] version/);
});
```

- [ ] **Step 2: Run it to see it fail**

Run: `node --test scripts/test/release.test.mjs`
Expected: FAIL with `Cannot find module '.../scripts/release.mjs'`.

- [ ] **Step 3: Write `scripts/release.mjs`**

```js
// The release version and its notes.
//
//   node scripts/release.mjs version [tag]   the [workspace.package] version in
//                                            Cargo.toml; fails if tag is not
//                                            v<version> or CHANGELOG.md has no
//                                            section for it
//   node scripts/release.mjs notes <version> that version's CHANGELOG.md section
//
// Run from the repository root.

import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export function workspaceVersion(cargoToml) {
  const section = cargoToml.split(/^\[/m).find((s) => s.startsWith("workspace.package]"));
  const match = section?.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error("Cargo.toml has no [workspace.package] version");
  return match[1];
}

export function releaseNotes(changelog, version) {
  const lines = changelog.split(/\r?\n/);
  const start = lines.findIndex((line) => line.startsWith(`## [${version}]`));
  if (start === -1) throw new Error(`CHANGELOG.md has no "## [${version}]" section`);
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((line) => line.startsWith("## ") || /^\[[^\]]+\]: /.test(line));
  const notes = (end === -1 ? rest : rest.slice(0, end)).join("\n").trim();
  if (!notes) throw new Error(`CHANGELOG.md's ${version} section is empty`);
  return `${notes}\n`;
}

function main([command, arg]) {
  const changelog = () => readFileSync("CHANGELOG.md", "utf8");
  if (command === "version") {
    const version = workspaceVersion(readFileSync("Cargo.toml", "utf8"));
    if (arg !== undefined && arg !== `v${version}`) {
      throw new Error(`tag ${arg} does not match the Cargo.toml version ${version}`);
    }
    releaseNotes(changelog(), version);
    process.stdout.write(`${version}\n`);
  } else if (command === "notes" && arg) {
    process.stdout.write(releaseNotes(changelog(), arg));
  } else {
    throw new Error("usage: node scripts/release.mjs version [tag] | notes <version>");
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(`release: ${error.message}`);
    process.exit(1);
  }
}
```

- [ ] **Step 4: Run the test to see it pass**

Run: `node --test scripts/test/release.test.mjs`
Expected: PASS, 7 tests.

- [ ] **Step 5: Commit**

```bash
git add scripts/release.mjs scripts/test/release.test.mjs
git commit -m "feat(release): read the release version and its changelog notes

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: The Homebrew formula (`scripts/homebrew-formula.mjs`)

**Files:**
- Create: `scripts/homebrew-formula.mjs`
- Test: `scripts/test/homebrew-formula.test.mjs`

**Interfaces:**
- Consumes: `TARGETS`, `assetName` from `scripts/targets.mjs`; `normalizeVersion` from `scripts/npm-packages.mjs`.
- Produces:
  - `readSha256(distDir: string, target) -> string` — the lowercase hash from `<distDir>/<assetName>.sha256`; throws `<file> does not hold a SHA-256`.
  - `formula(version: string, sha256Of: (target) => string) -> string` — the full `Formula/vernier.rb` text.
  - CLI: `node scripts/homebrew-formula.mjs <version> <dist-dir>` prints the formula.

- [ ] **Step 1: Write the failing test `scripts/test/homebrew-formula.test.mjs`**

```js
import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { TARGETS } from "../targets.mjs";
import { formula, readSha256 } from "../homebrew-formula.mjs";

const SUMS = {
  "aarch64-apple-darwin": "a".repeat(64),
  "x86_64-apple-darwin": "b".repeat(64),
  "aarch64-unknown-linux-musl": "c".repeat(64),
  "x86_64-unknown-linux-musl": "d".repeat(64),
};
const DL = "https://github.com/Go-Vernier/Vernier-OSS/releases/download/v0.1.0";

test("the formula names each macOS and Linux asset with its checksum", () => {
  assert.equal(
    formula("v0.1.0", (t) => SUMS[t.triple]),
    `# Generated by scripts/homebrew-formula.mjs in Go-Vernier/Vernier-OSS. Do not edit by hand.
class Vernier < Formula
  desc "Blast radius of a pull request across services, from code and traces"
  homepage "https://github.com/Go-Vernier/Vernier-OSS"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "${DL}/vernier-aarch64-apple-darwin.tar.gz"
      sha256 "${"a".repeat(64)}"
    end
    on_intel do
      url "${DL}/vernier-x86_64-apple-darwin.tar.gz"
      sha256 "${"b".repeat(64)}"
    end
  end

  on_linux do
    on_arm do
      url "${DL}/vernier-aarch64-unknown-linux-musl.tar.gz"
      sha256 "${"c".repeat(64)}"
    end
    on_intel do
      url "${DL}/vernier-x86_64-unknown-linux-musl.tar.gz"
      sha256 "${"d".repeat(64)}"
    end
  end

  def install
    bin.install "vernier"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/vernier --version")
  end
end
`,
  );
});

test("a checksum is read from sha256sum output and lowercased", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "vernier-dist-"));
  const target = TARGETS.find((t) => t.triple === "aarch64-apple-darwin");
  writeFileSync(path.join(dir, "vernier-aarch64-apple-darwin.tar.gz.sha256"), `${"AB".repeat(32)}  vernier-aarch64-apple-darwin.tar.gz\n`);
  assert.equal(readSha256(dir, target), "ab".repeat(32));
});

test("a file without a checksum is refused", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "vernier-dist-"));
  const target = TARGETS.find((t) => t.triple === "aarch64-apple-darwin");
  writeFileSync(path.join(dir, "vernier-aarch64-apple-darwin.tar.gz.sha256"), "Not Found\n");
  assert.throws(() => readSha256(dir, target), /does not hold a SHA-256/);
});
```

- [ ] **Step 2: Run it to see it fail**

Run: `node --test scripts/test/homebrew-formula.test.mjs`
Expected: FAIL with `Cannot find module '.../scripts/homebrew-formula.mjs'`.

- [ ] **Step 3: Write `scripts/homebrew-formula.mjs`**

```js
// Prints the Homebrew formula for one release, for Go-Vernier/homebrew-tap.
//
//   node scripts/homebrew-formula.mjs <version> <dist-dir>
//
// <dist-dir> holds the release archives' .sha256 files.

import { readFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { normalizeVersion } from "./npm-packages.mjs";
import { TARGETS, assetName } from "./targets.mjs";

const REPO = "https://github.com/Go-Vernier/Vernier-OSS";

export function readSha256(distDir, target) {
  const file = path.join(distDir, `${assetName(target)}.sha256`);
  const hash = readFileSync(file, "utf8").trim().split(/\s+/)[0].toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(hash)) throw new Error(`${file} does not hold a SHA-256`);
  return hash;
}

export function formula(version, sha256Of) {
  const v = normalizeVersion(version);
  const block = (os, cpu) => {
    const t = TARGETS.find((x) => x.os === os && x.cpu === cpu);
    return [
      `      url "${REPO}/releases/download/v${v}/${assetName(t)}"`,
      `      sha256 "${sha256Of(t)}"`,
    ].join("\n");
  };
  return `# Generated by scripts/homebrew-formula.mjs in Go-Vernier/Vernier-OSS. Do not edit by hand.
class Vernier < Formula
  desc "Blast radius of a pull request across services, from code and traces"
  homepage "${REPO}"
  version "${v}"
  license "MIT"

  on_macos do
    on_arm do
${block("darwin", "arm64")}
    end
    on_intel do
${block("darwin", "x64")}
    end
  end

  on_linux do
    on_arm do
${block("linux", "arm64")}
    end
    on_intel do
${block("linux", "x64")}
    end
  end

  def install
    bin.install "vernier"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/vernier --version")
  end
end
`;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [version, distDir] = process.argv.slice(2);
  if (!distDir) {
    console.error("usage: node scripts/homebrew-formula.mjs <version> <dist-dir>");
    process.exit(2);
  }
  try {
    process.stdout.write(formula(version, (t) => readSha256(distDir, t)));
  } catch (error) {
    console.error(`homebrew-formula: ${error.message}`);
    process.exit(1);
  }
}
```

- [ ] **Step 4: Run the test to see it pass**

Run: `node --test scripts/test/homebrew-formula.test.mjs`
Expected: PASS, 3 tests.

- [ ] **Step 5: Check the Ruby syntax**

```bash
node -e 'import("./scripts/homebrew-formula.mjs").then(m => process.stdout.write(m.formula("0.1.0", () => "a".repeat(64))))' > /tmp/vernier.rb
ruby -c /tmp/vernier.rb
```

Expected: `Syntax OK`.

- [ ] **Step 6: Commit**

```bash
git add scripts/homebrew-formula.mjs scripts/test/homebrew-formula.test.mjs
git commit -m "feat(release): generate the Homebrew formula

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: The macOS and Linux installer (`scripts/install.sh`)

**Files:**
- Create: `scripts/install.sh`
- Test: `scripts/test/install.test.mjs`

**Interfaces:**
- Consumes: `TARGETS`, `assetName` from `scripts/targets.mjs` (test only); the release asset layout from Global Constraints.
- Produces: `install.sh` honouring `VERNIER_VERSION`, `VERNIER_INSTALL_DIR`, `VERNIER_BASE_URL`; prints `Installed vernier <version> to <dir>/vernier`; on failure prints `vernier install: <reason>` to stderr and exits 1.

- [ ] **Step 1: Write the failing test `scripts/test/install.test.mjs`**

```js
import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { TARGETS, assetName } from "../targets.mjs";

const SCRIPT = fileURLToPath(new URL("../install.sh", import.meta.url));
const target = TARGETS.find((t) => t.os === process.platform && t.cpu === process.arch);
const skip = target && process.platform !== "win32" ? false : "install.sh runs on macOS and Linux";

const temp = (prefix) => mkdtempSync(path.join(tmpdir(), prefix));

// A release directory, as a file:// URL, holding a fake vernier for this
// machine. `tamper` corrupts the archive after its checksum is written.
function release({ version = "9.9.9", tamper = false } = {}) {
  const root = temp("vernier-release-");
  const stage = path.join(root, "stage");
  const dist = path.join(root, "dist");
  mkdirSync(stage);
  mkdirSync(dist);
  writeFileSync(path.join(stage, "vernier"), `#!/bin/sh\necho "vernier ${version}"\n`, { mode: 0o755 });
  writeFileSync(path.join(stage, "LICENSE"), "MIT\n");
  writeFileSync(path.join(stage, "README.md"), "# Vernier\n");
  const asset = assetName(target);
  execFileSync("tar", ["-czf", path.join(dist, asset), "-C", stage, "vernier", "LICENSE", "README.md"]);
  const hash = createHash("sha256").update(readFileSync(path.join(dist, asset))).digest("hex");
  writeFileSync(path.join(dist, `${asset}.sha256`), `${hash}  ${asset}\n`);
  if (tamper) writeFileSync(path.join(dist, asset), "not the archive");
  return pathToFileURL(dist).href;
}

function install(baseUrl, { dir = path.join(temp("vernier-home-"), "bin"), env = {} } = {}) {
  const out = spawnSync("sh", [SCRIPT], {
    encoding: "utf8",
    env: { ...process.env, VERNIER_BASE_URL: baseUrl, VERNIER_INSTALL_DIR: dir, ...env },
  });
  return { ...out, dir, bin: path.join(dir, "vernier") };
}

test("installs the binary and reports its version", { skip }, () => {
  const out = install(release());
  assert.equal(out.status, 0, out.stderr);
  assert.match(out.stdout, /Installed vernier 9\.9\.9 to .*\/vernier/);
  assert.equal(statSync(out.bin).mode & 0o777, 0o755);
  assert.ok(!existsSync(path.join(out.dir, "LICENSE")), "only the binary is installed");
});

test("says how to put the directory on PATH when it is not", { skip }, () => {
  const out = install(release());
  assert.match(out.stdout, /is not on your PATH/);
  assert.match(out.stdout, /export PATH=/);
});

test("says nothing about PATH when the directory is on it", { skip }, () => {
  const dir = path.join(temp("vernier-home-"), "bin");
  const out = install(release(), { dir, env: { PATH: `${dir}:${process.env.PATH}` } });
  assert.equal(out.status, 0, out.stderr);
  assert.doesNotMatch(out.stdout, /is not on your PATH/);
});

test("upgrade: installing again replaces the binary", { skip }, () => {
  const first = install(release({ version: "1.0.0" }));
  assert.equal(first.status, 0, first.stderr);
  const second = install(release({ version: "2.0.0" }), { dir: first.dir });
  assert.equal(second.status, 0, second.stderr);
  assert.equal(execFileSync(first.bin, ["--version"], { encoding: "utf8" }).trim(), "vernier 2.0.0");
});

test("checksum: a corrupted download installs nothing", { skip }, () => {
  const out = install(release({ tamper: true }));
  assert.equal(out.status, 1);
  assert.match(out.stderr, /vernier install: checksum mismatch/);
  assert.ok(!existsSync(out.bin));
});

test("missing release: a download that fails installs nothing", { skip }, () => {
  const empty = pathToFileURL(temp("vernier-empty-")).href;
  const out = install(empty);
  assert.equal(out.status, 1);
  assert.match(out.stderr, /vernier install: could not download/);
  assert.ok(!existsSync(out.bin));
});
```

- [ ] **Step 2: Run it to see it fail**

Run: `node --test scripts/test/install.test.mjs`
Expected: FAIL — `sh: .../scripts/install.sh: No such file or directory` (status 127, not 0).

- [ ] **Step 3: Write `scripts/install.sh`**

```sh
#!/bin/sh
# Installs vernier from a GitHub release on macOS or Linux.
#
#   curl -fsSL https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.sh | sh
#
# VERNIER_VERSION      a release such as v0.1.0 (default: the latest)
# VERNIER_INSTALL_DIR  where to put the binary (default: ~/.local/bin)
set -eu

REPO="https://github.com/Go-Vernier/Vernier-OSS"

say() { printf '%s\n' "$*"; }
die() {
  printf 'vernier install: %s\n' "$*" >&2
  exit 1
}

unsupported() { die "there is no prebuilt binary for $(uname -s) $(uname -m); build from source: $REPO"; }
case "$(uname -s)" in
  Darwin) os=apple-darwin ;;
  Linux) os=unknown-linux-musl ;;
  *) unsupported ;;
esac
case "$(uname -m)" in
  arm64 | aarch64) arch=aarch64 ;;
  x86_64 | amd64) arch=x86_64 ;;
  *) unsupported ;;
esac
asset="vernier-$arch-$os.tar.gz"

if [ -n "${VERNIER_BASE_URL:-}" ]; then
  base="$VERNIER_BASE_URL"
elif [ -n "${VERNIER_VERSION:-}" ]; then
  base="$REPO/releases/download/v${VERNIER_VERSION#v}"
else
  base="$REPO/releases/latest/download"
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO "$2" "$1"; }
else
  die "needs curl or wget to download"
fi
if command -v sha256sum >/dev/null 2>&1; then
  check() { sha256sum -c "$1" >/dev/null 2>&1; }
elif command -v shasum >/dev/null 2>&1; then
  check() { shasum -a 256 -c "$1" >/dev/null 2>&1; }
else
  die "needs sha256sum or shasum to verify the download"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "Downloading $asset"
fetch "$base/$asset" "$tmp/$asset" || die "could not download $base/$asset"
fetch "$base/$asset.sha256" "$tmp/$asset.sha256" || die "could not download $base/$asset.sha256"
(cd "$tmp" && check "$asset.sha256") || die "checksum mismatch for $asset; nothing was installed"
tar -xzf "$tmp/$asset" -C "$tmp" vernier || die "$asset has no vernier binary"

dir="${VERNIER_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$dir"
cp "$tmp/vernier" "$dir/.vernier.new"
chmod 755 "$dir/.vernier.new"
mv -f "$dir/.vernier.new" "$dir/vernier"

say "Installed $("$dir/vernier" --version) to $dir/vernier"
case ":$PATH:" in
  *":$dir:"*) ;;
  *)
    say ""
    say "$dir is not on your PATH. Add it to your shell profile:"
    say "  export PATH=\"$dir:\$PATH\""
    ;;
esac
```

Then: `chmod +x scripts/install.sh`.

- [ ] **Step 4: Run the test to see it pass**

Run: `node --test scripts/test/install.test.mjs`
Expected: PASS, 6 tests.

- [ ] **Step 5: Run the whole Node suite**

Run: `npm run test:packaging`
Expected: PASS, all tests from Tasks 1–5.

- [ ] **Step 6: Commit**

```bash
git add scripts/install.sh scripts/test/install.test.mjs
git commit -m "feat(release): curl | sh installer for macOS and Linux

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: The Windows installer (`scripts/install.ps1`)

**Files:**
- Create: `scripts/install.ps1`

**Interfaces:**
- Consumes: the release asset layout from Global Constraints (`vernier-x86_64-pc-windows-msvc.zip` and its `.sha256`).
- Produces: `install.ps1` honouring `VERNIER_VERSION`, `VERNIER_INSTALL_DIR`, `VERNIER_BASE_URL`; prints `Installed vernier <version> to <dir>\vernier.exe`; throws `vernier install: <reason>` on failure. Exercised by the release smoke job (Task 9) under PowerShell 7 and 5.1 and parsed by CI (Task 7); this machine has no PowerShell.

- [ ] **Step 1: Write `scripts/install.ps1`**

```powershell
# Installs vernier from a GitHub release on Windows.
#
#   irm https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.ps1 | iex
#
# $env:VERNIER_VERSION      a release such as v0.1.0 (default: the latest)
# $env:VERNIER_INSTALL_DIR  where to put vernier.exe
#                           (default: %LOCALAPPDATA%\Programs\vernier\bin)

# A script block, so `irm | iex` leaves nothing behind in the caller's session.
& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    # Windows PowerShell 5.1 on older Windows does not offer TLS 1.2 by default.
    [Net.ServicePointManager]::SecurityProtocol =
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'https://github.com/Go-Vernier/Vernier-OSS'
    $asset = 'vernier-x86_64-pc-windows-msvc.zip'

    # x64 binary; Windows on ARM runs it under emulation.
    $cpu = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    if ($cpu -ne 'AMD64' -and $cpu -ne 'ARM64') {
        throw "vernier install: there is no prebuilt binary for $cpu Windows; build from source: $repo"
    }

    $base = if ($env:VERNIER_BASE_URL) {
        $env:VERNIER_BASE_URL
    } elseif ($env:VERNIER_VERSION) {
        "$repo/releases/download/v$($env:VERNIER_VERSION.TrimStart('v'))"
    } else {
        "$repo/releases/latest/download"
    }
    $dir = if ($env:VERNIER_INSTALL_DIR) {
        $env:VERNIER_INSTALL_DIR
    } else {
        Join-Path $env:LOCALAPPDATA 'Programs\vernier\bin'
    }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("vernier-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $zip = Join-Path $tmp $asset
        Write-Host "Downloading $asset"
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $zip
            Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset.sha256" -OutFile "$zip.sha256"
        } catch {
            throw "vernier install: could not download $base/${asset}: $($_.Exception.Message)"
        }
        $expected = ((Get-Content "$zip.sha256" -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
        $actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
        if ($expected -ne $actual) {
            throw "vernier install: checksum mismatch for $asset; nothing was installed"
        }
        Expand-Archive -Path $zip -DestinationPath (Join-Path $tmp 'x') -Force
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        Copy-Item -Force (Join-Path $tmp 'x\vernier.exe') (Join-Path $dir 'vernier.exe')
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @(if ($userPath) { $userPath -split ';' | Where-Object { $_ } })
    if ($entries -notcontains $dir) {
        [Environment]::SetEnvironmentVariable('Path', (@($entries) + $dir) -join ';', 'User')
        Write-Host "Added $dir to your user PATH. New terminals will find vernier."
    }
    if (($env:Path -split ';') -notcontains $dir) {
        $env:Path = "$env:Path;$dir"
    }

    $version = & (Join-Path $dir 'vernier.exe') --version
    Write-Host "Installed $version to $(Join-Path $dir 'vernier.exe')"
}
```

- [ ] **Step 2: Review the script against the spec by reading it once more**

Check, line by line: user `PATH` only (never `'Machine'`); every failure throws a message starting `vernier install:`; the temporary directory is removed in `finally`; `VERNIER_BASE_URL` takes precedence over `VERNIER_VERSION`. There is no local PowerShell; CI parses it (Task 7) and runs it (Task 9).

- [ ] **Step 3: Commit**

```bash
git add scripts/install.ps1
git commit -m "feat(release): PowerShell installer for Windows

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: CI on Windows, and a packaging job

**Files:**
- Create: `.gitattributes`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `npm run test:packaging` (Task 1), `scripts/install.sh` (Task 5), `scripts/install.ps1` (Task 6).
- Produces: CI jobs `test` (ubuntu, macos, windows) and `packaging`.

- [ ] **Step 1: Check no committed file is CRLF today**

Run: `git ls-files --eol | awk '$1 != "i/lf" && $1 != "i/none" && $1 != "i/-text"'`
Expected: no output. If any file is listed, it needs `git add --renormalize .` in Step 3's commit; note which files in the commit message.

- [ ] **Step 2: Write `.gitattributes`**

```gitattributes
# LF everywhere, so Windows checkouts keep the fixtures and expected output
# in test/ byte-identical. PowerShell scripts keep CRLF for Windows PowerShell.
* text=auto eol=lf
*.ps1 text eol=crlf
```

- [ ] **Step 3: Replace `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main, dev]
  pull_request:

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
      - name: Smoke test the CLI on this repository
        run: cargo run -q -- analyze .

  packaging:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - name: Packaging scripts and the npm launcher
        run: npm run test:packaging
      - name: Lint install.sh
        run: shellcheck scripts/install.sh
      - name: Parse install.ps1
        shell: pwsh
        run: |
          $errors = $null
          [System.Management.Automation.Language.Parser]::ParseFile("$PWD/scripts/install.ps1", [ref]$null, [ref]$errors) | Out-Null
          if ($errors) { $errors | ForEach-Object { Write-Error $_.ToString() }; exit 1 }
      - name: Lint the workflows
        run: |
          bash <(curl -fsSL https://raw.githubusercontent.com/rhysd/actionlint/main/scripts/download-actionlint.bash)
          ./actionlint -color
```

- [ ] **Step 4: Check the YAML parses**

Run: `ruby -ryaml -e 'YAML.load_file(".github/workflows/ci.yml"); puts "ok"'`
Expected: `ok`. (The real run happens in Task 11.)

- [ ] **Step 5: Commit**

```bash
git add .gitattributes .github/workflows/ci.yml
git commit -m "ci: test on Windows; lint and test the packaging scripts

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Version 0.1.0 and the changelog

**Files:**
- Modify: `Cargo.toml:6` (`version = "0.0.1"` → `"0.1.0"`)
- Modify: `Cargo.lock` (the `vernier-cli`, `vernier-core`, `vernier-tui` entries)
- Modify: `package.json` (`"version": "0.0.1"` → `"0.1.0"`)
- Modify: `crates/vernier-cli/tests/cli.rs:75-78`
- Create: `CHANGELOG.md`
- Test: `scripts/test/repository.test.mjs`

**Interfaces:**
- Consumes: `workspaceVersion`, `releaseNotes` from `scripts/release.mjs` (Task 3).
- Produces: `CHANGELOG.md` with a `## [0.1.0]` section, which the release `check` job requires.

- [ ] **Step 1: Write the failing test `scripts/test/repository.test.mjs`**

```js
// The repository's own release metadata agrees with itself.
import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { releaseNotes, workspaceVersion } from "../release.mjs";

const ROOT = fileURLToPath(new URL("../..", import.meta.url));
const read = (file) => readFileSync(new URL(`../../${file}`, import.meta.url), "utf8");
const version = workspaceVersion(read("Cargo.toml"));
const release = (...args) =>
  spawnSync(process.execPath, ["scripts/release.mjs", ...args], { cwd: ROOT, encoding: "utf8" });

test("CHANGELOG.md has notes for the Cargo.toml version", () => {
  assert.ok(releaseNotes(read("CHANGELOG.md"), version).length > 0);
});

test("the root package.json carries the same version", () => {
  assert.equal(JSON.parse(read("package.json")).version, version);
});

test("Cargo.lock carries the same version for every vernier crate", () => {
  for (const crate of ["vernier-cli", "vernier-core", "vernier-tui"]) {
    assert.match(read("Cargo.lock"), new RegExp(`name = "${crate}"\\nversion = "${version.replaceAll(".", "\\.")}"`), crate);
  }
});

test("the matching tag passes the release check", () => {
  const out = release("version", `v${version}`);
  assert.equal(out.status, 0, out.stderr);
  assert.equal(out.stdout, `${version}\n`);
});

test("another tag fails the release check", () => {
  const out = release("version", "v99.0.0");
  assert.equal(out.status, 1);
  assert.match(out.stderr, /release: tag v99\.0\.0 does not match the Cargo\.toml version/);
});
```

- [ ] **Step 2: Run it to see it fail**

Run: `node --test scripts/test/repository.test.mjs`
Expected: FAIL — `ENOENT ... CHANGELOG.md` and the version tests fail on `0.0.1`.

- [ ] **Step 3: Bump the version**

```bash
sed -i '' 's/^version = "0.0.1"$/version = "0.1.0"/' Cargo.toml
perl -0pi -e 's/(name = "vernier-(?:cli|core|tui)"\nversion = )"0\.0\.1"/$1"0.1.0"/g' Cargo.lock
sed -i '' 's/"version": "0.0.1"/"version": "0.1.0"/' package.json
git diff --stat
```

Expected: `Cargo.toml` 1 line, `Cargo.lock` 3 lines, `package.json` 1 line.

- [ ] **Step 4: Make the Rust version test follow the crate version**

In `crates/vernier-cli/tests/cli.rs`, replace

```rust
#[test]
fn version_flag() {
    let out = bin().arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("vernier 0.0.1"));
}
```

with

```rust
#[test]
fn version_flag() {
    let out = bin().arg("--version").output().unwrap();
    let expected = concat!("vernier ", env!("CARGO_PKG_VERSION"));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with(expected));
}
```

(CI runs it in Task 11; there is no local `cargo`.)

- [ ] **Step 5: Write `CHANGELOG.md`**

```markdown
# Changelog

Every release of Vernier, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-10-04

The first release. Install with `npx @go-vernier/cli`, `brew install
go-vernier/tap/vernier`, or the shell and PowerShell installers; see the
README.

### Added

- **Service discovery** with no configuration: docker-compose (with `.env`
  resolution), Kubernetes manifests, monorepo layouts (`services/`, `apps/`,
  `packages/`, ...) and workspace config (npm, pnpm, Cargo, Nx). A
  single-service or deploy-only repository is named for what it is.
- **Static edges** between services: HTTP calls, gRPC stubs, message topics
  and typed events, shared databases and cross-package imports, read from
  JavaScript, TypeScript, Python, Go, Java, C# and PHP through tree-sitter,
  and from configuration files. Every edge points to a file and line.
- **Runtime join**: production edges from OpenTelemetry (`--otel`,
  servicegraph scrape or OTLP JSON) and Datadog (`--datadog`), with a report
  of how trace names matched services.
- **Blast radius** of a change: `--pr`, `--diff`, `--files`, and `--history`
  for the last N pull requests, with Observed, Static, Inferred and
  Uncertain confidence labels and `--depth`.
- **Reports**: terminal, `--json`, and a self-contained `--html` report with
  the graph.
- **`vernier tui`**: explore the services, changes and blast radius
  interactively.
- **Prebuilt binaries** for macOS (Apple silicon, Intel), Linux (x64, arm64;
  static, any distribution) and Windows (x64).

[0.1.0]: https://github.com/Go-Vernier/Vernier-OSS/releases/tag/v0.1.0
```

- [ ] **Step 6: Run the test to see it pass**

Run: `node --test scripts/test/repository.test.mjs`
Expected: PASS, 5 tests.

- [ ] **Step 7: Run the whole Node suite**

Run: `npm run test:packaging`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock package.json crates/vernier-cli/tests/cli.rs CHANGELOG.md scripts/test/repository.test.mjs
git commit -m "chore: version 0.1.0 and the changelog

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: The release workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `node scripts/release.mjs version [tag]` and `notes <version>` (Task 3); `node scripts/npm-packages.mjs <version> <bins-dir> <out-dir>` (Task 2); `node scripts/homebrew-formula.mjs <version> <dist-dir>` (Task 4); `scripts/install.sh` (Task 5); `scripts/install.ps1` (Task 6); `npm/cli/lib/platform.js` `packageFor` (Task 1); `CHANGELOG.md` (Task 8).
- Produces: artifacts `dist-<triple>` (archive + `.sha256`) and `bin-<triple>` (bare binary); on a tag: the GitHub Release, six npm packages, the tap formula.

- [ ] **Step 1: Write `.github/workflows/release.yml`**

```yaml
# Builds, tests and publishes a release. Only a v* tag publishes; a pull
# request that touches packaging, or a manual run, is a dry run of the same
# pipeline. See docs/releasing.md.
name: Release

on:
  push:
    tags: ["v*"]
  pull_request:
    paths:
      - .github/workflows/release.yml
      - npm/**
      - scripts/**
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: release-${{ github.ref }}
  cancel-in-progress: false

jobs:
  check:
    runs-on: ubuntu-latest
    outputs:
      version: ${{ steps.version.outputs.version }}
      publish: ${{ steps.version.outputs.publish }}
      prerelease: ${{ steps.version.outputs.prerelease }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - id: version
        run: |
          if [ "$GITHUB_EVENT_NAME" = push ]; then
            version=$(node scripts/release.mjs version "$GITHUB_REF_NAME")
            publish=true
          else
            version=$(node scripts/release.mjs version)
            publish=false
          fi
          case "$version" in *-*) prerelease=true ;; *) prerelease=false ;; esac
          {
            echo "version=$version"
            echo "publish=$publish"
            echo "prerelease=$prerelease"
          } >> "$GITHUB_OUTPUT"
          echo "Release $version (publish: $publish, prerelease: $prerelease)"

  build:
    needs: check
    strategy:
      fail-fast: false
      matrix:
        include:
          - { target: aarch64-apple-darwin, os: macos-latest }
          - { target: x86_64-apple-darwin, os: macos-15-intel }
          - { target: x86_64-unknown-linux-musl, os: ubuntu-latest }
          - { target: aarch64-unknown-linux-musl, os: ubuntu-24.04-arm }
          - { target: x86_64-pc-windows-msvc, os: windows-latest }
    runs-on: ${{ matrix.os }}
    defaults:
      run:
        shell: bash
    env:
      TARGET: ${{ matrix.target }}
      VERSION: ${{ needs.check.outputs.version }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - name: Install the musl toolchain
        if: contains(matrix.target, 'musl')
        run: |
          sudo apt-get update
          sudo apt-get install -y musl-tools
          echo "CC_$(echo "$TARGET" | tr - _)=musl-gcc" >> "$GITHUB_ENV"
      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}
      - run: cargo build --release --locked -p vernier-cli --target "$TARGET"
      - name: Package
        run: |
          exe=vernier
          if [ "$RUNNER_OS" = Windows ]; then exe=vernier.exe; fi
          bin="target/$TARGET/release/$exe"
          "$bin" --version | grep -F "vernier $VERSION"
          "$bin" analyze . > /dev/null
          mkdir -p stage out "bin-$TARGET"
          cp "$bin" LICENSE README.md stage/
          cp "$bin" "bin-$TARGET/"
          if [ "$RUNNER_OS" = Windows ]; then
            asset="vernier-$TARGET.zip"
            (cd stage && 7z a -tzip "../out/$asset" "$exe" LICENSE README.md > /dev/null)
          else
            asset="vernier-$TARGET.tar.gz"
            tar -czf "out/$asset" -C stage "$exe" LICENSE README.md
          fi
          cd out
          if command -v sha256sum > /dev/null; then
            sha256sum "$asset" > "$asset.sha256"
          else
            shasum -a 256 "$asset" > "$asset.sha256"
          fi
          cat "$asset.sha256"
      - uses: actions/upload-artifact@v4
        with:
          name: dist-${{ matrix.target }}
          path: out/
          if-no-files-found: error
      - uses: actions/upload-artifact@v4
        with:
          name: bin-${{ matrix.target }}
          path: bin-${{ matrix.target }}/
          if-no-files-found: error

  smoke:
    needs: [check, build]
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    defaults:
      run:
        shell: bash
    env:
      VERSION: ${{ needs.check.outputs.version }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - uses: actions/download-artifact@v4
        with:
          pattern: bin-*
          path: bins
      - uses: actions/download-artifact@v4
        with:
          pattern: dist-*
          path: dist
          merge-multiple: true
      - name: The npm packages install and run
        run: |
          node scripts/npm-packages.mjs "$VERSION" bins "$RUNNER_TEMP/npm"
          platform=$(node -p "require('./npm/cli/lib/platform').packageFor(process.platform, process.arch).split('/')[1]")
          mkdir "$RUNNER_TEMP/packs" "$RUNNER_TEMP/app"
          (cd "$RUNNER_TEMP/packs" && npm pack "$RUNNER_TEMP/npm/cli" "$RUNNER_TEMP/npm/$platform")
          cd "$RUNNER_TEMP/app"
          npm init -y > /dev/null
          npm install --omit=optional "$RUNNER_TEMP"/packs/*.tgz
          npx --no-install vernier --version | grep -F "vernier $VERSION"
          npx --no-install vernier analyze "$GITHUB_WORKSPACE/test/fixtures/edges-http-app"
      - name: install.sh installs from the build
        if: runner.os != 'Windows'
        run: |
          VERNIER_BASE_URL="file://$PWD/dist" VERNIER_INSTALL_DIR="$RUNNER_TEMP/sh-bin" sh scripts/install.sh
          "$RUNNER_TEMP/sh-bin/vernier" --version | grep -F "vernier $VERSION"
      - name: The Linux binary runs on Alpine
        if: runner.os == 'Linux'
        run: |
          chmod +x bins/bin-x86_64-unknown-linux-musl/vernier
          docker run --rm -v "$PWD/bins/bin-x86_64-unknown-linux-musl:/b:ro" -v "$PWD/test/fixtures:/f:ro" \
            alpine:latest sh -c '/b/vernier --version && /b/vernier analyze /f/edges-http-app > /dev/null'
      - name: install.ps1 installs from the build (PowerShell 7 and 5.1)
        if: runner.os == 'Windows'
        shell: pwsh
        run: |
          $server = Start-Process python -ArgumentList '-m', 'http.server', '8765', '--bind', '127.0.0.1', '--directory', 'dist' -PassThru -WindowStyle Hidden
          Start-Sleep -Seconds 3
          try {
            $env:VERNIER_BASE_URL = 'http://127.0.0.1:8765'
            foreach ($shell in 'pwsh', 'powershell') {
              $env:VERNIER_INSTALL_DIR = "$env:RUNNER_TEMP\ps-bin-$shell"
              & $shell -NoProfile -ExecutionPolicy Bypass -File scripts/install.ps1
              if ($LASTEXITCODE -ne 0) { throw "install.ps1 failed under $shell" }
              $v = & "$env:VERNIER_INSTALL_DIR\vernier.exe" --version
              if ($v -notmatch [regex]::Escape("vernier $env:VERSION")) { throw "$shell installed: $v" }
              Write-Host "$shell -> $v"
            }
          } finally {
            Stop-Process -Id $server.Id -ErrorAction SilentlyContinue
          }

  release:
    needs: [check, smoke]
    if: needs.check.outputs.publish == 'true'
    runs-on: ubuntu-latest
    permissions:
      contents: write
    env:
      VERSION: ${{ needs.check.outputs.version }}
      PRERELEASE: ${{ needs.check.outputs.prerelease }}
      GH_TOKEN: ${{ github.token }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - uses: actions/download-artifact@v4
        with:
          pattern: dist-*
          path: dist
          merge-multiple: true
      - name: Create the GitHub release
        run: |
          (cd dist && sha256sum -- *.tar.gz *.zip > SHA256SUMS && cat SHA256SUMS)
          cp scripts/install.sh scripts/install.ps1 dist/
          node scripts/release.mjs notes "$VERSION" > "$RUNNER_TEMP/notes.md"
          if gh release view "v$VERSION" > /dev/null 2>&1; then
            gh release upload "v$VERSION" dist/* --clobber
          else
            flags=""
            if [ "$PRERELEASE" = true ]; then flags="--prerelease"; fi
            # shellcheck disable=SC2086
            gh release create "v$VERSION" dist/* --verify-tag --title "v$VERSION" \
              --notes-file "$RUNNER_TEMP/notes.md" $flags
          fi

  npm:
    needs: [check, smoke, release]
    if: >-
      !cancelled() && needs.smoke.result == 'success' &&
      (needs.release.result == 'success' || needs.release.result == 'skipped')
    runs-on: ubuntu-latest
    permissions:
      contents: read
      id-token: write
    env:
      VERSION: ${{ needs.check.outputs.version }}
      PUBLISH: ${{ needs.check.outputs.publish }}
      PRERELEASE: ${{ needs.check.outputs.prerelease }}
      NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          registry-url: https://registry.npmjs.org
      - uses: actions/download-artifact@v4
        with:
          pattern: bin-*
          path: bins
      - name: Publish the platform packages, then @go-vernier/cli
        run: |
          node scripts/npm-packages.mjs "$VERSION" bins "$RUNNER_TEMP/npm"
          tag=latest
          if [ "$PRERELEASE" = true ]; then tag=next; fi
          publish() {
            name=$(node -p "require('$1/package.json').name")
            if [ "$PUBLISH" != true ]; then
              npm publish "$1" --access public --tag "$tag" --dry-run
            elif npm view "$name@$VERSION" version > /dev/null 2>&1; then
              echo "$name@$VERSION is already published; skipping"
            else
              npm publish "$1" --access public --tag "$tag" --provenance
            fi
          }
          for dir in "$RUNNER_TEMP"/npm/cli-*; do publish "$dir"; done
          publish "$RUNNER_TEMP/npm/cli"

  homebrew:
    needs: [check, smoke, release]
    if: >-
      !cancelled() && needs.smoke.result == 'success' &&
      needs.check.outputs.prerelease != 'true' &&
      (needs.release.result == 'success' || needs.release.result == 'skipped')
    runs-on: ubuntu-latest
    env:
      VERSION: ${{ needs.check.outputs.version }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - uses: actions/download-artifact@v4
        with:
          pattern: dist-*
          path: dist
          merge-multiple: true
      - name: Write the formula
        run: |
          node scripts/homebrew-formula.mjs "$VERSION" dist > "$RUNNER_TEMP/vernier.rb"
          cat "$RUNNER_TEMP/vernier.rb"
          ruby -c "$RUNNER_TEMP/vernier.rb"
      - name: Commit it to the tap
        if: needs.check.outputs.publish == 'true'
        env:
          TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}
        run: |
          git clone --depth 1 "https://x-access-token:$TAP_TOKEN@github.com/Go-Vernier/homebrew-tap.git" "$RUNNER_TEMP/tap"
          cd "$RUNNER_TEMP/tap"
          mkdir -p Formula
          cp "$RUNNER_TEMP/vernier.rb" Formula/vernier.rb
          git add Formula/vernier.rb
          if git diff --cached --quiet; then
            echo "The formula is already at $VERSION"
            exit 0
          fi
          git -c user.name="github-actions[bot]" \
              -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
              commit -m "vernier $VERSION"
          git push

  verify:
    needs: [check, npm, homebrew]
    if: needs.check.outputs.publish == 'true' && needs.check.outputs.prerelease != 'true'
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    defaults:
      run:
        shell: bash
    env:
      VERSION: ${{ needs.check.outputs.version }}
    steps:
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - name: curl | sh
        if: runner.os != 'Windows'
        run: |
          curl -fsSL https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.sh \
            | VERNIER_INSTALL_DIR="$RUNNER_TEMP/v" sh
          "$RUNNER_TEMP/v/vernier" --version | grep -F "vernier $VERSION"
      - name: irm | iex
        if: runner.os == 'Windows'
        shell: pwsh
        run: |
          irm https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.ps1 | iex
          $v = & "$env:LOCALAPPDATA\Programs\vernier\bin\vernier.exe" --version
          if ($v -notmatch [regex]::Escape("vernier $env:VERSION")) { throw "installed: $v" }
      - name: npx
        run: |
          # The registry can take a minute to serve a new version everywhere.
          for attempt in 1 2 3 4 5; do
            if npx --yes "@go-vernier/cli@$VERSION" --version | grep -F "vernier $VERSION"; then exit 0; fi
            echo "attempt $attempt failed; retrying in 30s"
            sleep 30
          done
          exit 1
      - name: brew
        if: runner.os == 'macOS'
        run: |
          brew install go-vernier/tap/vernier
          vernier --version | grep -F "vernier $VERSION"
```

- [ ] **Step 2: Check the YAML parses**

Run: `ruby -ryaml -e 'YAML.load_file(".github/workflows/release.yml"); puts "ok"'`
Expected: `ok`. (`actionlint` runs in CI from Task 7; the full dry run is Task 11.)

- [ ] **Step 3: Check the job graph by reading it once more**

Confirm against the spec's Release workflow section: `release` needs `smoke` and only runs when `publish == 'true'`; `npm` and `homebrew` run on a dry run (release skipped) and after a successful release; `homebrew` and `verify` skip prereleases; nothing but the `release` job has `contents: write`; only the `npm` job has `id-token: write`.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: release workflow; tag to publish, pull requests dry-run it

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 10: Launch docs and the demo recording

**Files:**
- Modify: `README.md:8-14` (badges), `README.md:26-36` (Quick start → Install + Quick start)
- Modify: `docs/how-it-works.md` (Developing: one line pointing to `docs/releasing.md`)
- Create: `docs/releasing.md`
- Create: `docs/demo.tape`
- Create: `.github/workflows/demo.yml`

**Interfaces:**
- Consumes: the install one-liners and names from Global Constraints; `release.yml` job names (Task 9); `scripts/release.mjs` (Task 3).

- [ ] **Step 1: Replace the Phase 0 badge in `README.md`**

Replace

```markdown
[![Status: Phase 0](https://img.shields.io/badge/status-phase%200-ffb454.svg)](docs/build-spec.md)
```

with

```markdown
[![Release](https://img.shields.io/github/v/release/Go-Vernier/Vernier-OSS?color=ffb454)](https://github.com/Go-Vernier/Vernier-OSS/releases/latest)
[![npm](https://img.shields.io/npm/v/@go-vernier/cli?color=cb3837&logo=npm)](https://www.npmjs.com/package/@go-vernier/cli)
```

- [ ] **Step 2: Replace the Quick start in `README.md`**

Replace from `## Quick start` through the line `Not yet published to npm or crates.io. You need a stable Rust toolchain.` with:

````markdown
## Install

```bash
npx @go-vernier/cli analyze .            # run it once, nothing to install
npm install -g @go-vernier/cli           # npm
brew install go-vernier/tap/vernier      # Homebrew, macOS and Linux
curl -fsSL https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.ps1 | iex
```

Prebuilt for macOS (Apple silicon, Intel), Linux (x64, arm64; any
distribution) and Windows (x64). Every download is checked against its
SHA-256. From source, with a stable Rust toolchain:

```bash
cargo install --git https://github.com/Go-Vernier/Vernier-OSS vernier-cli
```

## Quick start

```bash
vernier analyze /path/to/repo      # the report
vernier tui /path/to/repo          # explore it interactively
```
````

- [ ] **Step 3: Write `docs/releasing.md`**

````markdown
# Releasing Vernier

A release is a `v*` tag on `main`. `.github/workflows/release.yml` does the
rest: it builds five binaries, smoke-tests every way to install, creates the
GitHub Release, publishes to npm and updates the Homebrew tap.

## One-time setup

1. **npm.** Create the organisation `go-vernier` on npmjs.com. Create a
   granular access token with read and write on `@go-vernier/*`, and add it
   to this repository as the Actions secret `NPM_TOKEN`.
2. **Homebrew.** Create the public repository `Go-Vernier/homebrew-tap` with
   a README. Create a fine-grained token with Contents read and write on that
   repository only, and add it here as `HOMEBREW_TAP_TOKEN`.

After the first release, switch npm to trusted publishing: on each of the six
packages' settings pages on npmjs.com, add this repository and `release.yml`
as a trusted publisher, then delete `NPM_TOKEN`.

## Cutting a release

1. On `dev`, set `[workspace.package] version` in `Cargo.toml`, and the
   `version` in the root `package.json`, to the new version. Run `cargo
   build` so `Cargo.lock` follows.
2. Add a `## [x.y.z] - YYYY-MM-DD` section to the top of `CHANGELOG.md`, and
   its link at the bottom. The release notes are this section.
3. Open the pull request. CI runs the tests on Linux, macOS and Windows. If
   the pull request touches packaging, the Release workflow dry-runs too;
   otherwise run it by hand from the Actions tab (Release → Run workflow)
   once it is on `main`.
4. Merge to `main`, then tag:

   ```bash
   git switch main && git pull
   git tag v0.1.0 && git push origin v0.1.0
   ```

5. Watch the Release run. The `verify` job installs the published release
   with every one-liner on clean machines; when it is green, the release is
   done.

A version with a suffix (`0.2.0-rc.1`) is a prerelease: a GitHub prerelease,
the `next` tag on npm, and no Homebrew update.

## When something fails

- **check fails:** the tag does not match `Cargo.toml`, or `CHANGELOG.md`
  has no section for the version. Delete the tag (`git push --delete origin
  vX.Y.Z`), fix, and tag again.
- **build or smoke fails:** nothing has been published. Fix on `dev`, merge,
  move the tag.
- **npm or homebrew fails** after the release exists: fix the cause (a
  secret, usually) and re-run the failed job. Packages already published are
  skipped.
- **A broken release reached users:** npm versions cannot be reused. Publish
  a fix as the next patch version; deprecate the broken one with `npm
  deprecate @go-vernier/cli@X.Y.Z "use X.Y.Z+1"`.

## The README demo

`docs/demo.tape` records `vernier tui` on robot-shop with
[VHS](https://github.com/charmbracelet/vhs). Run the Demo workflow from the
Actions tab, download the `demo` artifact, and commit it as `docs/demo.gif`.
````

- [ ] **Step 4: Point the Developing section at it**

In `docs/how-it-works.md`, after the paragraph that ends `...which the Rust engine
must reproduce.`, add:

```markdown
Releases are cut by tagging; see [Releasing](releasing.md).
```

- [ ] **Step 5: Write `docs/demo.tape`**

```
# The README demo: the blast radius of one file in robot-shop, then the TUI.
# Recorded by .github/workflows/demo.yml, with vernier on PATH and
# instana/robot-shop cloned into ./robot-shop.

Output demo.gif

Set Shell bash
Set FontSize 16
Set Width 1200
Set Height 720
Set Padding 20
Set TypingSpeed 50ms
Set Theme "Catppuccin Mocha"

Hide
Type "export PS1='$ ' && clear"
Enter
Show

Type "vernier analyze robot-shop --files cart/server.js"
Sleep 500ms
Enter
Sleep 4s

Type "vernier tui robot-shop"
Sleep 500ms
Enter
Sleep 2s
Type "2"
Sleep 1.5s
Type "j"
Sleep 400ms
Type "j"
Sleep 800ms
Enter
Sleep 3s
Type "+"
Sleep 2s
Type "q"
Sleep 500ms
```

- [ ] **Step 6: Write `.github/workflows/demo.yml`**

```yaml
# Records the README demo GIF from docs/demo.tape. Run it by hand, download
# the demo artifact, and commit it as docs/demo.gif.
name: Demo

on:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  record:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release --locked -p vernier-cli
      - run: echo "$PWD/target/release" >> "$GITHUB_PATH"
      - run: git clone --depth 1 https://github.com/instana/robot-shop.git robot-shop
      - uses: charmbracelet/vhs-action@v2
        with:
          path: docs/demo.tape
      - uses: actions/upload-artifact@v4
        with:
          name: demo
          path: demo.gif
          if-no-files-found: error
```

- [ ] **Step 7: Check the README renders the install block and the YAML parses**

Run: `grep -n "## Install" -A 6 README.md && ruby -ryaml -e 'YAML.load_file(".github/workflows/demo.yml"); puts "ok"'`
Expected: the install block, then `ok`. Confirm `Not yet published` no longer appears: `grep -c "Not yet published" README.md` prints `0`.

- [ ] **Step 8: Commit**

```bash
git add README.md docs/releasing.md docs/how-it-works.md docs/demo.tape .github/workflows/demo.yml
git commit -m "docs: install instructions, releasing checklist, demo recording

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 11: Prove it in CI (outward-facing: ask first)

**Files:**
- Modify: whatever CI shows is broken; most likely `crates/vernier-core/src/**` for Windows path handling.

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Ask the human partner before pushing**

Pushing the branch and opening a pull request to `dev` is the first outward-facing step. Ask: "Push `release-distribution` and open a PR to `dev` so CI and the release dry run can run?" Wait for yes.

- [ ] **Step 2: Push and open the pull request**

```bash
git push -u origin release-distribution
gh pr create --base dev --title "Release and distribution: one-command installs for v0.1.0" --body "$(cat <<'EOF'
Implements docs/superpowers/specs/2026-10-04-release-distribution-design.md.

- release.yml: five native builds, smoke tests of every channel, GitHub Release, npm (@go-vernier/cli + 5 platform packages), Homebrew tap, post-release verify. Only a v* tag publishes; this PR runs it as a dry run.
- npm/cli: the launcher package; scripts/: package generation, formula, release notes, install.sh, install.ps1, with node:test suites.
- CI: Windows in the test matrix; a packaging job (node tests, shellcheck, PowerShell parse, actionlint).
- Version 0.1.0, CHANGELOG.md, README install section, docs/releasing.md, demo tape.

Before tagging: create the go-vernier npm org + NPM_TOKEN, and Go-Vernier/homebrew-tap + HOMEBREW_TAP_TOKEN (docs/releasing.md).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 3: Watch the runs**

Run: `gh pr checks --watch`
Expected eventually: CI `test` ×3, `packaging`, and Release `check`, `build` ×5, `smoke` ×3, `npm` (dry run) and `homebrew` (formula printed) all pass; `release` and `verify` are skipped.

- [ ] **Step 4: Fix failures, one at a time**

For each failing job: `gh run view <run-id> --log-failed`, find the root cause (use superpowers:systematic-debugging), write or adjust a test that shows it where one can run, fix, commit (`fix: ...`), push, and watch again.

Expected kinds of failure and what to do:
- **Windows `cargo test`**: backslashes in reported paths, or `git` output with CRLF. Normalise paths to `/` at the point they enter the engine (the existing `blast.rs` `normalize` does `replace('\\', "/")`; follow that pattern). If the fix spreads beyond path separators and command invocation, stop and bring it back to the human partner before going on (spec: CI changes).
- **musl build**: a C dependency not finding `musl-gcc` — confirm the `CC_<target>` line ran; for `ring`, also set `CFLAGS_<target>` only if the log asks for it.
- **`macos-15-intel` unavailable**: switch that matrix entry to `macos-latest` with the same target (Apple's toolchain builds x86_64 on arm64) and drop its smoke `--version`/`analyze` lines behind `if [ "$TARGET" != x86_64-apple-darwin ]`; note it in the spec.
- **npm smoke**: if `npm install --omit=optional` still fetches the unpublished platform packages and fails, install with `--no-package-lock --omit=optional` and both tarballs listed explicitly; keep the test, change only the install line.

- [ ] **Step 5: Final whole-branch check**

Run: `npm run test:packaging` locally and `gh pr checks` on the PR.
Expected: local suite passes; every PR check passes. Report the run URLs to the human partner, and the remaining manual steps: npm org + `NPM_TOKEN`, tap repository + `HOMEBREW_TAP_TOKEN`, merge to `main`, push `v0.1.0`, run the Demo workflow.
