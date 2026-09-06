//! Stage 2: static dependency mapping.
//!
//! `facts` turns one file into language-neutral facts. Matchers turn facts
//! into candidates naming what they found. The resolver turns candidates
//! into edges between discovered services.
pub mod facts;
