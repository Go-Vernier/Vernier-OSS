import { describe, expect, it } from "vitest";
import { BlastGraph } from "../src/graph/graph";
import type { Service } from "../src/graph/types";

const service = (name: string): Service => ({
  name,
  root: name,
  language: "go",
  entryPoints: [],
  role: "code",
  discoveredBy: "monorepo",
  evidence: { file: `${name}/go.mod` },
});

describe("BlastGraph", () => {
  it("stores services and directed edges, and answers inbound queries", () => {
    const g = new BlastGraph();
    g.addService(service("checkout"));
    g.addService(service("orders"));
    g.addService(service("payments"));
    g.addEdge({ source: "checkout", target: "orders", type: "http", confidence: "static", evidence: [] });
    g.addEdge({ source: "payments", target: "orders", type: "event", confidence: "inferred", evidence: [] });

    expect(g.size).toEqual({ services: 3, edges: 2 });
    expect(g.inbound("orders").map((e) => e.source).sort()).toEqual(["checkout", "payments"]);
    expect(g.outbound("orders")).toEqual([]);
    expect(g.services().map((s) => s.name)).toEqual(["checkout", "orders", "payments"]);
  });

  it("keeps parallel edges of different types between the same pair", () => {
    const g = new BlastGraph();
    g.addService(service("a"));
    g.addService(service("b"));
    g.addEdge({ source: "a", target: "b", type: "http", confidence: "static", evidence: [] });
    g.addEdge({ source: "a", target: "b", type: "import", confidence: "static", evidence: [] });
    expect(g.edges()).toHaveLength(2);
  });

  it("refuses an edge to a service it does not know", () => {
    const g = new BlastGraph();
    g.addService(service("a"));
    expect(() =>
      g.addEdge({ source: "a", target: "ghost", type: "http", confidence: "static", evidence: [] }),
    ).toThrow(/Unknown service "ghost"/);
  });
});
