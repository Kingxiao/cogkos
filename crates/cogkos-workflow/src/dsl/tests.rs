use super::*;
use std::collections::HashMap;

// ── Helper ──────────────────────────────────────────────────────

fn minimal_workflow(nodes: Vec<NodeDefinition>, edges: Vec<EdgeDefinition>) -> WorkflowDefinition {
    WorkflowDefinition {
        id: "test".to_string(),
        name: "test_workflow".to_string(),
        version: "1.0".to_string(),
        description: None,
        nodes,
        edges,
        variables: HashMap::new(),
        timeout_seconds: None,
        retry_policy: None,
    }
}

fn task_node(id: &str) -> NodeDefinition {
    NodeDefinition {
        id: id.to_string(),
        node_type: NodeType::Task,
        name: None,
        description: None,
        config: serde_json::Value::Object(Default::default()),
        retry_policy: None,
        timeout_seconds: None,
        condition: None,
    }
}

fn seq_edge(from: &str, to: &str) -> EdgeDefinition {
    EdgeDefinition {
        from: from.to_string(),
        to: to.to_string(),
        edge_type: EdgeType::Sequential,
        condition: None,
    }
}

// ── JSON parsing ────────────────────────────────────────────────

#[test]
fn parse_json_valid_workflow() {
    let json = r#"{
        "id": "w1",
        "name": "demo",
        "version": "1.0",
        "nodes": [
            {"id": "a", "node_type": "task", "config": {}},
            {"id": "b", "node_type": "task", "config": {}}
        ],
        "edges": [
            {"from": "a", "to": "b", "edge_type": "sequential"}
        ],
        "variables": {}
    }"#;
    let parser = WorkflowParser::new();
    let wf = parser.parse_json(json).unwrap();
    assert_eq!(wf.name, "demo");
    assert_eq!(wf.nodes.len(), 2);
    assert_eq!(wf.edges.len(), 1);
}

#[test]
fn parse_json_invalid_syntax() {
    let parser = WorkflowParser::new();
    let err = parser.parse_json("{ not valid json }").unwrap_err();
    match err {
        crate::WorkflowError::DslError(msg) => assert!(msg.contains("JSON parse error")),
        other => panic!("expected DslError, got {:?}", other),
    }
}

#[test]
fn parse_json_missing_required_field() {
    // missing "version"
    let json = r#"{"id": "w1", "name": "demo", "nodes": [], "edges": [], "variables": {}}"#;
    let parser = WorkflowParser::new();
    assert!(parser.parse_json(json).is_err());
}

// ── YAML parsing ────────────────────────────────────────────────

#[test]
fn parse_yaml_valid_workflow() {
    let yaml = r#"
id: w1
name: demo
version: "1.0"
nodes:
  - id: a
    node_type: task
    config: {}
  - id: b
    node_type: task
    config: {}
edges:
  - from: a
    to: b
    edge_type: sequential
variables: {}
"#;
    let parser = WorkflowParser::new();
    let wf = parser.parse_yaml(yaml).unwrap();
    assert_eq!(wf.name, "demo");
    assert_eq!(wf.nodes.len(), 2);
}

#[test]
fn parse_yaml_invalid() {
    let parser = WorkflowParser::new();
    let err = parser.parse_yaml(":::bad yaml[[[").unwrap_err();
    match err {
        crate::WorkflowError::DslError(msg) => assert!(msg.contains("YAML parse error")),
        other => panic!("expected DslError, got {:?}", other),
    }
}

// ── Custom DSL parsing ──────────────────────────────────────────

#[test]
fn parse_dsl_basic() {
    let dsl = r#"
workflow "my_flow" {
  node "step1" { type: task }
  node "step2" { type: condition }
  edge "step1" -> "step2"
}
"#;
    let parser = WorkflowParser::new();
    let wf = parser.parse_dsl(dsl).unwrap();
    assert_eq!(wf.name, "my_flow");
    assert_eq!(wf.nodes.len(), 2);
    assert_eq!(wf.nodes[0].id, "step1");
    assert_eq!(wf.nodes[0].node_type, NodeType::Task);
    assert_eq!(wf.nodes[1].id, "step2");
    assert_eq!(wf.nodes[1].node_type, NodeType::Condition);
    assert_eq!(wf.edges.len(), 1);
    assert_eq!(wf.edges[0].from, "step1");
    assert_eq!(wf.edges[0].to, "step2");
    assert_eq!(wf.edges[0].edge_type, EdgeType::Sequential);
}

