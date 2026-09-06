import { LineCounter, parseAllDocuments, type Document } from "yaml";

export interface YamlDoc {
  json: unknown;
  /** 1-based line where the document's content starts. */
  line: number;
  doc: Document;
}

export interface ParsedYaml {
  docs: YamlDoc[];
  /** 1-based line for a character offset in the source text. */
  lineOf(offset: number): number;
}

/**
 * Parses every document in a YAML file, keeping line numbers so evidence can
 * point at the exact declaration. Documents that fail to parse (Helm
 * templates with `{{ }}`, broken files) are skipped rather than fatal.
 */
export function parseYamlDocs(text: string): ParsedYaml {
  const lineCounter = new LineCounter();
  const lineOf = (offset: number): number => lineCounter.linePos(offset).line;
  const docs: YamlDoc[] = [];
  let parsed: Document[];
  try {
    parsed = parseAllDocuments(text, { lineCounter, uniqueKeys: false });
  } catch {
    return { docs, lineOf };
  }
  for (const doc of parsed) {
    if (doc.errors.length > 0) continue;
    let json: unknown;
    try {
      json = doc.toJSON();
    } catch {
      continue;
    }
    const start = doc.contents?.range?.[0] ?? 0;
    docs.push({ json, line: lineOf(start), doc });
  }
  return { docs, lineOf };
}
