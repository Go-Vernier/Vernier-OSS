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
