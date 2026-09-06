/**
 * How sure we are that an edge exists. The weakest confidence on a path
 * decides the confidence of everything reached through it.
 *
 *   observed   seen in production traces
 *   static     a code path exists, never observed
 *   inferred   joined through an indirection we cannot resolve exactly
 *              (a topic, a shared table)
 *   uncertain  a computed URL or a fuzzy name match; included on purpose,
 *              because recall beats precision here
 */
export type Confidence = "observed" | "static" | "inferred" | "uncertain";

export const CONFIDENCE_RANK: Record<Confidence, number> = {
  observed: 3,
  static: 2,
  inferred: 1,
  uncertain: 0,
};

/** Where a fact came from. Every service and every edge carries one. */
export interface Evidence {
  /** Path relative to the repository root. */
  file: string;
  /** 1-based line number, when known. */
  line?: number;
  /** Short human-readable note: the matched key, the image, the URL. */
  detail?: string;
}

/** The strategies stage 1 tries, in this order, stopping at the first
 * that yields more than one service with code in the repository. */
export type DiscoveryStrategy =
  | "docker-compose"
  | "kubernetes"
  | "monorepo"
  | "workspace";

/** "root" is the honest fallback: the repository itself is one service. */
export type ServiceSource = DiscoveryStrategy | "root";

/** A service with code in this repository, or infrastructure the
 * repository declares but does not build (a database image, a broker). */
export type ServiceRole = "code" | "infrastructure";

export interface Service {
  name: string;
  /** Directory relative to the repository root. Null for image-only services. */
  root: string | null;
  language: string | null;
  /** Files that start the service, relative to its root. Best effort. */
  entryPoints: string[];
  role: ServiceRole;
  discoveredBy: ServiceSource;
  /** The declaration that told us this service exists. */
  evidence: Evidence;
  /** Container image, when the declaration names one. */
  image?: string;
  /** Package name from its manifest, when different from the directory name. */
  packageName?: string;
}

export type EdgeType = "http" | "event" | "grpc" | "database" | "import";

/** `source` depends on `target`: source calls, publishes to, imports from,
 * or shares a database with target. */
export interface Edge {
  source: string;
  target: string;
  type: EdgeType;
  confidence: Confidence;
  evidence: Evidence[];
}
