import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { discoverServices } from "../src/discover";
import { imageBasename, normalise } from "../src/discover/directories";
import { hasUnresolved, interpolate, parseDotenv } from "../src/discover/env";
import { cargoMembers } from "../src/discover/workspace";

const fixture = (name: string): string =>
  fileURLToPath(new URL(`./fixtures/${name}/`, import.meta.url));

const byName = <T extends { name: string }>(items: T[]): Record<string, T> =>
  Object.fromEntries(items.map((i) => [i.name, i]));

describe("docker-compose discovery", () => {
  it("reads services, build contexts, images, and line numbers", async () => {
    const result = await discoverServices(fixture("compose-app"));
    expect(result.strategy).toBe("docker-compose");
    const s = byName(result.services);
    expect(Object.keys(s).sort()).toEqual(["checkout", "orders", "redis"]);

    expect(s.checkout).toMatchObject({
      root: "services/checkout",
      language: "typescript",
      role: "code",
      packageName: "@acme/checkout",
      evidence: { file: "docker-compose.yml", line: 2, detail: "build: ./services/checkout" },
    });
    expect(s.checkout?.entryPoints).toContain("src/index.ts");

    expect(s.orders).toMatchObject({ root: "services/orders", language: "go", role: "code" });
    expect(s.orders?.entryPoints).toEqual(["main.go"]);
    expect(s.orders?.evidence.line).toBe(5);

    expect(s.redis).toMatchObject({
      root: null,
      role: "infrastructure",
      image: "redis:7-alpine",
      evidence: { file: "docker-compose.yml", line: 9 },
    });
  });

  it("wins over the monorepo convention, so an undeclared directory is not a service", async () => {
    const result = await discoverServices(fixture("compose-app"));
    expect(result.services.map((s) => s.name)).not.toContain("legacy");
    expect(result.attempted).toEqual([{ strategy: "docker-compose", services: 2 }]);
  });

  it("matches image-only services to directories by name suffix or image name", async () => {
    const result = await discoverServices(fixture("compose-images-app"));
    expect(result.strategy).toBe("docker-compose");
    const s = byName(result.services);
    expect(s["customers-service"]).toMatchObject({
      root: "spring-petclinic-customers-service",
      language: "java",
      role: "code",
      image: "springcommunity/spring-petclinic-customers-service:3.2.0",
      evidence: { detail: "image: springcommunity/spring-petclinic-customers-service:3.2.0" },
    });
    expect(s["vets-service"]?.root).toBe("spring-petclinic-vets-service");
    expect(s["config-server"]?.root).toBe("spring-petclinic-config-server");
    expect(s["tracing-server"]).toMatchObject({ root: null, role: "infrastructure", image: "openzipkin/zipkin" });
  });

  it("uses the Dockerfile's directory when every service builds from the repository root", async () => {
    const result = await discoverServices(fixture("compose-rootctx-app"));
    expect(result.strategy).toBe("docker-compose");
    const s = byName(result.services);
    expect(s.accounting).toMatchObject({
      root: "src/accounting",
      language: "go",
      role: "code",
      evidence: { detail: "build: ./, dockerfile: ./src/accounting/Dockerfile" },
    });
    expect(s.frontend).toMatchObject({ root: "src/frontend", language: "javascript", packageName: "frontend" });
    expect(s.kafka).toMatchObject({ root: null, role: "infrastructure" });
  });

  it("resolves ${VAR} references from the .env beside the compose file", async () => {
    const result = await discoverServices(fixture("compose-env-app"));
    expect(result.strategy).toBe("docker-compose");
    const s = byName(result.services);
    expect(s.ad).toMatchObject({
      root: "src/ad",
      language: "java",
      image: "ghcr.io/demo:1.0-ad",
      evidence: { detail: "build: ./, dockerfile: ./src/ad/Dockerfile" },
    });
    // CART_DOCKERFILE is not in .env: the service name still finds src/cart.
    expect(s.cart).toMatchObject({
      root: "src/cart",
      language: "go",
      evidence: { detail: "build: ./, matched directory src/cart" },
    });
    // An unresolvable image stays visible as written, not silently dropped.
    expect(s.db).toMatchObject({ root: null, role: "infrastructure", image: "${POSTGRES_IMAGE}" });
  });

  it("keeps a deploy-only repository's images so the report can say what it is", async () => {
    const result = await discoverServices(fixture("deploy-only-app"));
    expect(result.strategy).toBeNull();
    expect(result.services).toHaveLength(3);
    expect(result.services.every((s) => s.role === "infrastructure")).toBe(true);
    expect(result.attempted[0]).toEqual({ strategy: "docker-compose", services: 0 });
  });
});

describe("compose environment", () => {
  it("parses dotenv files", () => {
    const env = parseDotenv([
      "# comment",
      "PLAIN=value",
      "export EXPORTED=yes",
      'QUOTED="a # not a comment"',
      "SINGLE='x'",
      "TRAILING=abc # comment",
      "EMPTY=",
      "not a valid line",
    ].join("\n"));
    expect(env).toEqual({
      PLAIN: "value",
      EXPORTED: "yes",
      QUOTED: "a # not a comment",
      SINGLE: "x",
      TRAILING: "abc",
      EMPTY: "",
    });
  });

  it("interpolates compose references and leaves unknown ones visible", () => {
    const env = { IMAGE: "ghcr.io/x", EMPTY: "" };
    expect(interpolate("${IMAGE}:${TAG:-latest}", env)).toBe("ghcr.io/x:latest");
    expect(interpolate("${EMPTY:-fallback}", env)).toBe("fallback");
    expect(interpolate("${TAG-dash}", env)).toBe("dash");
    expect(interpolate("$IMAGE/svc", env)).toBe("ghcr.io/x/svc");
    expect(interpolate("${MISSING}", env)).toBe("${MISSING}");
    expect(hasUnresolved("${MISSING}")).toBe(true);
    expect(hasUnresolved("plain")).toBe(false);
  });
});

