use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedNode {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub domains: Vec<String>,
    pub expertise_scores: HashMap<String, f64>,
    pub health_status: NodeHealth,
    pub last_heartbeat: DateTime<Utc>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeHealth {
    Healthy,
    Degraded { reason: String },
    Unhealthy { reason: String },
    Unknown,
}

impl FederatedNode {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            endpoint: endpoint.into(),
            domains: Vec::new(),
            expertise_scores: HashMap::new(),
            health_status: NodeHealth::Unknown,
            last_heartbeat: Utc::now(),
            metadata: HashMap::new(),
            priority: 0,
        }
    }

    pub fn with_domains(mut self, domains: Vec<String>) -> Self {
        self.domains = domains;
        self
    }

    pub fn with_expertise(mut self, domain: impl Into<String>, score: f64) -> Self {
        self.expertise_scores
            .insert(domain.into(), score.clamp(0.0, 1.0));
        self
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self.health_status, NodeHealth::Healthy)
    }

    pub fn expertise_for(&self, domain: &str) -> f64 {
        self.expertise_scores.get(domain).copied().unwrap_or(0.0)
    }

    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat = Utc::now();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node_id: String,
    pub online: bool,
    pub response_time_ms: u64,
    pub active_queries: usize,
    pub error_rate: f64,
    pub last_check: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NodeRegistry {
    nodes: HashMap<String, FederatedNode>,
    status: HashMap<String, NodeStatus>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, node: FederatedNode) {
        let node_id = node.id.clone();
        let status_node_id = node_id.clone();
        self.nodes.insert(node_id.clone(), node);
        self.status.insert(
            node_id,
            NodeStatus {
                node_id: status_node_id,
                online: true,
                response_time_ms: 0,
                active_queries: 0,
                error_rate: 0.0,
                last_check: Utc::now(),
            },
        );
    }

    pub fn unregister(&mut self, node_id: &str) -> Option<FederatedNode> {
        self.status.remove(node_id);
        self.nodes.remove(node_id)
    }

    pub fn get(&self, node_id: &str) -> Option<&FederatedNode> {
        self.nodes.get(node_id)
    }

    pub fn get_mut(&mut self, node_id: &str) -> Option<&mut FederatedNode> {
        self.nodes.get_mut(node_id)
    }

    pub fn list(&self) -> Vec<&FederatedNode> {
        self.nodes.values().collect()
    }

    pub fn list_healthy(&self) -> Vec<&FederatedNode> {
        self.nodes.values().filter(|n| n.is_healthy()).collect()
    }

    pub fn find_by_domain(&self, domain: &str) -> Vec<&FederatedNode> {
        self.nodes
            .values()
            .filter(|n| n.domains.iter().any(|d| d.eq_ignore_ascii_case(domain)))
            .collect()
    }

    pub fn update_status(&mut self, status: NodeStatus) {
        self.status.insert(status.node_id.clone(), status);
    }

    pub fn get_status(&self, node_id: &str) -> Option<&NodeStatus> {
        self.status.get(node_id)
    }

    pub fn healthy_nodes_for_domain(&self, domain: &str) -> Vec<&FederatedNode> {
        self.nodes
            .values()
            .filter(|n| n.is_healthy() && n.domains.iter().any(|d| d.eq_ignore_ascii_case(domain)))
            .collect()
    }
}
