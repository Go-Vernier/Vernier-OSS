mod common;

use blastradius::*;
use common::{by_name, discover};
use pretty_assertions::assert_eq;

// ---------------------------------------------------------------- compose

#[test]
fn compose_reads_services_build_contexts_images_and_lines() {
    let r = discover("compose-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::DockerCompose));
    let s = by_name(&r.services);
    assert_eq!(
        s.keys().cloned().collect::<Vec<_>>(),
        vec!["checkout", "orders", "redis"]
    );
    let c = &s["checkout"];
    assert_eq!(c.root.as_deref(), Some("services/checkout"));
    assert_eq!(c.language.as_deref(), Some("typescript"));
    assert_eq!(c.role, ServiceRole::Code);
    assert_eq!(c.package_name.as_deref(), Some("@acme/checkout"));
    assert_eq!(
        c.evidence,
        Evidence {
            file: "docker-compose.yml".into(),
            line: Some(2),
            detail: Some("build: ./services/checkout".into())
        }
    );
    assert!(c.entry_points.contains(&"src/index.ts".to_string()));
    let o = &s["orders"];
    assert_eq!(
        (o.root.as_deref(), o.language.as_deref(), o.role),
        (Some("services/orders"), Some("go"), ServiceRole::Code)
    );
    assert_eq!(o.entry_points, vec!["main.go"]);
    assert_eq!(o.evidence.line, Some(5));
    let redis = &s["redis"];
    assert_eq!(
        (redis.root.as_deref(), redis.role, redis.image.as_deref()),
        (None, ServiceRole::Infrastructure, Some("redis:7-alpine"))
    );
    assert_eq!(
        (redis.evidence.file.as_str(), redis.evidence.line),
        ("docker-compose.yml", Some(9))
    );
}

#[test]
fn compose_wins_over_monorepo_so_undeclared_directory_is_not_a_service() {
    let r = discover("compose-app");
    assert!(!r.services.iter().any(|s| s.name == "legacy"));
    assert_eq!(r.attempted.len(), 1);
    assert_eq!(
        (r.attempted[0].strategy, r.attempted[0].services),
        (DiscoveryStrategy::DockerCompose, 2)
    );
}

#[test]
fn compose_matches_image_only_services_to_directories() {
    let r = discover("compose-images-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::DockerCompose));
    let s = by_name(&r.services);
    let c = &s["customers-service"];
    assert_eq!(
        c.root.as_deref(),
        Some("spring-petclinic-customers-service")
    );
    assert_eq!(c.language.as_deref(), Some("java"));
    assert_eq!(
        c.image.as_deref(),
        Some("springcommunity/spring-petclinic-customers-service:3.2.0")
    );
    assert_eq!(
        c.evidence.detail.as_deref(),
        Some("image: springcommunity/spring-petclinic-customers-service:3.2.0")
    );
    assert_eq!(
        s["vets-service"].root.as_deref(),
        Some("spring-petclinic-vets-service")
    );
    assert_eq!(
        s["config-server"].root.as_deref(),
        Some("spring-petclinic-config-server")
    );
    let t = &s["tracing-server"];
    assert_eq!(
        (t.root.as_deref(), t.role, t.image.as_deref()),
        (None, ServiceRole::Infrastructure, Some("openzipkin/zipkin"))
    );
}

#[test]
fn compose_uses_dockerfile_directory_when_context_is_root() {
    let r = discover("compose-rootctx-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::DockerCompose));
    let s = by_name(&r.services);
    let a = &s["accounting"];
    assert_eq!(
        (a.root.as_deref(), a.language.as_deref(), a.role),
        (Some("src/accounting"), Some("go"), ServiceRole::Code)
    );
    assert_eq!(
        a.evidence.detail.as_deref(),
        Some("build: ./, dockerfile: ./src/accounting/Dockerfile")
    );
    let f = &s["frontend"];
    assert_eq!(
        (
            f.root.as_deref(),
            f.language.as_deref(),
            f.package_name.as_deref()
        ),
        (Some("src/frontend"), Some("javascript"), Some("frontend"))
    );
    assert_eq!(
        (s["kafka"].root.as_deref(), s["kafka"].role),
        (None, ServiceRole::Infrastructure)
    );
}

#[test]
fn compose_resolves_env_references() {
    let r = discover("compose-env-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::DockerCompose));
    let s = by_name(&r.services);
    let ad = &s["ad"];
    assert_eq!(
        (
            ad.root.as_deref(),
            ad.language.as_deref(),
            ad.image.as_deref()
        ),
        (Some("src/ad"), Some("java"), Some("ghcr.io/demo:1.0-ad"))
    );
    assert_eq!(
        ad.evidence.detail.as_deref(),
        Some("build: ./, dockerfile: ./src/ad/Dockerfile")
    );
    // CART_DOCKERFILE is not in .env: the service name still finds src/cart.
    let cart = &s["cart"];
    assert_eq!(
        (cart.root.as_deref(), cart.language.as_deref()),
        (Some("src/cart"), Some("go"))
    );
    assert_eq!(
        cart.evidence.detail.as_deref(),
        Some("build: ./, matched directory src/cart")
    );
    // An unresolvable image stays visible as written, not silently dropped.
    let db = &s["db"];
    assert_eq!(
        (db.root.as_deref(), db.role, db.image.as_deref()),
        (None, ServiceRole::Infrastructure, Some("${POSTGRES_IMAGE}"))
    );
}

#[test]
fn compose_keeps_deploy_only_images() {
    let r = discover("deploy-only-app");
    assert_eq!(r.strategy, None);
    assert_eq!(r.services.len(), 3);
    assert!(
        r.services
            .iter()
            .all(|s| s.role == ServiceRole::Infrastructure)
    );
    assert_eq!(
        (r.attempted[0].strategy, r.attempted[0].services),
        (DiscoveryStrategy::DockerCompose, 0)
    );
}

// ------------------------------------------------------------- kubernetes

#[test]
fn kubernetes_matches_workloads_to_directories() {
    let r = discover("k8s-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::Kubernetes));
    let s = by_name(&r.services);
    assert_eq!(
        s.keys().cloned().collect::<Vec<_>>(),
        vec!["cartservice", "frontend", "redis-cart"]
    );
    let f = &s["frontend"];
    assert_eq!(
        (
            f.root.as_deref(),
            f.language.as_deref(),
            f.role,
            f.image.as_deref()
        ),
        (
            Some("src/frontend"),
            Some("go"),
            ServiceRole::Code,
            Some("acme/frontend:1.0")
        )
    );
    // The Deployment beats the Service object of the same name.
    assert_eq!(
        f.evidence,
        Evidence {
            file: "kubernetes/frontend.yaml".into(),
            line: Some(9),
            detail: Some("Deployment, image: acme/frontend:1.0".into())
        }
    );
    assert_eq!(
        (
            s["cartservice"].root.as_deref(),
            s["cartservice"].language.as_deref()
        ),
        (Some("src/cartservice"), Some("csharp"))
    );
    assert_eq!(
        (
            s["redis-cart"].root.as_deref(),
            s["redis-cart"].role,
            s["redis-cart"].image.as_deref()
        ),
        (None, ServiceRole::Infrastructure, Some("redis:alpine"))
    );
}

#[test]
fn kubernetes_skips_helm_templates() {
    let r = discover("k8s-app");
    assert!(!r.services.iter().any(|s| s.name.contains("Values")));
}

// --------------------------------------------------------------- monorepo

#[test]
fn monorepo_takes_children_with_manifests() {
    let r = discover("monorepo-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::Monorepo));
    let s = by_name(&r.services);
    assert_eq!(
        s.keys().cloned().collect::<Vec<_>>(),
        vec!["api", "web", "worker"]
    );
    assert_eq!(
        (
            s["api"].root.as_deref(),
            s["api"].language.as_deref(),
            s["api"].entry_points.clone()
        ),
        (
            Some("services/api"),
            Some("typescript"),
            vec!["src/server.ts".to_string()]
        )
    );
    assert_eq!(
        (
            s["worker"].root.as_deref(),
            s["worker"].language.as_deref(),
            s["worker"].entry_points.clone()
        ),
        (
            Some("services/worker"),
            Some("python"),
            vec!["main.py".to_string()]
        )
    );
    assert_eq!(
        (s["web"].root.as_deref(), s["web"].language.as_deref()),
        (Some("apps/web"), Some("javascript"))
    );
    assert_eq!(
        s["api"].evidence,
        Evidence {
            file: "services/api/package.json".into(),
            line: None,
            detail: Some("package.json".into())
        }
    );
}

// -------------------------------------------------------------- workspace

#[test]
fn workspace_expands_pnpm_globs_and_negations() {
    let r = discover("workspace-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::Workspace));
    let s = by_name(&r.services);
    assert_eq!(
        s.keys().cloned().collect::<Vec<_>>(),
        vec!["billing", "gateway"]
    );
    assert_eq!(
        (
            s["billing"].root.as_deref(),
            s["billing"].language.as_deref(),
            s["billing"].package_name.as_deref()
        ),
        (
            Some("components/billing"),
            Some("typescript"),
            Some("@acme/billing")
        )
    );
    assert_eq!(
        s["gateway"].evidence,
        Evidence {
            file: "pnpm-workspace.yaml".into(),
            line: None,
            detail: Some("components/*".into())
        }
    );
    assert_eq!(
        r.attempted.iter().map(|a| a.services).collect::<Vec<_>>(),
        vec![0, 0, 0, 2]
    );
}

#[test]
fn workspace_reads_package_json_workspaces() {
    let r = discover("npm-workspace-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::Workspace));
    assert_eq!(
        r.services
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta"]
    );
    assert_eq!(r.services[0].evidence.file, "package.json");
}

// --------------------------------------------------------------- fallback

#[test]
fn single_service_fallback_reports_attempts() {
    let r = discover("single-app");
    assert_eq!(r.strategy, None);
    assert_eq!(
        r.attempted
            .iter()
            .map(|a| (a.strategy, a.services))
            .collect::<Vec<_>>(),
        vec![
            (DiscoveryStrategy::DockerCompose, 0),
            (DiscoveryStrategy::Kubernetes, 0),
            (DiscoveryStrategy::Monorepo, 0),
            (DiscoveryStrategy::Workspace, 0),
        ]
    );
    assert_eq!(r.services.len(), 1);
    let s = &r.services[0];
    assert_eq!(
        (
            s.name.as_str(),
            s.root.as_deref(),
            s.language.as_deref(),
            s.role,
            s.discovered_by
        ),
        (
            "single-app",
            Some("."),
            Some("typescript"),
            ServiceRole::Code,
            ServiceSource::Root
        )
    );
}
