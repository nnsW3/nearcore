use crate::{GGas, ReceiptId, Round, ShardId, TransactionId};
use std::collections::{HashMap, HashSet};

/// Model-internal representation of a transaction, as in, the entire graph of
/// receipts generated by one transaction submitted to the chain.
///
/// Shards only interact with this using the [`TransactionId`]`.
pub(crate) struct Transaction {
    #[allow(dead_code)]
    pub(crate) id: TransactionId,

    /// Where the transaction is converted to the first receipt.
    pub(crate) sender_shard: ShardId,
    /// Where the transaction's first receipt is sent to.
    pub(crate) initial_receipt_receiver: ShardId,

    /// The receipt created when converting the transaction to a receipt.
    pub(crate) initial_receipt: ReceiptId,
    /// Gas burnt for converting the transaction to the first receipt.
    pub(crate) tx_conversion_cost: GGas,
    /// Gas attached to the first receipt.
    pub(crate) initial_receipt_gas: GGas,

    /// Definition of directed edges of the DAG.
    pub(crate) outgoing: HashMap<ReceiptId, Vec<ReceiptId>>,
    /// Reverse edge index for quick access.
    ///
    /// TODO: this is currently ignored, but we will need it for postponed
    /// receipts handling.
    #[allow(dead_code)]
    pub(crate) dependencies: HashMap<ReceiptId, Vec<ReceiptId>>,

    /// Receipts that have not been created on chain, yet, but will be part of
    /// the transaction execution. Once created, receipts are removed here and
    /// should stay in queue up until they are executed.
    pub(crate) future_receipts: HashMap<ReceiptId, Receipt>,
    /// Receipts that have been created but did not execute, yet. Only the ID is
    /// here because the real receipt is in a queue somewhere.
    pub(crate) pending_receipts: HashSet<ReceiptId>,
    /// Receipts that were explicitly dropped by a shard.
    pub(crate) dropped_receipts: HashMap<ReceiptId, Receipt>,
    /// Receipts that have finished execution.
    pub(crate) executed_receipts: HashMap<ReceiptId, Receipt>,
}

#[must_use = "Forward, explicitly drop, or put receipts in a queue."]
#[derive(Clone, Debug)]
pub struct Receipt {
    pub id: ReceiptId,
    pub created_at: Option<Round>,
    pub dropped_at: Option<Round>,
    pub executed_at: Option<Round>,
    pub receiver: ShardId,
    pub size: u64,
    pub attached_gas: GGas,

    // private to the shards until after the execution
    execution_gas: GGas,
}

pub(crate) struct ExecutionResult {
    pub gas_burnt: GGas,
    pub new_receipts: Vec<Receipt>,
}

impl Transaction {
    pub(crate) fn start(&mut self, round: Round) -> ExecutionResult {
        let receipt = self
            .activate_receipt(self.initial_receipt, round)
            .expect("should not start the same transaction twice");
        self.pending_receipts.insert(self.initial_receipt);
        ExecutionResult { gas_burnt: self.tx_conversion_cost, new_receipts: vec![receipt] }
    }

    pub(crate) fn execute_receipt(
        &mut self,
        mut receipt: Receipt,
        round: Round,
    ) -> ExecutionResult {
        let outgoing_ids = self.outgoing[&receipt.id].clone();
        let new_receipts = outgoing_ids
            .into_iter()
            .map(|receipt_id| {
                self.activate_receipt(receipt_id, round)
                    .expect("must not create the same receipt multiple times")
            })
            .collect();

        let gas_burnt = receipt.execution_gas;
        receipt.executed_at = Some(round);

        self.pending_receipts.remove(&receipt.id);
        self.executed_receipts.insert(receipt.id, receipt);

        ExecutionResult { gas_burnt, new_receipts }
    }

    pub(crate) fn drop_receipt(&mut self, mut receipt: Receipt, round: Round) {
        self.pending_receipts.remove(&receipt.id);
        receipt.dropped_at = Some(round);
        self.dropped_receipts.insert(receipt.id, receipt);
    }

    pub(crate) fn activate_receipt(
        &mut self,
        receipt_id: ReceiptId,
        round: Round,
    ) -> Option<Receipt> {
        let mut receipt = self.future_receipts.remove(&receipt_id)?;
        receipt.created_at = Some(round);
        self.pending_receipts.insert(receipt.id);
        Some(receipt)
    }

    pub(crate) fn initial_receipt_receiver(&self) -> ShardId {
        self.initial_receipt_receiver
    }

    pub(crate) fn initial_receipt_gas(&self) -> GGas {
        self.initial_receipt_gas
    }
}

impl Receipt {
    pub fn new_future_receipt(
        id: ReceiptId,
        receiver: ShardId,
        size: u64,
        attached_gas: GGas,
        execution_gas: GGas,
    ) -> Self {
        Self {
            id,
            created_at: None,
            dropped_at: None,
            executed_at: None,
            receiver,
            size,
            attached_gas,
            execution_gas,
        }
    }

    pub fn transaction_id(&self) -> TransactionId {
        self.id.transaction_id()
    }

    pub(crate) fn gas_burnt(&self) -> GGas {
        if self.executed_at.is_some() {
            self.execution_gas
        } else {
            0
        }
    }
}
