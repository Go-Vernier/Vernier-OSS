#!/usr/bin/env node
"use strict";
// Runs the prebuilt vernier binary from the platform package npm installed
// beside this one.

const { spawnSync } = require("node:child_process");
const { packageFor, binaryName } = require("../lib/platform");

const INSTALL_ELSEWHERE = [
  "Install vernier another way:",
  "  curl -fsSL https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.sh | sh",
  `  powershell -ExecutionPolicy ByPass -c "[Net.ServicePointManager]::SecurityProtocol = 'Tls12'; irm https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.ps1 | iex"`,
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
