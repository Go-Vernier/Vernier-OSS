//! Matchers turn the facts of one file into candidates. Each names what it
//! found; the resolver decides which service that is.
pub mod database;
pub mod event;
pub mod grpc;
pub mod http;
pub mod import;

use super::Candidate;
use super::config::ConfigIndex;
use super::facts::Fact;
use super::symbols::Symbols;

pub struct FileContext<'a> {
    pub service: &'a str,
    pub file: &'a str,
    pub facts: &'a [Fact],
    pub config: &'a ConfigIndex,
    pub symbols: &'a Symbols,
}

pub trait Matcher: Sync + Send {
    fn name(&self) -> &'static str;
    fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate>;
}

pub fn all() -> Vec<Box<dyn Matcher>> {
    vec![
        Box::new(http::Http),
        Box::new(grpc::Grpc),
        Box::new(event::Event),
        Box::new(database::Database),
        Box::new(import::Import),
    ]
}