#[test]
fn parse_dsl_all_node_types() {
    let dsl = r#"
workflow "types" {
  node "n1" { type: task }
  node "n2" { type: condition }
  node "n3" { type: parallel }
  node "n4" { type: wait }
  node "n5" { type: ab_test }
  node "n6" { type: insight }
  node "n7" { type: conflict }
  node "n8" { type: unknown_type }
}
"#;
    let parser = WorkflowParser::new();
    let wf = parser.parse_dsl(dsl).unwrap();
    assert_eq!(wf.nodes.len(), 8);
    assert_eq!(wf.nodes[0].node_type, NodeType::Task);
    assert_eq!(wf.nodes[1].node_type, NodeType::Condition);
    assert_eq!(wf.nodes[2].node_type, NodeType::Parallel);
    assert_eq!(wf.nodes[3].node_type, NodeType::Wait);
    assert_eq!(wf.nodes[4].node_type, NodeType::AbTest);
    assert_eq!(wf.nodes[5].node_type, NodeType::InsightExtract);
    assert_eq!(wf.nodes[6].node_type, NodeType::ConflictDetect);
    // unknown defaults to Task
    assert_eq!(wf.nodes[7].node_type, NodeType::Task);
}

#[test]
fn parse_dsl_no_workflow_name_uses_default() {
    let dsl = r#"
  node "only" { type: task }
"#;
    let parser = WorkflowParser::new();
    let wf = parser.parse_dsl(dsl).unwrap();
    assert_eq!(wf.name, "parsed_workflow");
}

#[test]
fn parse_dsl_empty_is_valid() {
    let parser = WorkflowParser::new();
    let wf = parser.parse_dsl("").unwrap();
    assert!(wf.nodes.is_empty());
    assert!(wf.edges.is_empty());
}

#[test]
fn parse_dsl_multiple_edges() {
    let dsl = r#"
workflow "multi" {
  node "a" { type: task }
  node "b" { type: task }
  node "c" { type: task }
  edge "a" -> "b"
  edge "a" -> "c"
  edge "b" -> "c"
}
"#;
    let parser = WorkflowParser::new();
    let wf = parser.parse_dsl(dsl).unwrap();
    assert_eq!(wf.edges.len(), 3);
}

// ── Validation: duplicate node IDs ──────────────────────────────

#[test]
fn validate_duplicate_node_ids() {
    let wf = minimal_workflow(vec![task_node("dup"), task_node("dup")], vec![]);
    let parser = WorkflowParser::new();
    let err = parser.validate(&wf).unwrap_err();
    match err {
        crate::WorkflowError::InvalidDefinition(msg) => {
            assert!(msg.contains("Duplicate node ID: dup"))
        }
        other => panic!("expected InvalidDefinition, got {:?}", other),
    }
}

// ── Validation: dangling edges ──────────────────────────────────

#[test]
fn validate_edge_from_nonexistent_node() {
    let wf = minimal_workflow(vec![task_node("a")], vec![seq_edge("ghost", "a")]);
    let parser = WorkflowParser::new();
    let err = parser.validate(&wf).unwrap_err();
    match err {
        crate::WorkflowError::InvalidDefinition(msg) => {
            assert!(msg.contains("non-existent node: ghost"));
        }
        other => panic!("expected InvalidDefinition, got {:?}", other),
    }
}

#[test]
fn validate_edge_to_nonexistent_node() {
    let wf = minimal_workflow(vec![task_node("a")], vec![seq_edge("a", "ghost")]);
    let parser = WorkflowParser::new();
    let err = parser.validate(&wf).unwrap_err();
    match err {
        crate::WorkflowError::InvalidDefinition(msg) => {
            assert!(msg.contains("non-existent node: ghost"));
        }
        other => panic!("expected InvalidDefinition, got {:?}", other),
    }
}

// ── Validation: cycle detection ─────────────────────────────────

#[test]
fn validate_cycle_two_nodes() {
    let wf = minimal_workflow(
        vec![task_node("a"), task_node("b")],
        vec![seq_edge("a", "b"), seq_edge("b", "a")],
    );
    let parser = WorkflowParser::new();
    let err = parser.validate(&wf).unwrap_err();
    match err {
        crate::WorkflowError::InvalidDefinition(msg) => assert!(msg.contains("cycles")),
        other => panic!("expected InvalidDefinition (cycles), got {:?}", other),
    }
}

