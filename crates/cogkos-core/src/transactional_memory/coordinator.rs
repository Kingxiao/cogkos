//! Two-phase commit coordinator and transactional memory system

use super::catalog::*;
use super::router::*;
use crate::models::{EpistemicClaim, Id, MetaKnowledgeEntry};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Transaction status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    Pending,
    Preparing,
    Prepared,
    Committing,
    Committed,
    Aborting,
    Aborted,
    Failed,
}

/// Distributed transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedTransaction {
    pub id: Uuid,
    pub status: TransactionStatus,
    pub participants: Vec<String>, // Instance IDs
    pub operations: Vec<TransactionOperation>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub coordinator: String, // Instance ID of coordinator
}

/// Transaction operation type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionOperation {
    InsertKnowledge {
        claim: Box<EpistemicClaim>,
    },
    UpdateKnowledge {
        claim_id: Id,
        updates: KnowledgeUpdate,
    },
    DeleteKnowledge {
        claim_id: Id,
    },
    RouteQuery {
        query: String,
        target_instances: Vec<String>,
    },
    SyncMetadata {
        entry: MetaKnowledgeEntry,
    },
}

/// Knowledge update payload
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KnowledgeUpdate {
    pub confidence: Option<f64>,
    pub status: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

impl DistributedTransaction {
    pub fn new(coordinator: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            status: TransactionStatus::Pending,
            participants: Vec::new(),
            operations: Vec::new(),
            created_at: now,
            updated_at: now,
            coordinator: coordinator.into(),
        }
    }

    pub fn add_operation(mut self, op: TransactionOperation) -> Self {
        self.operations.push(op);
        self.updated_at = Utc::now();
        self
    }

    pub fn add_participant(mut self, instance_id: impl Into<String>) -> Self {
        let id = instance_id.into();
        if !self.participants.contains(&id) {
            self.participants.push(id);
        }
        self
    }

    pub fn update_status(&mut self, status: TransactionStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }
}

/// Two-phase commit coordinator
pub struct TransactionCoordinator {
    transactions: HashMap<Uuid, DistributedTransaction>,
    _instance_timeout_seconds: i64,
}

impl TransactionCoordinator {
    pub fn new() -> Self {
        Self {
            transactions: HashMap::new(),
            _instance_timeout_seconds: 30,
        }
    }

    /// Start a new distributed transaction
    pub fn begin_transaction(
        &mut self,
        coordinator_id: impl Into<String>,
    ) -> DistributedTransaction {
        let tx = DistributedTransaction::new(coordinator_id);
        self.transactions.insert(tx.id, tx.clone());
        tx
    }

    /// Prepare phase - ask participants to prepare
    pub async fn prepare(&mut self, tx_id: Uuid) -> Result<PrepareResult> {
        let tx = self.transactions.get_mut(&tx_id).ok_or_else(|| {
            TransactionalMemoryError::TransactionFailed("Transaction not found".to_string())
        })?;

        tx.update_status(TransactionStatus::Preparing);

        // In production: send prepare requests to all participants
        // For now, simulate success
        let all_prepared = true; // Simulated

        if all_prepared {
            tx.update_status(TransactionStatus::Prepared);
            Ok(PrepareResult::Ready)
        } else {
            tx.update_status(TransactionStatus::Aborting);
            Ok(PrepareResult::Abort)
        }
    }

    /// Commit phase - commit the transaction
    pub async fn commit(&mut self, tx_id: Uuid) -> Result<CommitResult> {
        let tx = self.transactions.get_mut(&tx_id).ok_or_else(|| {
            TransactionalMemoryError::TransactionFailed("Transaction not found".to_string())
        })?;

        if tx.status != TransactionStatus::Prepared {
            return Err(TransactionalMemoryError::TransactionFailed(
                "Transaction not in prepared state".to_string(),
            ));
        }

        tx.update_status(TransactionStatus::Committing);

        // In production: send commit to all participants
        // For now, simulate success
        let all_committed = true;

        if all_committed {
            tx.update_status(TransactionStatus::Committed);
            Ok(CommitResult::Committed)
        } else {
            tx.update_status(TransactionStatus::Failed);
            Err(TransactionalMemoryError::TransactionFailed(
                "Some participants failed to commit".to_string(),
            ))
        }
    }

    /// Abort the transaction
    pub async fn abort(&mut self, tx_id: Uuid) -> Result<()> {
        let tx = self.transactions.get_mut(&tx_id).ok_or_else(|| {
            TransactionalMemoryError::TransactionFailed("Transaction not found".to_string())
        })?;

        tx.update_status(TransactionStatus::Aborting);

        // In production: send abort to all participants

        tx.update_status(TransactionStatus::Aborted);
        Ok(())
    }

    /// Get transaction status
    pub fn get_transaction(&self, tx_id: Uuid) -> Option<&DistributedTransaction> {
        self.transactions.get(&tx_id)
    }

    /// Get all active transactions
    pub fn active_transactions(&self) -> Vec<&DistributedTransaction> {
        self.transactions
            .values()
            .filter(|tx| {
                matches!(
                    tx.status,
                    TransactionStatus::Pending
                        | TransactionStatus::Preparing
                        | TransactionStatus::Prepared
                        | TransactionStatus::Committing
                )
            })
            .collect()
    }

    /// Cleanup completed transactions older than age
    pub fn cleanup_completed(&mut self, max_age: chrono::Duration) {
        let cutoff = Utc::now() - max_age;
        self.transactions.retain(|_, tx| {
            !(matches!(
                tx.status,
                TransactionStatus::Committed
                    | TransactionStatus::Aborted
                    | TransactionStatus::Failed
            ) && tx.updated_at < cutoff)
        });
    }
}

impl Default for TransactionCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Prepare phase result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareResult {
    Ready,
    Abort,
    Timeout,
}

/// Commit phase result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitResult {
    Committed,
    Partial,
    Failed,
}

/// Complete transactional memory system
pub struct TransactionalMemory {
    pub catalog: MetaKnowledgeCatalog,
    pub router: CrossInstanceRouter,
    pub coordinator: TransactionCoordinator,
    instance_id: String,
}

impl TransactionalMemory {
    pub fn new(instance_id: impl Into<String>) -> Self {
        let instance_id = instance_id.into();
        Self {
            catalog: MetaKnowledgeCatalog::new(),
            router: CrossInstanceRouter::new(),
            coordinator: TransactionCoordinator::new(),
            instance_id,
        }
    }

    /// Register a remote instance
    pub fn register_remote_instance(&mut self, entry: CatalogEntry) -> Result<()> {
        self.catalog.upsert(entry.clone())?;
        self.router.register_instance(entry)?;
        Ok(())
    }

    /// Route federated query
    pub fn route_federated_query(&mut self, query: &str, domains: &[String]) -> RoutingDecision {
        self.router.route_query(query, domains)
    }

    /// Start distributed knowledge transaction
    pub fn begin_knowledge_transaction(&mut self) -> DistributedTransaction {
        self.coordinator.begin_transaction(self.instance_id.clone())
    }

    /// Sync knowledge with remote instance
    pub async fn sync_knowledge(
        &mut self,
        target_instance: &str,
        knowledge: Vec<EpistemicClaim>,
    ) -> Result<DistributedTransaction> {
        let mut tx = self.begin_knowledge_transaction();
        tx = tx.add_participant(target_instance);

        for claim in knowledge {
            tx = tx.add_operation(TransactionOperation::InsertKnowledge {
                claim: Box::new(claim),
            });
        }

        Ok(tx)
    }
}
