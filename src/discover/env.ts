import path from "node:path";
import { readText } from "./fs";

export type Env = Record<string, string>;

/** Parses a dotenv file: `KEY=value`, optional `export`, quotes, comments. */
export function parseDotenv(text: string): Env {
  const env: Env = {};
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) continue;
    const match = /^(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/.exec(line);
    const key = match?.[1];
    if (!key) continue;
    let value = match[2] ?? "";
    const quoted = (value.startsWith('"') && value.endsWith('"') && value.length >= 2)
      || (value.startsWith("'") && value.endsWith("'") && value.length >= 2);
    value = quoted ? value.slice(1, -1) : value.replace(/\s+#.*$/, "").trim();
    env[key] = value;
  }
  return env;
}

/**
 * Compose interpolation: `${VAR}`, `${VAR:-default}`, `${VAR-default}`, `$VAR`.
 * Only the dotenv values are used, never the analyst's shell, so two people
 * analysing the same commit get the same answer. Unresolved references are
 * left in place so the evidence shows exactly what could not be resolved.
 */
export function interpolate(value: string, env: Env): string {
  return value.replace(
    /\$\{([A-Za-z_][A-Za-z0-9_]*)(?::?-([^}]*))?\}|\$([A-Za-z_][A-Za-z0-9_]*)/g,
    (whole: string, braced: string | undefined, fallback: string | undefined, bare: string | undefined) => {
      const key = braced ?? bare;
      if (!key) return whole;
      const resolved = env[key];
      if (resolved !== undefined && resolved !== "") return resolved;
      if (fallback !== undefined) return fallback;
      return whole;
    },
  );
}

export function hasUnresolved(value: string): boolean {
  return /\$\{?[A-Za-z_]/.test(value);
}

/** `.env` beside the compose file wins over `.env.example`. */
export async function loadComposeEnv(composeDir: string): Promise<Env> {
  const env: Env = {};
  for (const file of [".env.example", ".env"]) {
    const text = await readText(path.join(composeDir, file));
    if (text !== null) Object.assign(env, parseDotenv(text));
  }
  return env;
}
