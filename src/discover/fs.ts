import { readFile, stat } from "node:fs/promises";
import path from "node:path";
import { glob } from "tinyglobby";

/** Directories that never hold a service declaration worth reading. */
export const IGNORE = [
  "**/node_modules/**",
  "**/.git/**",
  "**/vendor/**",
  "**/dist/**",
  "**/build/**",
  "**/target/**",
  "**/.next/**",
  "**/__pycache__/**",
  "**/.venv/**",
  "**/venv/**",
  "**/bin/**",
  "**/obj/**",
];

export async function findFiles(
  root: string,
  patterns: string[],
  extraIgnore: string[] = [],
): Promise<string[]> {
  const files = await glob(patterns, {
    cwd: root,
    ignore: [...IGNORE, ...extraIgnore],
    onlyFiles: true,
    expandDirectories: false,
  });
  return files.map(posix).sort();
}

export async function findDirs(
  root: string,
  patterns: string[],
  extraIgnore: string[] = [],
): Promise<string[]> {
  const dirs = await glob(patterns, {
    cwd: root,
    ignore: [...IGNORE, ...extraIgnore],
    onlyDirectories: true,
    expandDirectories: false,
  });
  return dirs.map((d) => posix(d).replace(/\/+$/, "")).sort();
}

export async function isDir(p: string): Promise<boolean> {
  try {
    return (await stat(p)).isDirectory();
  } catch {
    return false;
  }
}

export async function isFile(p: string): Promise<boolean> {
  try {
    return (await stat(p)).isFile();
  } catch {
    return false;
  }
}

export async function readText(p: string): Promise<string | null> {
  try {
    return await readFile(p, "utf8");
  } catch {
    return null;
  }
}

export async function readJson(p: string): Promise<unknown> {
  const text = await readText(p);
  if (text === null) return null;
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return null;
  }
}

/** Repository-relative POSIX path, "." for the root itself, or null when
 * `abs` lies outside the repository. */
export function rel(root: string, abs: string): string | null {
  const r = path.relative(root, abs);
  if (r.startsWith("..") || path.isAbsolute(r)) return null;
  return r === "" ? "." : posix(r);
}

export function posix(p: string): string {
  return p.split(path.sep).join("/");
}

export function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}
