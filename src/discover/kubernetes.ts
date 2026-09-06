import path from "node:path";
import type { Service } from "../graph/types";
import { DirectoryIndex, imageBasename } from "./directories";
import { COMPOSE_PATTERNS } from "./docker-compose";
import { findFiles, isRecord, readText } from "./fs";
import { describeDirectory } from "./language";
import { parseYamlDocs } from "./yaml";

const WORKLOAD_KINDS = new Set(["Deployment", "StatefulSet", "DaemonSet", "Job", "CronJob", "Rollout"]);
const KINDS = new Set([...WORKLOAD_KINDS, "Service"]);

/**
 * Strategy 2. Any `kind: Deployment` (or other workload) or `kind: Service`
 * is a service. The manifest names it and its image; the repository
 * directory is found by matching the name and the image against directory
 * names, since manifests rarely say where their code lives.
 */
export async function discoverFromKubernetes(root: string): Promise<Service[]> {
  const files = await findFiles(root, ["**/*.{yml,yaml}"], COMPOSE_PATTERNS);
  const index = await DirectoryIndex.build(root);
  const found = new Map<string, { service: Service; workload: boolean }>();

  for (const file of files) {
    const text = await readText(path.join(root, file));
    if (text === null || !/^kind:\s*\S+/m.test(text)) continue;
    // Helm and other Go templates are not YAML until rendered.
    if (text.includes("{{")) continue;
    const { docs } = parseYamlDocs(text);

    for (const { json, line } of docs) {
      if (!isRecord(json)) continue;
      const kind = json.kind;
      if (typeof kind !== "string" || !KINDS.has(kind)) continue;
      const metadata = isRecord(json.metadata) ? json.metadata : {};
      const name = typeof metadata.name === "string" ? metadata.name : null;
      if (!name) continue;

      const workload = WORKLOAD_KINDS.has(kind);
      const image = workload ? firstImage(json) : undefined;
      const serviceRoot = await index.match([name, imageBasename(image)]);
      const described = serviceRoot !== null
        ? await describeDirectory(path.join(root, serviceRoot))
        : { manifest: null, language: null, entryPoints: [] as string[] };

      const service: Service = {
        name,
        root: serviceRoot,
        language: described.language,
        entryPoints: described.entryPoints,
        role: serviceRoot !== null ? "code" : "infrastructure",
        discoveredBy: "kubernetes",
        evidence: { file, line, detail: image ? `${kind}, image: ${image}` : kind },
        ...(image ? { image } : {}),
        ...(described.packageName ? { packageName: described.packageName } : {}),
      };

      const prev = found.get(name);
      // A workload beats a Service object of the same name (it carries the
      // image); anything with code beats infrastructure.
      const better = !prev
        || (workload && !prev.workload)
        || (prev.service.role === "infrastructure" && service.role === "code");
      if (better) found.set(name, { service, workload });
    }
  }
  return [...found.values()].map((f) => f.service);
}

/** The first container image declared anywhere under this object. */
function firstImage(obj: unknown, depth = 0): string | undefined {
  if (depth > 8 || !isRecord(obj)) return undefined;
  const containers = obj.containers;
  if (Array.isArray(containers)) {
    for (const c of containers) {
      if (isRecord(c) && typeof c.image === "string") return c.image;
    }
  }
  for (const value of Object.values(obj)) {
    if (isRecord(value)) {
      const hit = firstImage(value, depth + 1);
      if (hit) return hit;
    }
  }
  return undefined;
}
