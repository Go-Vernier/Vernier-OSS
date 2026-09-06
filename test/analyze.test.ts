import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { analysisToJSON, analyze } from "../src/analyze";
import { formatRepoReport } from "../src/report/terminal";

const fixture = (name: string): string =>
  fileURLToPath(new URL(`./fixtures/${name}/`, import.meta.url));

describe("analyze", () => {
  it("builds a graph of the discovered services", async () => {
    const analysis = await analyze(fixture("compose-app"));
    expect(analysis.repository.length).toBeGreaterThan(0);
    expect(analysis.discovery.strategy).toBe("docker-compose");
    expect(analysis.graph.size).toEqual({ services: 3, edges: 0 });
    expect(analysis.runtime.connected).toBe(false);
  });

  it("serialises to plain JSON", async () => {
    const json = analysisToJSON(await analyze(fixture("monorepo-app")));
    expect(json.services.map((s) => s.name)).toEqual(["api", "web", "worker"]);
    expect(json.edges).toEqual([]);
    expect(JSON.parse(JSON.stringify(json))).toEqual(json);
  });

  it("rejects a path that is not a directory", async () => {
    await expect(analyze(`${fixture("compose-app")}docker-compose.yml`)).rejects.toThrow(
      /not a directory/,
    );
  });
});

describe("terminal report", () => {
  it("shows what was found and says what is not built yet", async () => {
    const report = formatRepoReport(await analyze(fixture("compose-app")));
    expect(report).toContain("BLAST RADIUS");
    expect(report).toContain("2 detected  (docker-compose)");
    expect(report).toContain("not connected - static only");
    expect(report).toMatch(/checkout\s+typescript\s+services\/checkout\s+docker-compose\.yml:2/);
    expect(report).toContain("1 declared but not built here (images): redis");
    expect(report).toContain("Dependency mapping is not built yet");
    expect(report).not.toMatch(/\[/); // no colour codes unless asked
  });

  it("is honest about a single-service repository", async () => {
    const report = formatRepoReport(await analyze(fixture("single-app")));
    expect(report).toContain("This looks like a single service.");
    expect(report).toContain("docker-compose 0");
    expect(report).toContain("workspace 0");
  });

  it("names a deploy-only repository for what it is", async () => {
    const report = formatRepoReport(await analyze(fixture("deploy-only-app")));
    expect(report).toContain("declares 3 services but builds none of them here");
    expect(report).toContain("3 declared but not built here (images): carts, carts-db, front-end");
  });

  it("colours only when asked", async () => {
    const report = formatRepoReport(await analyze(fixture("compose-app")), { color: true });
    expect(report).toMatch(/\[/);
  });
});
