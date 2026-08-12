//! Durable operation primitives for the Workstream 1 transaction coordinator.
//!
//! This module is deliberately not connected to the v6 dispatcher yet. It defines the persisted
//! authority and recovery rules that a later protocol/coordinator phase will consume.

#![allow(dead_code, unused_imports)]

mod coordinator;
mod fingerprint;
mod journal;
mod model;
mod recovery;

pub(crate) use coordinator::{OperationCoordinator, StartupRecovery};
pub(crate) use fingerprint::{SnapshotTagContext, SnapshotTagger};
pub(crate) use journal::{BeginResult, OperationJournalStore};
pub(crate) use model::{
    Digest32, JournalOperation, OperationJournal, OperationKind, OperationPhase, OperationRequest,
    SnapshotBinding,
};
pub(crate) use recovery::RecoveryClassification;
