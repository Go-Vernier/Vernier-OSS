import { readdir } from "node:fs/promises";
import path from "node:path";
import type { Service } from "../graph/types";
import { isDir } from "./fs";
import { describeDirectory } from "./language";

/**
 * Conventional parents. The spec names services/, apps/ and packages/;
 * src/ is added because the reference corpus (Google's microservices-demo,
 * the OpenTelemetry demo) keeps one service per directory under src/.
 */
export const MONOREPO_PARENTS = ["services", "apps", "packages", "microservices", "src"];

/**
 * Strategy 3. A child directory of a conventional parent that holds its own
 * manifest (package.json, go.mod, pom.xml, requirements.txt, ...) is a
 * service. Children without one (docs, shared config) are not.
 */
export async function discoverFromMonorepo(root: string): Promise<Service[]> {
  const found = new Map<string, Service>();
  for (const parent of MONOREPO_PARENTS) {
    const parentAbs = path.join(root, parent);
    if (!(await isDir(parentAbs))) continue;
    const entries = await readdir(parentAbs, { withFileTypes: true });
    const children = entries
      .filter((e) => e.isDirectory() && !e.name.startsWith(".") && e.name !== "node_modules")
      .map((e) => e.name)
      .sort();

    for (const child of children) {
      const abs = path.join(parentAbs, child);
      const described = await describeDirectory(abs);
      if (!described.manifest) continue;
      // First parent wins on a name clash (apps/web vs packages/web).
      if (found.has(child)) continue;
      found.set(child, {
        name: child,
        root: `${parent}/${child}`,
        language: described.language,
        entryPoints: described.entryPoints,
        role: "code",
        discoveredBy: "monorepo",
        evidence: {
          file: `${parent}/${child}/${described.manifest.file}`,
          detail: described.manifest.file,
        },
        ...(described.packageName ? { packageName: described.packageName } : {}),
      });
    }
  }
  return [...found.values()];
}
