//! Cascade Detection - Detect cascading failures in federation
//!
//! Cascading failures occur when the failure of one node triggers
//! failures in dependent nodes, leading to system-wide collapse.

use std::collections::{HashMap, HashSet, VecDeque};

/// Node dependency graph
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    /// Map of node_id to nodes it depends on
    dependencies: HashMap<String, Vec<String>>,
    /// Map of node_id to nodes that depend on it
    dependents: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            dependencies: HashMap::new(),
            dependents: HashMap::new(),
        }
    }

    /// Add a dependency: node depends on dependency
    pub fn add_dependency(&mut self, node: String, dependency: String) {
        self.dependencies
            .entry(node.clone())
            .or_default()
            .push(dependency.clone());

        self.dependents.entry(dependency).or_default().push(node);
    }

    /// Get all direct and indirect dependents of a node
    pub fn get_all_dependents(&self, node_id: &str) -> HashSet<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        queue.push_back(node_id.to_string());

        while let Some(current) = queue.pop_front() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());

            if let Some(dependents) = self.dependents.get(&current) {
                for dependent in dependents {
                    if !visited.contains(dependent) {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }

        visited.remove(node_id); // Remove the original node
        visited
    }

    /// Calculate cascade risk score for a node
    /// Higher score = more nodes affected if this node fails
    pub fn calculate_cascade_risk(&self, node_id: &str) -> f64 {
        let all_dependents = self.get_all_dependents(node_id);
        let total_nodes = self.dependencies.len().max(self.dependents.len());

        if total_nodes == 0 {
            return 0.0;
        }

        all_dependents.len() as f64 / total_nodes as f64
    }

    /// Find all nodes sorted by cascade risk (highest first)
    pub fn nodes_by_cascade_risk(&self) -> Vec<(String, f64)> {
        let all_nodes: HashSet<String> = self
            .dependencies
            .keys()
            .chain(self.dependents.keys())
            .cloned()
            .collect();

        let mut risks: Vec<(String, f64)> = all_nodes
            .iter()
            .map(|node| (node.clone(), self.calculate_cascade_risk(node)))
            .collect();

        risks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        risks
    }

    /// Detect cycles in the dependency graph
    pub fn detect_cycles(&self) -> Vec<Vec<String>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut recursion_stack = Vec::new();

        for node in self.dependencies.keys() {
            if !visited.contains(node) {
                self.dfs_detect_cycles(node, &mut visited, &mut recursion_stack, &mut cycles);
            }
        }

        cycles
    }

    fn dfs_detect_cycles(
        &self,
        node: &str,
        visited: &mut HashSet<String>,
        recursion_stack: &mut Vec<String>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        visited.insert(node.to_string());
        recursion_stack.push(node.to_string());

        if let Some(dependencies) = self.dependencies.get(node) {
            for dependency in dependencies {
                if !visited.contains(dependency) {
                    self.dfs_detect_cycles(dependency, visited, recursion_stack, cycles);
                } else if recursion_stack.contains(dependency) {
                    // Found a cycle
                    let cycle_start = recursion_stack
                        .iter()
                        .position(|n| n == dependency)
                        .unwrap();
                    let cycle: Vec<String> = recursion_stack[cycle_start..].to_vec();
                    cycles.push(cycle);
                }
            }
        }

        recursion_stack.pop();
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Cascade failure simulation result
#[derive(Debug, Clone)]
pub struct CascadeSimulation {
    /// Initial failed node
    pub initial_failure: String,
    /// All nodes that would fail
    pub cascading_failures: Vec<String>,
    /// Sequence of failures
    pub failure_sequence: Vec<(String, u32)>, // (node_id, step)
    /// Total affected percentage
    pub affected_percentage: f64,
    /// Whether it causes system collapse (>50% nodes)
    pub is_systemic: bool,
}

/// Simulate a cascade failure starting from a node
pub fn simulate_cascade(
    graph: &DependencyGraph,
    initial_node: &str,
    failure_probability: f64,
) -> CascadeSimulation {
    let mut failed = HashSet::new();
    let mut sequence = Vec::new();
    let mut step = 0u32;

    // Initial failure
    failed.insert(initial_node.to_string());
    sequence.push((initial_node.to_string(), step));

    let mut newly_failed = vec![initial_node.to_string()];

    while !newly_failed.is_empty() {
        step += 1;
        let mut next_wave = Vec::new();

        for failed_node in &newly_failed {
            // Find dependents
            let dependents = graph.get_all_dependents(failed_node);

            for dependent in dependents {
                if !failed.contains(&dependent) {
                    // Check if all dependencies of this node have failed
                    let all_deps_failed = graph
                        .dependencies
                        .get(&dependent)
                        .map(|deps| deps.iter().all(|d| failed.contains(d)))
                        .unwrap_or(true);

                    // Probabilistic failure
                    if all_deps_failed || rand::random::<f64>() < failure_probability {
                        failed.insert(dependent.clone());
                        sequence.push((dependent.clone(), step));
                        next_wave.push(dependent);
                    }
                }
            }
        }

        newly_failed = next_wave;
    }

    let total_nodes = graph.dependencies.len().max(graph.dependents.len()).max(1);
    let affected_percentage = failed.len() as f64 / total_nodes as f64;

    CascadeSimulation {
        initial_failure: initial_node.to_string(),
        cascading_failures: failed.iter().cloned().collect(),
        failure_sequence: sequence,
        affected_percentage,
        is_systemic: affected_percentage > 0.5,
    }
}

/// Cascade risk assessment for the entire federation
#[derive(Debug, Clone)]
pub struct CascadeRiskAssessment {
    /// Overall cascade risk score (0.0 - 1.0)
    pub overall_risk: f64,
    /// Highest risk nodes
    pub high_risk_nodes: Vec<(String, f64)>,
    /// Detected cycles
    pub cycles: Vec<Vec<String>>,
    /// Single points of failure
    pub single_points_of_failure: Vec<String>,
    /// Worst-case scenario
    pub worst_case_scenario: Option<CascadeSimulation>,
    /// Recommendations
    pub recommendations: Vec<String>,
}

/// Assess cascade risk for the federation
pub fn assess_cascade_risk(graph: &DependencyGraph) -> CascadeRiskAssessment {
    // Get nodes by risk
    let nodes_by_risk = graph.nodes_by_cascade_risk();

    // Find high risk nodes (top 20% or risk > 0.3)
    let threshold = (nodes_by_risk.len() as f64 * 0.2) as usize;
    let high_risk_nodes: Vec<(String, f64)> = nodes_by_risk
        .iter()
        .take(threshold.max(1))
        .filter(|(_, risk)| *risk > 0.0)
        .cloned()
        .collect();

    // Detect cycles
    let cycles = graph.detect_cycles();

    // Find single points of failure (nodes with >0.5 cascade risk)
    let single_points: Vec<String> = nodes_by_risk
        .iter()
        .filter(|(_, risk)| *risk > 0.5)
        .map(|(node, _)| node.clone())
        .collect();

    // Simulate worst case
    let worst_case = if let Some((highest_risk_node, _)) = nodes_by_risk.first() {
        Some(simulate_cascade(graph, highest_risk_node, 1.0))
    } else {
        None
    };

    // Calculate overall risk
    let overall_risk = if nodes_by_risk.is_empty() {
        0.0
    } else {
        let total_risk: f64 = nodes_by_risk.iter().map(|(_, r)| r).sum();
        total_risk / nodes_by_risk.len() as f64
    };

    // Generate recommendations
    let mut recommendations = Vec::new();

    if !single_points.is_empty() {
        recommendations.push(format!(
            "Critical: Found {} single points of failure - add redundancy immediately",
            single_points.len()
        ));
    }

    if !cycles.is_empty() {
        recommendations.push(format!(
            "Warning: Found {} dependency cycles - may cause deadlock",
            cycles.len()
        ));
    }

    if overall_risk > 0.3 {
        recommendations
            .push("High cascade risk detected - review dependency structure".to_string());
    }

    if let Some(ref worst) = worst_case
        && worst.is_systemic
    {
        recommendations.push(format!(
            "CRITICAL: Failure of {} could cause system-wide collapse ({:.1}% affected)",
            worst.initial_failure,
            worst.affected_percentage * 100.0
        ));
    }

    CascadeRiskAssessment {
        overall_risk,
        high_risk_nodes,
        cycles,
        single_points_of_failure: single_points,
        worst_case_scenario: worst_case,
        recommendations,
    }
}

/// Load-based cascade detection
#[derive(Debug, Clone)]
pub struct LoadCascadeDetector {
    /// Node capacity map
    capacities: HashMap<String, f64>,
    /// Current load map
    loads: HashMap<String, f64>,
    /// Load redistribution factor (how much load shifts when node fails)
    redistribution_factor: f64,
}

impl LoadCascadeDetector {
    pub fn new() -> Self {
        Self {
            capacities: HashMap::new(),
            loads: HashMap::new(),
            redistribution_factor: 1.2, // 20% overhead from redistribution
        }
    }

    pub fn with_redistribution_factor(mut self, factor: f64) -> Self {
        self.redistribution_factor = factor;
        self
    }

    pub fn set_capacity(&mut self, node_id: String, capacity: f64) {
        self.capacities.insert(node_id, capacity);
    }

    pub fn set_load(&mut self, node_id: String, load: f64) {
        self.loads.insert(node_id, load);
    }

    /// Check if a node is overloaded
    pub fn is_overloaded(&self, node_id: &str) -> bool {
        let capacity = self.capacities.get(node_id).copied().unwrap_or(f64::MAX);
        let load = self.loads.get(node_id).copied().unwrap_or(0.0);
        load > capacity
    }

    /// Simulate load cascade starting from failed node
    pub fn simulate_load_cascade(
        &self,
        _graph: &DependencyGraph,
        initial_failure: &str,
    ) -> Vec<String> {
        let mut failed = HashSet::new();
        let mut new_loads = self.loads.clone();

        failed.insert(initial_failure.to_string());

        let mut changed = true;
        while changed {
            changed = false;

            // Get active nodes
            let active_nodes: Vec<String> = self
                .capacities
                .keys()
                .filter(|n| !failed.contains(*n))
                .cloned()
                .collect();

            // Redistribute load from failed nodes
            let failed_load: f64 = failed.iter().filter_map(|n| self.loads.get(n)).sum();

            if failed_load > 0.0 && !active_nodes.is_empty() {
                let additional_load =
                    failed_load * self.redistribution_factor / active_nodes.len() as f64;

                for node in &active_nodes {
                    let current = new_loads.get(node).copied().unwrap_or(0.0);
                    new_loads.insert(node.clone(), current + additional_load);
                }
            }

            // Check for new overloads
            for node in &active_nodes {
                let capacity = self.capacities.get(node).copied().unwrap_or(f64::MAX);
                let load = new_loads.get(node).copied().unwrap_or(0.0);

                if load > capacity && !failed.contains(node) {
                    failed.insert(node.clone());
                    changed = true;
                }
            }
        }

        failed.iter().cloned().collect()
    }

    /// Get current utilization for all nodes
    pub fn get_utilizations(&self) -> HashMap<String, f64> {
        let mut result = HashMap::new();

        for (node, capacity) in &self.capacities {
            let load = self.loads.get(node).copied().unwrap_or(0.0);
            result.insert(node.clone(), load / capacity);
        }

        result
    }
}

impl Default for LoadCascadeDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_graph() {
        let mut graph = DependencyGraph::new();

        // A depends on B, B depends on C
        graph.add_dependency("A".to_string(), "B".to_string());
        graph.add_dependency("B".to_string(), "C".to_string());

        let dependents_of_c = graph.get_all_dependents("C");
        assert!(dependents_of_c.contains("A"));
        assert!(dependents_of_c.contains("B"));
        assert!(!dependents_of_c.contains("C"));
    }

    #[test]
    fn test_cascade_risk() {
        let mut graph = DependencyGraph::new();

        // Star pattern: many nodes depend on center
        graph.add_dependency("A".to_string(), "CENTER".to_string());
        graph.add_dependency("B".to_string(), "CENTER".to_string());
        graph.add_dependency("C".to_string(), "CENTER".to_string());

        let center_risk = graph.calculate_cascade_risk("CENTER");
        assert!(center_risk > 0.5); // Center should have high risk

        let leaf_risk = graph.calculate_cascade_risk("A");
        assert!(leaf_risk < 0.3); // Leaves should have low risk
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = DependencyGraph::new();

        // A -> B -> C -> A (cycle)
        graph.add_dependency("A".to_string(), "B".to_string());
        graph.add_dependency("B".to_string(), "C".to_string());
        graph.add_dependency("C".to_string(), "A".to_string());

        let cycles = graph.detect_cycles();
        assert!(!cycles.is_empty());
    }

    #[test]
    fn test_cascade_simulation() {
        let mut graph = DependencyGraph::new();

        graph.add_dependency("A".to_string(), "B".to_string());
        graph.add_dependency("B".to_string(), "C".to_string());

        let sim = simulate_cascade(&graph, "C", 1.0);

        assert!(sim.cascading_failures.contains(&"B".to_string()));
        assert!(sim.cascading_failures.contains(&"A".to_string()));
        assert!(sim.affected_percentage > 0.5);
    }

    #[test]
    fn test_cascade_risk_assessment() {
        let mut graph = DependencyGraph::new();

        // Create a hub with many dependencies
        for i in 0..5 {
            graph.add_dependency(format!("NODE{}", i), "HUB".to_string());
        }

        let assessment = assess_cascade_risk(&graph);

        assert!(assessment.overall_risk > 0.0);
        assert!(!assessment.high_risk_nodes.is_empty());
        assert!(
            assessment
                .single_points_of_failure
                .contains(&"HUB".to_string())
        );
    }

    #[test]
    fn test_load_cascade_detector() {
        let mut detector = LoadCascadeDetector::new();

        detector.set_capacity("node1".to_string(), 100.0);
        detector.set_capacity("node2".to_string(), 100.0);
        detector.set_load("node1".to_string(), 50.0);
        detector.set_load("node2".to_string(), 50.0);

        assert!(!detector.is_overloaded("node1"));

        detector.set_load("node1".to_string(), 150.0);
        assert!(detector.is_overloaded("node1"));
    }

    #[test]
    fn test_load_cascade_simulation() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("node2".to_string(), "node1".to_string());

        let mut detector = LoadCascadeDetector::new();
        detector.set_capacity("node1".to_string(), 100.0);
        detector.set_capacity("node2".to_string(), 100.0);
        detector.set_load("node1".to_string(), 80.0);
        detector.set_load("node2".to_string(), 80.0);

        let failed = detector.simulate_load_cascade(&graph, "node1");

        assert!(failed.contains(&"node1".to_string()));
        // node2 may or may not fail depending on load redistribution
    }

    #[test]
    fn test_utilization() {
        let mut detector = LoadCascadeDetector::new();

        detector.set_capacity("node1".to_string(), 100.0);
        detector.set_load("node1".to_string(), 75.0);

        let utils = detector.get_utilizations();
        assert!((utils.get("node1").unwrap() - 0.75).abs() < 0.01);
    }
}
