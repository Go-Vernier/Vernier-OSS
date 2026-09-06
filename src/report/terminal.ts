import { createColors } from "picocolors";
import type { Analysis } from "../analyze";
import type { Service } from "../graph/types";

export interface ReportOptions {
  color?: boolean;
}

/**
 * The repository report. It says what was found, how it was found, and what
 * this version does not do yet. It never fills a gap with a guess.
 */
export function formatRepoReport(analysis: Analysis, options: ReportOptions = {}): string {
  const c = createColors(options.color ?? false);
  const services = analysis.graph.services();
  const code = services.filter((s) => s.role === "code");
  const infra = services.filter((s) => s.role === "infrastructure");
  const { strategy, attempted } = analysis.discovery;
  const out: string[] = [];

  out.push(c.bold("BLAST RADIUS"), "");
  out.push(row("Repository", analysis.repository));
  out.push(row(
    "Services",
    strategy
      ? `${code.length} detected  ${c.dim(`(${strategy})`)}`
      : `${code.length} detected`,
  ));
  out.push(row("Runtime", c.dim("not connected - static only")));
  out.push("");

  if (!strategy) {
    const lines = code.length === 0 && infra.length > 0
      ? [
          `This repository declares ${infra.length} services but builds none of them here,`,
          "so there is no code to trace. Run blast-radius on the repository that",
          "holds the services.",
        ]
      : [
          "This looks like a single service. Blast radius analysis needs a",
          "multi-service repository.",
        ];
    out.push(
      ...lines.map((l) => "  " + c.yellow(l)),
      "",
      row("Tried", c.dim(attempted.map((a) => `${a.strategy} ${a.services}`).join(" · "))),
      "",
    );
  }

  if (services.length > 0) {
    out.push(c.bold("SERVICES"), "");
    out.push(...table(services, c));
    out.push("");
    if (infra.length > 0) {
      out.push("  " + c.dim(`${infra.length} declared but not built here (images): ${infra.map((s) => s.name).join(", ")}`), "");
    }
  }

  out.push(c.bold("STRUCTURE"), "");
  out.push(
    "  " + c.dim("Dependency mapping is not built yet: this release reports service"),
    "  " + c.dim("boundaries only. Static edges, the runtime join and the blast radius"),
    "  " + c.dim("itself come next. See docs/build-spec.md for the order."),
  );
  return out.join("\n");
}

function row(label: string, value: string): string {
  return `  ${label.padEnd(13)} ${value}`;
}

type Colors = ReturnType<typeof createColors>;

function table(services: Service[], c: Colors): string[] {
  const rows = services.map((s) => ({
    name: s.name,
    language: s.language ?? "-",
    root: s.root ?? (s.image ? `(image ${s.image})` : "(no directory)"),
    evidence: s.evidence.line !== undefined ? `${s.evidence.file}:${s.evidence.line}` : s.evidence.file,
    infra: s.role === "infrastructure",
  }));
  const w = {
    name: Math.max(4, ...rows.map((r) => r.name.length)),
    language: Math.max(8, ...rows.map((r) => r.language.length)),
    root: Math.max(4, ...rows.map((r) => r.root.length)),
  };
  const lines = [
    "  " + c.dim(["NAME".padEnd(w.name), "LANGUAGE".padEnd(w.language), "ROOT".padEnd(w.root), "EVIDENCE"].join("  ")),
  ];
  for (const r of rows) {
    const cells = [r.name.padEnd(w.name), r.language.padEnd(w.language), r.root.padEnd(w.root), c.dim(r.evidence)];
    lines.push("  " + (r.infra ? c.dim(cells.join("  ")) : cells.join("  ")));
  }
  return lines;
}
