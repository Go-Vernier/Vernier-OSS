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
