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
