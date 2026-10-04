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
