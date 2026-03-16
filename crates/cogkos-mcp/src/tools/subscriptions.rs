//! Subscription management handlers (Issue #132)

use cogkos_core::Result;
use cogkos_core::models::*;
use cogkos_store::SubscriptionStore;

use super::types::*;

/// Handle RSS subscription request
///
/// Implements TC-L7-03: RSS订阅_定时拉取
pub async fn handle_subscribe_rss(
    req: SubscribeRssRequest,
    tenant_id: &str,
    subscription_store: &dyn SubscriptionStore,
) -> Result<SubscriptionResponse> {
    let config = serde_json::json!({
        "url": req.url,
        "fetch_full_content": req.fetch_full_content,
        "max_items": req.max_items,
    });

    let subscription = SubscriptionSource {
        id: uuid::Uuid::new_v4(),
        name: format!("RSS: {}", req.url),
        source_type: SubscriptionType::Rss,
        config,
        poll_interval_secs: req.poll_interval_secs,
        claimant_template: Claimant::System,
        base_confidence: 0.5,
        enabled: true,
        last_polled: None,
        error_count: 0,
        tenant_id: tenant_id.to_string(),
    };

    let subscription_id = subscription_store
        .create_subscription(&subscription)
        .await?;

    tracing::info!(
        subscription_id = %subscription_id,
        url = %req.url,
        poll_interval = req.poll_interval_secs,
        "RSS subscription created successfully"
    );

    Ok(SubscriptionResponse {
        subscription_id: subscription_id.to_string(),
        status: "active".to_string(),
        message: format!(
            "RSS subscription '{}' created successfully with ID: {}",
            req.url, subscription_id
        ),
        created_at: chrono::Utc::now(),
    })
}

/// Handle webhook subscription request
///
/// Implements TC-L7-01: Webhook_接收触发
/// Implements TC-L7-02: Webhook_签名验证
pub async fn handle_subscribe_webhook(
    req: SubscribeWebhookRequest,
    tenant_id: &str,
    subscription_store: &dyn SubscriptionStore,
) -> Result<SubscriptionResponse> {
    let config = serde_json::json!({
        "url": req.url,
        "secret": req.secret,
        "events": req.events,
    });

    let subscription = SubscriptionSource {
        id: uuid::Uuid::new_v4(),
        name: format!("Webhook: {}", req.url),
        source_type: SubscriptionType::Webhook,
        config,
        poll_interval_secs: 0,
        claimant_template: Claimant::System,
        base_confidence: 0.7,
        enabled: true,
        last_polled: None,
        error_count: 0,
        tenant_id: tenant_id.to_string(),
    };

    let subscription_id = subscription_store
        .create_subscription(&subscription)
        .await?;

    tracing::info!(
        subscription_id = %subscription_id,
        url = %req.url,
        events = ?req.events,
        "Webhook subscription created successfully"
    );

    Ok(SubscriptionResponse {
        subscription_id: subscription_id.to_string(),
        status: "active".to_string(),
        message: format!(
            "Webhook subscription '{}' created successfully with ID: {}",
            req.url, subscription_id
        ),
        created_at: chrono::Utc::now(),
    })
}

/// Handle API polling subscription request
///
/// Implements TC-L7-04: API轮询_间隔控制
pub async fn handle_subscribe_api(
    req: SubscribeApiRequest,
    tenant_id: &str,
    subscription_store: &dyn SubscriptionStore,
) -> Result<SubscriptionResponse> {
    let config = serde_json::json!({
        "url": req.url,
        "method": req.method,
        "headers": req.headers,
        "body": req.body,
    });

    let subscription = SubscriptionSource {
        id: uuid::Uuid::new_v4(),
        name: format!("API: {}", req.url),
        source_type: SubscriptionType::Rss,
        config,
        poll_interval_secs: req.poll_interval_secs,
        claimant_template: Claimant::System,
        base_confidence: 0.6,
        enabled: true,
        last_polled: None,
        error_count: 0,
        tenant_id: tenant_id.to_string(),
    };

    let subscription_id = subscription_store
        .create_subscription(&subscription)
        .await?;

    tracing::info!(
        subscription_id = %subscription_id,
        url = %req.url,
        poll_interval = req.poll_interval_secs,
        method = %req.method,
        "API polling subscription created successfully"
    );

    Ok(SubscriptionResponse {
        subscription_id: subscription_id.to_string(),
        status: "active".to_string(),
        message: format!(
            "API polling subscription '{}' created successfully with ID: {}",
            req.url, subscription_id
        ),
        created_at: chrono::Utc::now(),
    })
}

/// List active subscriptions
pub async fn handle_list_subscriptions(
    req: ListSubscriptionsRequest,
    tenant_id: &str,
    subscription_store: &dyn SubscriptionStore,
) -> Result<serde_json::Value> {
    let all_subscriptions = subscription_store.list_subscriptions(tenant_id).await?;

    let filtered: Vec<_> = all_subscriptions
        .iter()
        .filter(|s| match req {
            ListSubscriptionsRequest::Rss => s.source_type == SubscriptionType::Rss,
            ListSubscriptionsRequest::Webhook => s.source_type == SubscriptionType::Webhook,
        })
        .collect();

    let subscriptions: Vec<serde_json::Value> = filtered
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id.to_string(),
                "name": s.name,
                "source_type": format!("{:?}", s.source_type).to_lowercase(),
                "enabled": s.enabled,
                "poll_interval_secs": s.poll_interval_secs,
                "error_count": s.error_count,
                "last_polled": s.last_polled,
            })
        })
        .collect();

    let type_str = match req {
        ListSubscriptionsRequest::Rss => "rss",
        ListSubscriptionsRequest::Webhook => "webhook",
    };

    Ok(serde_json::json!({
        "type": type_str,
        "subscriptions": subscriptions,
        "total": subscriptions.len()
    }))
}
