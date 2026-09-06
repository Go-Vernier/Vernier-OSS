import path from "node:path";
import { glob } from "tinyglobby";
import { isFile, isRecord, posix, readJson } from "./fs";

export interface Manifest {
  /** Manifest file relative to the directory. */
  file: string;
  language: string | null;
}

/** Checked in order; the first hit decides the language. */
const MANIFESTS: ReadonlyArray<readonly [string, string]> = [
  ["package.json", "javascript"],
  ["go.mod", "go"],
  ["pom.xml", "java"],
  ["build.gradle", "java"],
  ["build.gradle.kts", "kotlin"],
  ["requirements.txt", "python"],
  ["pyproject.toml", "python"],
  ["Pipfile", "python"],
  ["Cargo.toml", "rust"],
  ["Gemfile", "ruby"],
  ["composer.json", "php"],
  ["mix.exs", "elixir"],
];

/**
 * Does this directory hold a buildable service? Answers with the manifest
 * that says so. A bare Dockerfile counts: it is a service boundary even when
 * we cannot name the language.
 */
export async function detectManifest(dirAbs: string): Promise<Manifest | null> {
  for (const [file, language] of MANIFESTS) {
    if (!(await isFile(path.join(dirAbs, file)))) continue;
    if (file === "package.json" && (await isFile(path.join(dirAbs, "tsconfig.json")))) {
      return { file, language: "typescript" };
    }
    return { file, language };
  }
  const projects = await glob(["*.csproj", "*.fsproj"], {
    cwd: dirAbs,
    onlyFiles: true,
    expandDirectories: false,
  });
  const project = projects.sort()[0];
  if (project) {
    return { file: project, language: project.endsWith(".fsproj") ? "fsharp" : "csharp" };
  }
  if (await isFile(path.join(dirAbs, "Dockerfile"))) {
    return { file: "Dockerfile", language: null };
  }
  return null;
}

export interface DirectoryDescription {
  manifest: Manifest | null;
  language: string | null;
  entryPoints: string[];
  packageName?: string;
}

export async function describeDirectory(dirAbs: string): Promise<DirectoryDescription> {
  const manifest = await detectManifest(dirAbs);
  const language = manifest?.language ?? null;
  const entryPoints = await detectEntryPoints(dirAbs, language);
  const description: DirectoryDescription = { manifest, language, entryPoints };
  if (manifest?.file === "package.json") {
    const pkg = await readJson(path.join(dirAbs, "package.json"));
    if (isRecord(pkg) && typeof pkg.name === "string") description.packageName = pkg.name;
  }
  return description;
}

/** Best-effort entry points. Cheap checks only; nothing is parsed. */
export async function detectEntryPoints(
  dirAbs: string,
  language: string | null,
): Promise<string[]> {
  const candidates: string[] = [];
  switch (language) {
    case "javascript":
    case "typescript": {
      const pkg = await readJson(path.join(dirAbs, "package.json"));
      if (isRecord(pkg)) {
        if (typeof pkg.main === "string") candidates.push(pkg.main);
        if (typeof pkg.bin === "string") candidates.push(pkg.bin);
        else if (isRecord(pkg.bin)) {
          for (const v of Object.values(pkg.bin)) if (typeof v === "string") candidates.push(v);
        }
      }
      candidates.push(
        "src/index.ts", "src/main.ts", "src/server.ts", "src/app.ts",
        "src/index.js", "src/main.js", "src/server.js", "src/app.js",
        "index.js", "server.js", "app.js", "main.js",
      );
      break;
    }
    case "go":
      candidates.push("main.go");
      break;
    case "python":
      candidates.push("main.py", "app.py", "manage.py", "server.py", "__main__.py", "wsgi.py", "asgi.py");
      break;
    case "rust":
      candidates.push("src/main.rs");
      break;
    case "ruby":
      candidates.push("config.ru", "app.rb");
      break;
    case "php":
      candidates.push("public/index.php", "index.php");
      break;
    default:
      break;
  }
  const found = new Set<string>();
  for (const c of candidates) {
    const clean = posix(path.normalize(c)).replace(/^\.\//, "");
    if (await isFile(path.join(dirAbs, clean))) found.add(clean);
  }
  if (language === "go") {
    const mains = await glob(["cmd/*/main.go"], { cwd: dirAbs, onlyFiles: true, expandDirectories: false });
    for (const m of mains) found.add(posix(m));
  }
  return [...found].sort();
}
