import { MultiDirectedGraph } from "graphology";
import type { Edge, Service } from "./types";

export interface GraphJSON {
  services: Service[];
  edges: Edge[];
}

/**
 * The single in-memory graph every stage writes to. Nodes are services,
 * keyed by name. Edges point from the dependent to the dependency: an edge
 * checkout -> orders means checkout calls orders, so a change to orders
 * reaches checkout by walking inbound edges.
 */
export class BlastGraph {
  private readonly g = new MultiDirectedGraph<Service, Edge>();

  addService(service: Service): void {
    if (this.g.hasNode(service.name)) {
      this.g.replaceNodeAttributes(service.name, service);
    } else {
      this.g.addNode(service.name, service);
    }
  }

  hasService(name: string): boolean {
    return this.g.hasNode(name);
  }

  getService(name: string): Service | undefined {
    return this.g.hasNode(name) ? this.g.getNodeAttributes(name) : undefined;
  }

  addEdge(edge: Edge): void {
    for (const end of [edge.source, edge.target]) {
      if (!this.g.hasNode(end)) {
        throw new Error(
          `Unknown service "${end}" on edge ${edge.source} -> ${edge.target}`,
        );
      }
    }
    this.g.addDirectedEdge(edge.source, edge.target, edge);
  }

  services(): Service[] {
    return this.g.mapNodes((_, attrs) => attrs).sort(byName);
  }

  edges(): Edge[] {
    return this.g.mapEdges((_, attrs) => attrs);
  }

  /** Who depends on this service: every edge whose target is `name`. */
  inbound(name: string): Edge[] {
    return this.g.mapInEdges(name, (_, attrs) => attrs);
  }

  /** What this service depends on: every edge whose source is `name`. */
  outbound(name: string): Edge[] {
    return this.g.mapOutEdges(name, (_, attrs) => attrs);
  }

  get size(): { services: number; edges: number } {
    return { services: this.g.order, edges: this.g.size };
  }

  toJSON(): GraphJSON {
    return { services: this.services(), edges: this.edges() };
  }
}

const byName = (a: Service, b: Service): number => a.name.localeCompare(b.name);
