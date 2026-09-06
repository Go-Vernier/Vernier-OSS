import path from "node:path";
import type { DiscoveryStrategy, Service } from "../graph/types";
import { discoverFromCompose } from "./docker-compose";
import { discoverFromKubernetes } from "./kubernetes";
import { describeDirectory } from "./language";
import { discoverFromMonorepo } from "./monorepo";
import { discoverFromWorkspace } from "./workspace";

export interface DiscoveryAttempt {
  strategy: DiscoveryStrategy;
  /** Services with code in the repository that this strategy found. */
  services: number;
}

export interface DiscoveryResult {
  services: Service[];
  /** The strategy that found more than one service, or null: the honest
   * single-service fallback. */
  strategy: DiscoveryStrategy | null;
  attempted: DiscoveryAttempt[];
}

const STRATEGIES: ReadonlyArray<readonly [DiscoveryStrategy, (root: string) => Promise<Service[]>]> = [
  ["docker-compose", discoverFromCompose],
  ["kubernetes", discoverFromKubernetes],
  ["monorepo", discoverFromMonorepo],
  ["workspace", discoverFromWorkspace],
];

const codeCount = (services: Service[]): number => services.filter((s) => s.role === "code").length;

/**
 * Stage 1. Tries each strategy in order and stops at the first that finds
 * more than one service with code in the repository. When none does, the
 * repository is reported as one service, keeping whatever infrastructure
 * was declared so a deploy-only repository is described, not hidden.
 * Boundaries are never invented.
 */
export async function discoverServices(root: string): Promise<DiscoveryResult> {
  const absRoot = path.resolve(root);
  const attempted: DiscoveryAttempt[] = [];
  let fallback: Service[] = [];

  for (const [strategy, run] of STRATEGIES) {
    const services = await run(absRoot);
    const code = codeCount(services);
    attempted.push({ strategy, services: code });
    if (code > 1) return { services: sortServices(services), strategy, attempted };
    const better = code > codeCount(fallback)
      || (code === codeCount(fallback) && services.length > fallback.length);
    if (better) fallback = services;
  }

  if (codeCount(fallback) === 0) {
    const described = await describeDirectory(absRoot);
    if (described.manifest) {
      fallback = [
        ...fallback,
        {
          name: path.basename(absRoot),
          root: ".",
          language: described.language,
          entryPoints: described.entryPoints,
          role: "code",
          discoveredBy: "root",
          evidence: { file: described.manifest.file, detail: described.manifest.file },
          ...(described.packageName ? { packageName: described.packageName } : {}),
        },
      ];
    }
  }
  return { services: sortServices(fallback), strategy: null, attempted };
}

function sortServices(services: Service[]): Service[] {
  return [...services].sort((a, b) => {
    if (a.role !== b.role) return a.role === "code" ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
}

export { discoverFromCompose, discoverFromKubernetes, discoverFromMonorepo, discoverFromWorkspace };
