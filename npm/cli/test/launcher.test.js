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
