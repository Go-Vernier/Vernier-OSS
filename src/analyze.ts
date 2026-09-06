import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";
import { discoverServices, type DiscoveryAttempt } from "./discover";
import { isDir } from "./discover/fs";
import { BlastGraph } from "./graph/graph";
import type { DiscoveryStrategy, Edge, Service } from "./graph/types";

const run = promisify(execFile);

export interface Analysis {
  /** `owner/repo` from the git remote, or the directory name. */
  repository: string;
  /** Absolute path that was analysed. */
  root: string;
  discovery: {
    strategy: DiscoveryStrategy | null;
    attempted: DiscoveryAttempt[];
  };
  graph: BlastGraph;
  /** Stage 3 fills this in. Until then the report says so plainly. */
  runtime: { connected: false };
}

export interface AnalysisJSON {
  repository: string;
  root: string;
  discovery: Analysis["discovery"];
  services: Service[];
  edges: Edge[];
  runtime: Analysis["runtime"];
}

/** Runs every built stage over a repository and returns the graph. */
export async function analyze(root: string): Promise<Analysis> {
  const absRoot = path.resolve(root);
  if (!(await isDir(absRoot))) throw new Error(`not a directory: ${root}`);

  const [repository, discovery] = await Promise.all([
    repositoryName(absRoot),
    discoverServices(absRoot),
  ]);

  const graph = new BlastGraph();
  for (const service of discovery.services) graph.addService(service);

  return {
    repository,
    root: absRoot,
    discovery: { strategy: discovery.strategy, attempted: discovery.attempted },
    graph,
    runtime: { connected: false },
  };
}

export function analysisToJSON(analysis: Analysis): AnalysisJSON {
  const { services, edges } = analysis.graph.toJSON();
  return {
    repository: analysis.repository,
    root: analysis.root,
    discovery: analysis.discovery,
    services,
    edges,
    runtime: analysis.runtime,
  };
}

/** `owner/repo` from the origin remote when there is one. */
export async function repositoryName(absRoot: string): Promise<string> {
  try {
    const { stdout } = await run("git", ["-C", absRoot, "remote", "get-url", "origin"], {
      timeout: 5_000,
    });
    const match = /[:/]([^/:\s]+)\/([^/\s]+?)(?:\.git)?\/?$/.exec(stdout.trim());
    if (match?.[1] && match[2]) return `${match[1]}/${match[2]}`;
  } catch {
    // not a git repository, or no origin: fall through
  }
  return path.basename(absRoot);
}
