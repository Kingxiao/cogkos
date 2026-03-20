//! Feedback system integration tests

use cogkos_core::models::*;
use cogkos_store::*;

#[tokio::test]
async fn test_feedback_insert_and_retrieve() {
    let store = InMemoryFeedbackStore::new();
    let fb = AgentFeedback {
        query_hash: 42,
        agent_id: "agent-1".into(),
        success: true,
        feedback_note: Some("Helpful".into()),
        timestamp: chrono::Utc::now(),
    };
    store.insert_feedback("test-tenant", &fb).await.unwrap();
    let results = store.get_feedback_for_query("test-tenant", 42).await.unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
}

#[tokio::test]
async fn test_feedback_multiple_for_same_query() {
    let store = InMemoryFeedbackStore::new();

    for i in 0..5 {
        let fb = AgentFeedback {
            query_hash: 100,
            agent_id: format!("agent-{}", i),
            success: i % 2 == 0,
            feedback_note: None,
            timestamp: chrono::Utc::now(),
        };
        store.insert_feedback("test-tenant", &fb).await.unwrap();
    }

    let results = store.get_feedback_for_query("test-tenant", 100).await.unwrap();
    assert_eq!(results.len(), 5);
}

#[tokio::test]
async fn test_feedback_different_queries_isolated() {
    let store = InMemoryFeedbackStore::new();

    let fb1 = AgentFeedback {
        query_hash: 1,
        agent_id: "a1".into(),
        success: true,
        feedback_note: None,
        timestamp: chrono::Utc::now(),
    };
    let fb2 = AgentFeedback {
        query_hash: 2,
        agent_id: "a1".into(),
        success: false,
        feedback_note: None,
        timestamp: chrono::Utc::now(),
    };

    store.insert_feedback("test-tenant", &fb1).await.unwrap();
    store.insert_feedback("test-tenant", &fb2).await.unwrap();

    assert_eq!(store.get_feedback_for_query("test-tenant", 1).await.unwrap().len(), 1);
    assert_eq!(store.get_feedback_for_query("test-tenant", 2).await.unwrap().len(), 1);
    assert_eq!(store.get_feedback_for_query("test-tenant", 999).await.unwrap().len(), 0);
}
