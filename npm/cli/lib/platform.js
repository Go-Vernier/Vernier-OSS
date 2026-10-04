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