describe("directory matching", () => {
  it("takes the image basename without registry, tag or digest", () => {
    expect(imageBasename("gcr.io/google-samples/microservices-demo/cartservice:v0.10.0")).toBe("cartservice");
    expect(imageBasename("redis:7-alpine")).toBe("redis");
    expect(imageBasename("ghcr.io/acme/api@sha256:abcdef")).toBe("api");
    expect(imageBasename(undefined)).toBeUndefined();
  });
});

describe("kubernetes discovery", () => {
  it("matches workload names to directories and keeps image-only workloads as infrastructure", async () => {
    const result = await discoverServices(fixture("k8s-app"));
    expect(result.strategy).toBe("kubernetes");
    const s = byName(result.services);
    expect(Object.keys(s).sort()).toEqual(["cartservice", "frontend", "redis-cart"]);

    expect(s.frontend).toMatchObject({
      root: "src/frontend",
      language: "go",
      role: "code",
      image: "acme/frontend:1.0",
    });
    // The Deployment beats the Service object of the same name.
    expect(s.frontend?.evidence).toEqual({
      file: "kubernetes/frontend.yaml",
      line: 9,
      detail: "Deployment, image: acme/frontend:1.0",
    });
    expect(s.cartservice).toMatchObject({ root: "src/cartservice", language: "csharp" });
    expect(s["redis-cart"]).toMatchObject({ root: null, role: "infrastructure", image: "redis:alpine" });
  });

  it("skips Helm templates instead of failing", async () => {
    const result = await discoverServices(fixture("k8s-app"));
    expect(result.services.some((s) => s.name.includes("Values"))).toBe(false);
  });

  it("normalises service-ish suffixes", () => {
    expect(normalise("checkout-api")).toBe("checkout");
    expect(normalise("CheckoutService")).toBe("checkout");
    expect(normalise("checkout_svc")).toBe("checkout");
    expect(normalise("redis-cart")).toBe("rediscart");
  });
});

describe("monorepo discovery", () => {
  it("takes children of conventional parents that carry a manifest", async () => {
    const result = await discoverServices(fixture("monorepo-app"));
    expect(result.strategy).toBe("monorepo");
    const s = byName(result.services);
    expect(Object.keys(s).sort()).toEqual(["api", "web", "worker"]);
    expect(s.api).toMatchObject({
      root: "services/api",
      language: "typescript",
      entryPoints: ["src/server.ts"],
    });
    expect(s.worker).toMatchObject({
      root: "services/worker",
      language: "python",
      entryPoints: ["main.py"],
    });
    expect(s.web).toMatchObject({ root: "apps/web", language: "javascript" });
    expect(s.api?.evidence).toEqual({ file: "services/api/package.json", detail: "package.json" });
  });
});

describe("workspace discovery", () => {
  it("expands pnpm-workspace.yaml globs and honours negations", async () => {
    const result = await discoverServices(fixture("workspace-app"));
    expect(result.strategy).toBe("workspace");
    const s = byName(result.services);
    expect(Object.keys(s).sort()).toEqual(["billing", "gateway"]);
    expect(s.billing).toMatchObject({
      root: "components/billing",
      language: "typescript",
      packageName: "@acme/billing",
    });
    expect(s.gateway?.evidence).toEqual({ file: "pnpm-workspace.yaml", detail: "components/*" });
    expect(result.attempted.map((a) => a.services)).toEqual([0, 0, 0, 2]);
  });

  it("reads package.json workspaces", async () => {
    const result = await discoverServices(fixture("npm-workspace-app"));
    expect(result.strategy).toBe("workspace");
    expect(result.services.map((s) => s.name)).toEqual(["alpha", "beta"]);
    expect(result.services[0]?.evidence.file).toBe("package.json");
  });

  it("parses Cargo workspace members without a TOML parser", () => {
    const toml = [
      "[package]",
      'name = "root"',
      "",
      "[workspace]",
      "members = [",
      '  "crates/api", # the api',
      "  'crates/worker',",
      '  "tools/*"',
      "]",
    ].join("\n");
    expect(cargoMembers(toml)).toEqual(["crates/api", "crates/worker", "tools/*"]);
    expect(cargoMembers("[package]\nname = 'x'")).toEqual([]);
  });
});

describe("single service fallback", () => {
  it("reports the repository as one service and says which strategies were tried", async () => {
    const result = await discoverServices(fixture("single-app"));
    expect(result.strategy).toBeNull();
    expect(result.attempted).toEqual([
      { strategy: "docker-compose", services: 0 },
      { strategy: "kubernetes", services: 0 },
      { strategy: "monorepo", services: 0 },
      { strategy: "workspace", services: 0 },
    ]);
    expect(result.services).toHaveLength(1);
    expect(result.services[0]).toMatchObject({
      name: "single-app",
      root: ".",
      language: "typescript",
      role: "code",
      discoveredBy: "root",
    });
  });
});
