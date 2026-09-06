import path from "node:path";
import { isMap, isNode, isScalar } from "yaml";
import type { Service } from "../graph/types";
import { DirectoryIndex, imageBasename } from "./directories";
import { interpolate, loadComposeEnv } from "./env";
import { findFiles, isDir, isFile, isRecord, readText, rel } from "./fs";
import { describeDirectory } from "./language";
import { parseYamlDocs } from "./yaml";

export const COMPOSE_PATTERNS = ["**/docker-compose*.{yml,yaml}", "**/compose.{yml,yaml}"];

/**
 * Strategy 1. Each key under `services:` is a service. `${VAR}` references
 * are resolved from the `.env` beside the compose file.
 *
 * Its directory, in order of preference:
 *   - the directory of `build.dockerfile` when that is a subdirectory of the
 *     build context (monorepos build every service from the repository root
 *     with `context: .` and `dockerfile: src/<name>/Dockerfile`)
 *   - when the context is the repository root, a directory matching the
 *     service name (the Dockerfile variable could not be resolved)
 *   - `build.context` (or a string `build:`)
 *   - for `image:`-only services, a directory matching the service name or
 *     the image name; otherwise the service is infrastructure the repository
 *     runs but does not build
 */
export async function discoverFromCompose(root: string): Promise<Service[]> {
  const files = await findFiles(root, COMPOSE_PATTERNS);
  if (files.length === 0) return [];
  const index = await DirectoryIndex.build(root);
  const found = new Map<string, Service>();

  for (const file of files) {
    const text = await readText(path.join(root, file));
    if (text === null) continue;
    const { docs, lineOf } = parseYamlDocs(text);
    const first = docs[0];
    if (!first) continue;
    const servicesNode = first.doc.get("services", true);
    if (!isMap(servicesNode)) continue;
    const composeDir = path.dirname(path.join(root, file));
    const env = await loadComposeEnv(composeDir);

    for (const pair of servicesNode.items) {
      if (!isScalar(pair.key)) continue;
      const name = String(pair.key.value);
      const line = pair.key.range ? lineOf(pair.key.range[0]) : undefined;
      const def = isNode(pair.value) ? (pair.value.toJSON() as unknown) : null;
      const definition = isRecord(def) ? def : {};
      const image = typeof definition.image === "string" ? interpolate(definition.image, env) : undefined;

      let context: string | null = null;
      let dockerfile: string | null = null;
      const build = definition.build;
      if (typeof build === "string") context = interpolate(build, env);
      else if (isRecord(build)) {
        if (typeof build.context === "string") context = interpolate(build.context, env);
        if (typeof build.dockerfile === "string") dockerfile = interpolate(build.dockerfile, env);
        if (context === null && dockerfile !== null) context = ".";
      }

      let serviceRoot: string | null = null;
      let detail: string;
      if (context !== null) {
        const contextAbs = path.resolve(composeDir, context);
        detail = `build: ${context}`;
        if (dockerfile !== null) {
          const dockerfileAbs = path.resolve(contextAbs, dockerfile);
          const dockerfileDir = path.dirname(dockerfileAbs);
          if (dockerfileDir !== contextAbs && (await isFile(dockerfileAbs))) {
            serviceRoot = rel(root, dockerfileDir);
            detail = `build: ${context}, dockerfile: ${dockerfile}`;
          }
        }
        if (serviceRoot === null && contextAbs === root) {
          const matched = await index.match([name]);
          if (matched !== null && matched !== ".") {
            serviceRoot = matched;
            detail = `build: ${context}, matched directory ${matched}`;
          }
        }
        if (serviceRoot === null && (await isDir(contextAbs))) serviceRoot = rel(root, contextAbs);
      } else {
        serviceRoot = await index.match([name, imageBasename(image)]);
        detail = image ? `image: ${image}` : "service";
      }

      const described = serviceRoot !== null
        ? await describeDirectory(path.join(root, serviceRoot))
        : { manifest: null, language: null, entryPoints: [] as string[] };

      const service: Service = {
        name,
        root: serviceRoot,
        language: described.language,
        entryPoints: described.entryPoints,
        role: serviceRoot !== null ? "code" : "infrastructure",
        discoveredBy: "docker-compose",
        evidence: { file, ...(line !== undefined ? { line } : {}), detail },
        ...(image ? { image } : {}),
        ...(described.packageName ? { packageName: described.packageName } : {}),
      };

      const prev = found.get(name);
      if (!prev || (prev.role === "infrastructure" && service.role === "code")) {
        found.set(name, service);
      }
    }
  }
  return [...found.values()];
}
