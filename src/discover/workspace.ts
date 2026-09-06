import path from "node:path";
import type { Service } from "../graph/types";
import { findDirs, findFiles, isRecord, readJson, readText } from "./fs";
import { describeDirectory } from "./language";
import { parseYamlDocs } from "./yaml";

interface Pattern {
  pattern: string;
  file: string;
}

/**
 * Strategy 4. Workspace configuration lists member directories explicitly:
 * package.json `workspaces`, pnpm-workspace.yaml `packages`, Cargo
 * `[workspace] members`, and Nx `project.json` files. turbo.json rides on
 * package.json workspaces, so it needs no handling of its own.
 */
export async function discoverFromWorkspace(root: string): Promise<Service[]> {
  const patterns: Pattern[] = [];

  const pkg = await readJson(path.join(root, "package.json"));
  if (isRecord(pkg)) {
    const ws = pkg.workspaces;
    const list = Array.isArray(ws) ? ws : isRecord(ws) && Array.isArray(ws.packages) ? ws.packages : [];
    for (const p of list) if (typeof p === "string") patterns.push({ pattern: p, file: "package.json" });
  }

  const pnpm = await readText(path.join(root, "pnpm-workspace.yaml"));
  if (pnpm !== null) {
    const json = parseYamlDocs(pnpm).docs[0]?.json;
    if (isRecord(json) && Array.isArray(json.packages)) {
      for (const p of json.packages) if (typeof p === "string") patterns.push({ pattern: p, file: "pnpm-workspace.yaml" });
    }
  }

  const cargo = await readText(path.join(root, "Cargo.toml"));
  if (cargo !== null) {
    for (const m of cargoMembers(cargo)) patterns.push({ pattern: m, file: "Cargo.toml" });
  }

  // dir -> which declaration listed it
  const dirs = new Map<string, Pattern>();
  for (const pj of await findFiles(root, ["**/project.json"])) {
    const dir = path.posix.dirname(pj);
    if (dir !== ".") dirs.set(dir, { pattern: pj, file: pj });
  }

  const exclude = patterns.filter((p) => p.pattern.startsWith("!")).map((p) => p.pattern.slice(1));
  for (const p of patterns) {
    if (p.pattern.startsWith("!")) continue;
    for (const dir of await findDirs(root, [p.pattern], exclude)) {
      if (dir !== "." && !dirs.has(dir)) dirs.set(dir, p);
    }
  }

  const found = new Map<string, Service>();
  for (const [dir, source] of [...dirs.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    const described = await describeDirectory(path.join(root, dir));
    if (!described.manifest) continue;
    const name = path.posix.basename(dir);
    if (found.has(name)) continue;
    found.set(name, {
      name,
      root: dir,
      language: described.language,
      entryPoints: described.entryPoints,
      role: "code",
      discoveredBy: "workspace",
      evidence: { file: source.file, detail: source.pattern },
      ...(described.packageName ? { packageName: described.packageName } : {}),
    });
  }
  return [...found.values()];
}

/** `[workspace] members = ["a", "crates/*"]`, without a TOML parser. */
export function cargoMembers(toml: string): string[] {
  const block = /\[workspace\][\s\S]*?members\s*=\s*\[([\s\S]*?)\]/.exec(toml);
  if (!block?.[1]) return [];
  return block[1]
    .split("\n")
    .map((line) => line.replace(/#.*$/, ""))
    .join(",")
    .split(",")
    .map((s) => s.trim().replace(/^["']|["']$/g, ""))
    .filter((s) => s.length > 0);
}
