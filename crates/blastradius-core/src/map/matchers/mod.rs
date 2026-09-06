//! Matchers turn the facts of one file into candidates. Each names what it
//! found; the resolver decides which service that is.
pub mod grpc;
pub mod http;

use super::Candidate;
use super::config::ConfigIndex;
use super::facts::Fact;

pub struct FileContext<'a> {
    pub service: &'a str,
    pub file: &'a str,
    pub facts: &'a [Fact],
    pub config: &'a ConfigIndex,
}

pub trait Matcher: Sync + Send {
    fn name(&self) -> &'static str;
    fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate>;
}

pub fn all() -> Vec<Box<dyn Matcher>> {
    vec![Box::new(http::Http), Box::new(grpc::Grpc)]
}
