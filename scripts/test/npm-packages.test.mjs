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
