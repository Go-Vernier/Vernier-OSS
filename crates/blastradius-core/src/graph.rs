//! The single in-memory graph every stage writes to. Nodes are services,
//! keyed by name. Edges point from the dependent to the dependency: an edge
//! checkout -> orders means checkout calls orders, so a change to orders
//! reaches checkout by walking inbound edges.
use indexmap::IndexMap;
use thiserror::Error;

use crate::discover::sort_services;
use crate::model::{Edge, Service};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GraphError {
    #[error("Unknown service \"{name}\" on edge {from} -> {to}")]
    UnknownService {
        name: String,
        from: String,
        to: String,
    },
}

#[derive(Debug, Default, Clone)]
pub struct BlastGraph {
    services: IndexMap<String, Service>,
    edges: Vec<Edge>,
}

impl BlastGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a service, replacing one of the same name.
    pub fn add_service(&mut self, service: Service) {
        self.services.insert(service.name.clone(), service);
    }

    pub fn has_service(&self, name: &str) -> bool {
        self.services.contains_key(name)
    }

    pub fn service(&self, name: &str) -> Option<&Service> {
        self.services.get(name)
    }

    /// Both ends must be known services. Parallel edges of different types
    /// between the same pair are kept.
    pub fn add_edge(&mut self, edge: Edge) -> Result<(), GraphError> {
        for end in [&edge.source, &edge.target] {
            if !self.services.contains_key(end) {
                return Err(GraphError::UnknownService {
                    name: end.clone(),
                    from: edge.source.clone(),
                    to: edge.target.clone(),
                });
            }
        }
        self.edges.push(edge);
        Ok(())
    }

    /// Every service, code first, then by name.
    pub fn services(&self) -> Vec<Service> {
        let mut services: Vec<Service> = self.services.values().cloned().collect();
        sort_services(&mut services);
        services
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Who depends on this service: every edge whose target is `name`.
    pub fn inbound(&self, name: &str) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.target == name).collect()
    }

    /// What this service depends on: every edge whose source is `name`.
    pub fn outbound(&self, name: &str) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.source == name).collect()
    }

    /// (services, edges)
    pub fn size(&self) -> (usize, usize) {
        (self.services.len(), self.edges.len())
    }
}