#[test]
fn validate_cycle_three_nodes() {
    let wf = minimal_workflow(
        vec![task_node("a"), task_node("b"), task_node("c")],
        vec![seq_edge("a", "b"), seq_edge("b", "c"), seq_edge("c", "a")],
    );
    let parser = WorkflowParser::new();
    let err = parser.validate(&wf).unwrap_err();
    match err {
        crate::WorkflowError::InvalidDefinition(msg) => assert!(msg.contains("cycles")),
        other => panic!("expected InvalidDefinition (cycles), got {:?}", other),
    }
}

#[test]
fn validate_self_loop() {
    let wf = minimal_workflow(vec![task_node("a")], vec![seq_edge("a", "a")]);
    let parser = WorkflowParser::new();
    let err = parser.validate(&wf).unwrap_err();
    match err {
        crate::WorkflowError::InvalidDefinition(msg) => assert!(msg.contains("cycles")),
        other => panic!("expected InvalidDefinition (cycles), got {:?}", other),
    }
}

// ── Validation: valid DAGs pass ─────────────────────────────────

#[test]
fn validate_empty_workflow_ok() {
    let wf = minimal_workflow(vec![], vec![]);
    let parser = WorkflowParser::new();
    parser.validate(&wf).unwrap();
}

#[test]
fn validate_single_node_ok() {
    let wf = minimal_workflow(vec![task_node("a")], vec![]);
    let parser = WorkflowParser::new();
    parser.validate(&wf).unwrap();
}

#[test]
fn validate_linear_chain_ok() {
    let wf = minimal_workflow(
        vec![task_node("a"), task_node("b"), task_node("c")],
        vec![seq_edge("a", "b"), seq_edge("b", "c")],
    );
    let parser = WorkflowParser::new();
    parser.validate(&wf).unwrap();
}

#[test]
fn validate_diamond_dag_ok() {
    let wf = minimal_workflow(
        vec![
            task_node("a"),
            task_node("b"),
            task_node("c"),
            task_node("d"),
        ],
        vec![
            seq_edge("a", "b"),
            seq_edge("a", "c"),
            seq_edge("b", "d"),
            seq_edge("c", "d"),
        ],
    );
    let parser = WorkflowParser::new();
    parser.validate(&wf).unwrap();
}

// ── to_graph ────────────────────────────────────────────────────

#[test]
fn to_graph_builds_correct_structure() {
    let wf = minimal_workflow(
        vec![task_node("a"), task_node("b"), task_node("c")],
        vec![seq_edge("a", "b"), seq_edge("b", "c")],
    );
    let parser = WorkflowParser::new();
    let graph = parser.to_graph(&wf).unwrap();
    assert_eq!(graph.node_count(), 3);
    assert_eq!(graph.edge_count(), 2);
}

#[test]
fn to_graph_preserves_node_types() {
    let mut nodes = vec![task_node("a")];
    nodes[0].node_type = NodeType::ConflictDetect;
    let wf = minimal_workflow(nodes, vec![]);
    let parser = WorkflowParser::new();
    let graph = parser.to_graph(&wf).unwrap();
    let node = &graph[petgraph::graph::NodeIndex::new(0)];
    assert_eq!(node.node_type, NodeType::ConflictDetect);
    assert_eq!(node.id, "a");
}

#[test]
fn to_graph_preserves_edge_conditions() {
    let wf = minimal_workflow(
        vec![task_node("a"), task_node("b")],
        vec![EdgeDefinition {
            from: "a".to_string(),
            to: "b".to_string(),
            edge_type: EdgeType::Conditional,
            condition: Some("x > 0".to_string()),
        }],
    );
    let parser = WorkflowParser::new();
    let graph = parser.to_graph(&wf).unwrap();
    let edge_ref = graph.edge_indices().next().unwrap();
    let edge = &graph[edge_ref];
    assert_eq!(edge.edge_type, EdgeType::Conditional);
    assert_eq!(edge.condition.as_deref(), Some("x > 0"));
}

// ── Serialization roundtrip ─────────────────────────────────────

#[test]
fn json_roundtrip() {
    let parser = WorkflowParser::new();
    let original = minimal_workflow(
        vec![task_node("a"), task_node("b")],
        vec![seq_edge("a", "b")],
    );
    let json = parser.to_json(&original).unwrap();
    let restored = parser.parse_json(&json).unwrap();
    assert_eq!(restored.name, original.name);
    assert_eq!(restored.nodes.len(), original.nodes.len());
    assert_eq!(restored.edges.len(), original.edges.len());
}

#[test]
fn yaml_roundtrip() {
    let parser = WorkflowParser::new();
    let original = minimal_workflow(
        vec![task_node("a"), task_node("b")],
        vec![seq_edge("a", "b")],
    );
    let yaml = parser.to_yaml(&original).unwrap();
    let restored = parser.parse_yaml(&yaml).unwrap();
    assert_eq!(restored.name, original.name);
    assert_eq!(restored.nodes.len(), original.nodes.len());
    assert_eq!(restored.edges.len(), original.edges.len());
}

