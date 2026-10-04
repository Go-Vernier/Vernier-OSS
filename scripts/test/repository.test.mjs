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
