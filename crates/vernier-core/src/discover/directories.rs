//! Matching a declared service to the directory holding its code, when the
//! declaration does not say: `cartservice` -> `src/cartservice`, or the image
//! `springcommunity/spring-petclinic-vets-service` ->
//! `spring-petclinic-vets-service`.
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::language::detect_manifest;
use crate::fs::FileIndex;

/// Every directory up to four levels deep, shallower first. Tiers, strongest
/// first: exact basename, normalised basename, then a basename that ends
/// with the name after a separator. Only directories with a manifest count.
#[derive(Debug)]
pub struct DirectoryIndex<'a> {
    root: &'a Path,
    dirs: Vec<String>,
}

impl<'a> DirectoryIndex<'a> {
    pub fn build(index: &'a FileIndex) -> Self {
        let mut dirs = index.dirs_up_to_depth(4);
        dirs.sort_by(|a, b| depth(a).cmp(&depth(b)).then_with(|| a.cmp(b)));
        Self {
            root: index.root(),
            dirs,
        }
    }

    /// Repository-relative directory for the first name that matches.
    pub fn matching(&self, names: &[Option<&str>]) -> Option<String> {
        let mut wanted: Vec<&str> = Vec::new();
        for name in names.iter().flatten() {
            if !name.is_empty() && !wanted.contains(name) {
                wanted.push(name);
            }
        }
        let mut tiers: [Vec<&String>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for dir in &self.dirs {
            let base = dir.rsplit('/').next().unwrap_or(dir).to_lowercase();
            for name in &wanted {
                let lower = name.to_lowercase();
                if base == lower {
                    tiers[0].push(dir);
                } else if !normalise(name).is_empty() && normalise(&base) == normalise(name) {
                    tiers[1].push(dir);
                } else if lower.len() >= 4 && ends_with_after_separator(&base, &lower) {
                    tiers[2].push(dir);
                }
            }
        }
        tiers
            .iter()
            .flatten()
            .find(|dir| detect_manifest(&self.root.join(dir)).is_some())
            .map(|dir| (*dir).clone())
    }
}

fn ends_with_after_separator(base: &str, suffix: &str) -> bool {
    base.len() > suffix.len()
        && base.ends_with(suffix)
        && base[..base.len() - suffix.len()]
            .chars()
            .next_back()
            .is_some_and(|c| matches!(c, '-' | '_' | '.'))
}

fn depth(p: &str) -> usize {
    p.split('/').count()
}

static SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(service|svc|api|server|deployment|deploy)$").unwrap());

/// `checkout-api`, `checkout_svc`, `CheckoutService`, `Checkout.API` all
/// become `checkout`.
pub fn normalise(s: &str) -> String {
    let compact: String = s
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | '.'))
        .collect();
    SUFFIX.replace(&compact, "").into_owned()
}

/// `gcr.io/demo/cartservice:v1` -> `cartservice`; `redis:7-alpine` -> `redis`.
pub fn image_basename(image: Option<&str>) -> Option<String> {
    let image = image?;
    let last = image.rsplit('/').next().unwrap_or(image);
    let name = last.split('@').next().unwrap_or(last);
    let name = name.split(':').next().unwrap_or(name);
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FileIndex;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn fixture(p: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures")
            .join(p)
            .canonicalize()
            .unwrap()
    }

    #[test]
    fn normalises_serviceish_suffixes() {
        assert_eq!(normalise("checkout-api"), "checkout");
        assert_eq!(normalise("CheckoutService"), "checkout");
        assert_eq!(normalise("checkout_svc"), "checkout");
        assert_eq!(normalise("redis-cart"), "rediscart");
        assert_eq!(normalise("Basket.API"), "basket");
    }

    #[test]
    fn image_basename_strips_registry_tag_digest() {
        assert_eq!(
            image_basename(Some(
                "gcr.io/google-samples/microservices-demo/cartservice:v0.10.0"
            ))
            .as_deref(),
            Some("cartservice")
        );
        assert_eq!(
            image_basename(Some("redis:7-alpine")).as_deref(),
            Some("redis")
        );
        assert_eq!(
            image_basename(Some("ghcr.io/acme/api@sha256:abcdef")).as_deref(),
            Some("api")
        );
        assert_eq!(image_basename(None), None);
    }

    #[test]
    fn matches_by_exact_then_normalised_then_suffix_and_needs_a_manifest() {
        let ix = FileIndex::build(&fixture("compose-images-app"));
        let d = DirectoryIndex::build(&ix);
        assert_eq!(
            d.matching(&[Some("customers-service"), None]).as_deref(),
            Some("spring-petclinic-customers-service")
        );
        assert_eq!(d.matching(&[Some("tracing-server"), Some("zipkin")]), None);
        let ix = FileIndex::build(&fixture("k8s-app"));
        let d = DirectoryIndex::build(&ix);
        assert_eq!(
            d.matching(&[Some("cartservice"), None]).as_deref(),
            Some("src/cartservice")
        );
        assert_eq!(
            d.matching(&[None, Some("frontend")]).as_deref(),
            Some("src/frontend")
        );
        assert_eq!(d.matching(&[None, None]), None);
    }
}