// ── Serde enum variants ─────────────────────────────────────────

#[test]
fn node_type_serde_snake_case() {
    let json = r#""ab_test""#;
    let nt: NodeType = serde_json::from_str(json).unwrap();
    assert_eq!(nt, NodeType::AbTest);

    let json = r#""insight_extract""#;
    let nt: NodeType = serde_json::from_str(json).unwrap();
    assert_eq!(nt, NodeType::InsightExtract);

    let json = r#""conflict_detect""#;
    let nt: NodeType = serde_json::from_str(json).unwrap();
    assert_eq!(nt, NodeType::ConflictDetect);
}

#[test]
fn edge_type_serde_snake_case() {
    let json = r#""sequential""#;
    let et: EdgeType = serde_json::from_str(json).unwrap();
    assert_eq!(et, EdgeType::Sequential);

    let json = r#""conditional""#;
    let et: EdgeType = serde_json::from_str(json).unwrap();
    assert_eq!(et, EdgeType::Conditional);

    let json = r#""error""#;
    let et: EdgeType = serde_json::from_str(json).unwrap();
    assert_eq!(et, EdgeType::Error);
}

// ── RetryPolicy default ─────────────────────────────────────────

#[test]
fn retry_policy_default_values() {
    let rp = RetryPolicy::default();
    assert_eq!(rp.max_attempts, 3);
    assert_eq!(rp.backoff_multiplier, 2.0);
    assert_eq!(rp.initial_delay_ms, 1000);
    assert_eq!(rp.max_delay_ms, 60000);
    assert_eq!(rp.retryable_errors, vec!["timeout", "transient"]);
}

// ── Templates produce valid workflows ───────────────────────────

#[test]
fn template_conflict_detection_valid() {
    let parser = WorkflowParser::new();
    let wf = WorkflowTemplates::conflict_detection();
    parser.validate(&wf).unwrap();
    assert_eq!(wf.name, "conflict_detection");
    assert_eq!(wf.nodes.len(), 4);
    assert_eq!(wf.edges.len(), 3);
}

#[test]
fn template_paradigm_shift_valid() {
    let parser = WorkflowParser::new();
    let wf = WorkflowTemplates::paradigm_shift_test();
    parser.validate(&wf).unwrap();
    assert_eq!(wf.name, "paradigm_shift_test");
    assert_eq!(wf.nodes.len(), 5);
    assert_eq!(wf.edges.len(), 4);
}

#[test]
fn template_knowledge_consolidation_valid() {
    let parser = WorkflowParser::new();
    let wf = WorkflowTemplates::knowledge_consolidation();
    parser.validate(&wf).unwrap();
    assert_eq!(wf.name, "knowledge_consolidation");
    assert_eq!(wf.nodes.len(), 3);
    assert_eq!(wf.edges.len(), 2);
}

// ── Templates produce valid graphs ──────────────────────────────

#[test]
fn template_conflict_detection_to_graph() {
    let parser = WorkflowParser::new();
    let wf = WorkflowTemplates::conflict_detection();
    let graph = parser.to_graph(&wf).unwrap();
    assert_eq!(graph.node_count(), 4);
    assert_eq!(graph.edge_count(), 3);
}

// ── Edge case: DSL validation rejects invalid edges in DSL ──────

#[test]
fn parse_dsl_dangling_edge_rejected() {
    let dsl = r#"
workflow "bad" {
  node "a" { type: task }
  edge "a" -> "nonexistent"
}
"#;
    let parser = WorkflowParser::new();
    let err = parser.parse_dsl(dsl).unwrap_err();
    match err {
        crate::WorkflowError::InvalidDefinition(msg) => {
            assert!(msg.contains("non-existent node"));
        }
        other => panic!("expected InvalidDefinition, got {:?}", other),
    }
}

#[test]
fn parse_dsl_cycle_rejected() {
    let dsl = r#"
workflow "cyclic" {
  node "a" { type: task }
  node "b" { type: task }
  edge "a" -> "b"
  edge "b" -> "a"
}
"#;
    let parser = WorkflowParser::new();
    let err = parser.parse_dsl(dsl).unwrap_err();
    match err {
        crate::WorkflowError::InvalidDefinition(msg) => assert!(msg.contains("cycles")),
        other => panic!("expected InvalidDefinition (cycles), got {:?}", other),
    }
}
