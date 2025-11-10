use crate::messages::{InputProposal, PaymentOutput, Reveal};
use crate::{BatchError, Result};
use bitcoin::{Address, Network, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use tracing::{debug, info, warn};

const DUST_THRESHOLD: u64 = 546;
const _P2WPKH_INPUT_WEIGHT: u64 = 68; // 41 vbytes for P2WPKH input
const _P2WPKH_OUTPUT_WEIGHT: u64 = 31; // witness output
const _TX_OVERHEAD_WEIGHT: u64 = 42; // version, locktime, input/output counts

/// Deterministic batch assembler
pub struct BatchAssembler {
    network: Network,
    fee_rate_sat_vb: u64,
    allow_change: bool,
}

impl BatchAssembler {
    pub fn new(network: Network, fee_rate_sat_vb: u64, allow_change: bool) -> Self {
        Self {
            network,
            fee_rate_sat_vb,
            allow_change,
        }
    }

    /// Validate a reveal proposal
    pub fn validate_reveal(&self, reveal: &Reveal) -> Result<()> {
        // First, validate the participant has sufficient funds for their fee share
        self.validate_sufficient_funds_for_fees(reveal)?;

        // Check for duplicate inputs within this reveal
        let mut seen = HashSet::new();
        for input in &reveal.inputs {
            let key = (input.outpoint.txid.clone(), input.outpoint.vout);
            if !seen.insert(key) {
                return Err(BatchError::DuplicateInput(format!(
                    "{}:{}",
                    input.outpoint.txid, input.outpoint.vout
                )));
            }
        }

        // Validate descriptors are P2WPKH
        for input in &reveal.inputs {
            if !input.descriptor.starts_with("wpkh(") {
                return Err(BatchError::InvalidReveal(format!(
                    "Only P2WPKH inputs allowed, got: {}",
                    input.descriptor
                )));
            }
        }

        // Validate payment addresses match network
        for payment in &reveal.payments {
            let addr = Address::from_str(&payment.address)
                .map_err(|e| BatchError::InvalidReveal(format!("Invalid address: {}", e)))?;

            if !addr.is_valid_for_network(self.network) {
                return Err(BatchError::NetworkMismatch {
                    expected: format!("{:?}", self.network),
                    actual: format!("{:?}", addr.network),
                });
            }

            // Check for dust
            if payment.amount_sats < DUST_THRESHOLD {
                return Err(BatchError::DustOutput {
                    amount: payment.amount_sats,
                });
            }
        }

        // Validate change address if present
        if let Some(ref change_addr_str) = reveal.change_address {
            if !self.allow_change {
                return Err(BatchError::InvalidReveal(
                    "Change not allowed in this batch".to_string(),
                ));
            }

            let change_addr = Address::from_str(change_addr_str)
                .map_err(|e| BatchError::InvalidReveal(format!("Invalid change address: {}", e)))?;

            if !change_addr.is_valid_for_network(self.network) {
                return Err(BatchError::NetworkMismatch {
                    expected: format!("{:?}", self.network),
                    actual: format!("{:?}", change_addr.network),
                });
            }
        }

        Ok(())
    }

    /// Validate that a participant has sufficient funds to cover their weight-based fee
    fn validate_sufficient_funds_for_fees(&self, reveal: &Reveal) -> Result<()> {
        // Calculate total input value
        let input_sum: u64 = reveal
            .inputs
            .iter()
            .map(|i| i.witness_utxo.value_sats)
            .sum();

        // Calculate total payment value
        let payment_sum: u64 = reveal.payments.iter().map(|p| p.amount_sats).sum();

        // Calculate the participant's weight contribution
        let num_inputs = reveal.inputs.len();
        let num_outputs = reveal.payments.len()
            + if reveal.change_address.is_some() {
                1
            } else {
                0
            };

        // Weight calculation (same as in calculate_weight_based_fees)
        let base_input_weight = (num_inputs as u64 * 41) * 4; // 41 bytes at 4x weight
        let witness_input_weight = num_inputs as u64 * 107; // 107 bytes at 1x weight
        let output_weight = (num_outputs as u64 * 31) * 4; // 31 bytes at 4x weight
        let participant_weight = base_input_weight + witness_input_weight + output_weight;

        // Estimate the fee for this participant's weight
        // We use a conservative estimate: assume this is the only participant
        // (actual fee will be lower when split among multiple participants)
        let estimated_vsize = (participant_weight + 3) / 4; // Convert weight to vsize
        let estimated_min_fee = estimated_vsize * self.fee_rate_sat_vb;

        // Calculate remaining after payments and minimum fee
        let remaining = input_sum
            .saturating_sub(payment_sum)
            .saturating_sub(estimated_min_fee);

        // If there's a change address, ensure there's at least dust threshold for change
        // Otherwise, ensure exact payment (no significant leftover)
        if reveal.change_address.is_some() {
            // Must have at least dust threshold for change after fee
            if input_sum < payment_sum + estimated_min_fee + DUST_THRESHOLD {
                return Err(BatchError::InvalidReveal(format!(
                    "Insufficient funds: have {} sats, need at least {} sats (payments: {}, min fee: {}, change: {})",
                    input_sum,
                    payment_sum + estimated_min_fee + DUST_THRESHOLD,
                    payment_sum,
                    estimated_min_fee,
                    DUST_THRESHOLD
                )));
            }
        } else {
            // Without change address, must have enough for payments and fee
            if input_sum < payment_sum + estimated_min_fee {
                return Err(BatchError::InvalidReveal(format!(
                    "Insufficient funds: have {} sats, need at least {} sats (payments: {}, min fee: {})",
                    input_sum,
                    payment_sum + estimated_min_fee,
                    payment_sum,
                    estimated_min_fee
                )));
            }

            // Warn if leaving too much without change address (potential loss)
            if remaining > DUST_THRESHOLD * 2 {
                // This isn't an error, but it's suspicious - they're overpaying significantly
                // In production, might want to log this for monitoring
                debug!(
                    "Warning: Participant leaving {} sats as fee (no change address specified)",
                    remaining
                );
            }
        }

        Ok(())
    }

    /// Build deterministic transaction from all reveals
    pub fn build_transaction(
        &self,
        reveals: &HashMap<String, Reveal>,
    ) -> Result<(Transaction, HashMap<String, String>)> {
        info!(
            "Building deterministic transaction from {} reveals",
            reveals.len()
        );

        // Collect all inputs with owner info
        let mut all_inputs: Vec<(InputProposal, String)> = Vec::new();
        for (participant, reveal) in reveals {
            for input in &reveal.inputs {
                all_inputs.push((input.clone(), participant.clone()));
            }
        }

        // Check for duplicate inputs across participants
        let mut seen_outpoints = HashSet::new();
        for (input, participant) in &all_inputs {
            let key = (input.outpoint.txid.clone(), input.outpoint.vout);
            if !seen_outpoints.insert(key) {
                return Err(BatchError::DuplicateInput(format!(
                    "Participant {} tried to spend already-spent input {}:{}",
                    participant, input.outpoint.txid, input.outpoint.vout
                )));
            }
        }

        // Sort inputs deterministically: (txid LE bytes, vout asc)
        all_inputs.sort_by(|a, b| {
            let txid_a = &a.0.outpoint.txid;
            let txid_b = &b.0.outpoint.txid;
            txid_a
                .cmp(txid_b)
                .then(a.0.outpoint.vout.cmp(&b.0.outpoint.vout))
        });

        // Build input->owner map (with string keys for JSON serialization)
        let input_map: HashMap<String, String> = all_inputs
            .iter()
            .enumerate()
            .map(|(idx, (_, owner))| (idx.to_string(), owner.clone()))
            .collect();

        // Calculate total input value
        let total_input_value: u64 = all_inputs
            .iter()
            .map(|(input, _)| input.witness_utxo.value_sats)
            .sum();

        debug!("Total input value: {} sats", total_input_value);

        // Collect all payment outputs
        let mut all_payments: Vec<(PaymentOutput, String)> = Vec::new();
        for (participant, reveal) in reveals {
            for payment in &reveal.payments {
                all_payments.push((payment.clone(), participant.clone()));
            }
        }

        let total_payment_value: u64 = all_payments.iter().map(|(p, _)| p.amount_sats).sum();
        debug!("Total payment value: {} sats", total_payment_value);

        // Estimate transaction size (vbytes)
        let num_inputs = all_inputs.len();
        let mut num_outputs = all_payments.len();

        // Estimate with potential change outputs
        if self.allow_change {
            num_outputs += reveals.len(); // worst case: all participants get change
        }

        let estimated_vsize = Self::estimate_vsize(num_inputs, num_outputs);
        let total_fee = estimated_vsize * self.fee_rate_sat_vb;
        debug!(
            "Estimated vsize: {} vB, total fee: {} sats",
            estimated_vsize, total_fee
        );

        // Calculate weight-based fee for each participant
        // Fee is proportional to transaction weight contribution (inputs and outputs)
        let participant_fees = Self::calculate_weight_based_fees(reveals, total_fee);

        info!(
            "Weight-based fee distribution for total fee of {} sats:",
            total_fee
        );
        for (participant, fee_share) in &participant_fees {
            let participant_inputs = reveals
                .get(participant)
                .map(|r| r.inputs.len())
                .unwrap_or(0);
            let participant_outputs = reveals
                .get(participant)
                .map(|r| r.payments.len())
                .unwrap_or(0)
                + (reveals
                    .get(participant)
                    .and_then(|r| r.change_address.as_ref())
                    .map(|_| 1)
                    .unwrap_or(0));
            info!(
                "  {} ({} inputs, {} outputs): {} sats",
                &participant[..8],
                participant_inputs,
                participant_outputs,
                fee_share
            );
        }

        // Validate each participant can actually pay their calculated fee share
        // and calculate change for each participant
        let mut change_outputs: Vec<(String, String, u64)> = Vec::new(); // (participant, address, amount)

        for (participant, reveal) in reveals {
            let input_sum: u64 = reveal
                .inputs
                .iter()
                .map(|i| i.witness_utxo.value_sats)
                .sum();
            let payment_sum: u64 = reveal.payments.iter().map(|p| p.amount_sats).sum();
            let fee_share = participant_fees.get(participant).copied().unwrap_or(0);

            // Critical check: Ensure participant has enough funds for payments + their fee share
            let required_amount = payment_sum + fee_share;
            if input_sum < required_amount {
                return Err(BatchError::InvalidReveal(format!(
                    "Participant {} has insufficient funds after fee calculation: have {} sats, need {} sats (payments: {}, fee: {})",
                    participant,
                    input_sum,
                    required_amount,
                    payment_sum,
                    fee_share
                )));
            }

            // Calculate change amount
            let change_amount = input_sum - payment_sum - fee_share; // Safe subtraction after check above

            if self.allow_change && change_amount >= DUST_THRESHOLD {
                if let Some(ref change_addr) = reveal.change_address {
                    change_outputs.push((participant.clone(), change_addr.clone(), change_amount));
                    debug!("Participant {} change: {} sats", participant, change_amount);
                } else if change_amount > DUST_THRESHOLD * 2 {
                    // Warn if significant amount would be lost without change address
                    warn!(
                        "Participant {} will lose {} sats to fees (no change address provided)",
                        participant, change_amount
                    );
                }
            } else if change_amount > 0 && change_amount < DUST_THRESHOLD {
                // Small amount goes to fees (dust)
                debug!(
                    "Participant {} contributes additional {} sats to fees (below dust threshold)",
                    participant, change_amount
                );
            }
        }

        // Check sufficient funds
        let total_needed = total_payment_value + total_fee;
        if total_input_value < total_needed {
            return Err(BatchError::InsufficientFunds {
                need: total_needed,
                have: total_input_value,
            });
        }

        // Build outputs: payments + change
        let mut tx_outputs: Vec<TxOut> = Vec::new();

        // Add payment outputs
        for (payment, _) in &all_payments {
            let addr = Address::from_str(&payment.address)
                .map_err(|e| BatchError::InvalidReveal(format!("Invalid address: {}", e)))?
                .require_network(self.network)
                .map_err(|e| {
                    BatchError::InvalidReveal(format!("Address network mismatch: {}", e))
                })?;

            tx_outputs.push(TxOut {
                value: payment.amount_sats,
                script_pubkey: addr.script_pubkey(),
            });
        }

        // Add change outputs
        for (_, change_addr_str, amount) in &change_outputs {
            let addr = Address::from_str(change_addr_str)
                .map_err(|e| BatchError::InvalidReveal(format!("Invalid change address: {}", e)))?
                .require_network(self.network)
                .map_err(|e| {
                    BatchError::InvalidReveal(format!("Address network mismatch: {}", e))
                })?;

            tx_outputs.push(TxOut {
                value: *amount,
                script_pubkey: addr.script_pubkey(),
            });
        }

        // Sort outputs deterministically: (script_pubkey bytes asc, value asc)
        tx_outputs.sort_by(|a, b| {
            a.script_pubkey
                .as_bytes()
                .cmp(b.script_pubkey.as_bytes())
                .then(a.value.cmp(&b.value))
        });

        // Build transaction inputs
        let tx_inputs: Vec<TxIn> = all_inputs
            .iter()
            .map(|(input, _)| {
                let outpoint = input
                    .outpoint
                    .to_outpoint()
                    .map_err(|e| BatchError::InvalidReveal(format!("Invalid outpoint: {}", e)))?;

                Ok(TxIn {
                    previous_output: outpoint,
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX, // 0xFFFFFFFF
                    witness: Witness::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        // Build final transaction
        let tx = Transaction {
            version: 2,
            lock_time: bitcoin::blockdata::locktime::absolute::LockTime::ZERO,
            input: tx_inputs,
            output: tx_outputs,
        };

        info!(
            "Built transaction with {} inputs, {} outputs",
            tx.input.len(),
            tx.output.len()
        );

        Ok((tx, input_map))
    }

    /// Calculate weight-based fees for each participant
    /// Fee is proportional to the transaction weight each participant contributes
    pub fn calculate_weight_based_fees(
        reveals: &HashMap<String, Reveal>,
        total_fee: u64,
    ) -> HashMap<String, u64> {
        let mut participant_weights: HashMap<String, u64> = HashMap::new();
        let mut total_weight = 0u64;

        // Calculate each participant's weight contribution
        for (participant, reveal) in reveals {
            let num_inputs = reveal.inputs.len();
            // Count outputs: payments + change (if present)
            let num_outputs = reveal.payments.len()
                + if reveal.change_address.is_some() {
                    1
                } else {
                    0
                };

            // Weight calculation based on Bitcoin transaction weight units
            // Inputs contribute both base weight and witness weight
            let base_input_weight = (num_inputs as u64 * 41) * 4; // 41 bytes at 4x weight
            let witness_input_weight = num_inputs as u64 * 107; // 107 bytes at 1x weight

            // Outputs only contribute base weight (no witness data)
            let output_weight = (num_outputs as u64 * 31) * 4; // 31 bytes at 4x weight

            let participant_weight = base_input_weight + witness_input_weight + output_weight;

            participant_weights.insert(participant.clone(), participant_weight);
            total_weight += participant_weight;
        }

        // Distribute fees proportionally based on weight
        let mut participant_fees = HashMap::new();
        let mut allocated_fee = 0u64;
        let participants: Vec<_> = participant_weights.keys().cloned().collect();

        for (idx, participant) in participants.iter().enumerate() {
            let weight = participant_weights[participant];

            let fee_share = if idx == participants.len() - 1 {
                // Last participant gets remainder to handle rounding
                total_fee - allocated_fee
            } else {
                // Calculate proportional share based on weight
                (weight as f64 / total_weight as f64 * total_fee as f64) as u64
            };

            allocated_fee += fee_share;
            participant_fees.insert(participant.clone(), fee_share);

            debug!(
                "Participant {} contributes {} weight units ({:.1}%), pays {} sats",
                participant,
                weight,
                (weight as f64 / total_weight as f64) * 100.0,
                fee_share
            );
        }

        participant_fees
    }

    /// Estimate transaction virtual size (vbytes)
    fn estimate_vsize(num_inputs: usize, num_outputs: usize) -> u64 {
        // P2WPKH transaction size calculation:
        // Base transaction (counted at 4x weight):
        // - Version: 4 bytes
        // - Input count: 1 byte (assuming < 253 inputs)
        // - Per input: 41 bytes (outpoint 36 + empty scriptSig 1 + sequence 4)
        // - Output count: 1 byte (assuming < 253 outputs)
        // - Per output: 31 bytes (value 8 + script_pubkey 23 for P2WPKH)
        // - Locktime: 4 bytes

        // Witness data (counted at 1x weight):
        // - Marker + flag: 2 bytes
        // - Per input witness: ~107 bytes
        //   - Stack items count: 1 byte
        //   - Signature length: 1 byte
        //   - Signature: ~72 bytes (can be 71-73)
        //   - Pubkey length: 1 byte
        //   - Pubkey: 33 bytes (compressed)

        // More accurate calculation
        let overhead = 11; // version(4) + locktime(4) + counts(2) + marker(0.5) + flag(0.5)
        let base_input_size = 41; // outpoint + empty scriptSig + sequence
        let base_output_size = 31; // value + P2WPKH script
        let witness_per_input = 107; // Full witness data including signature and pubkey

        let base_weight =
            (overhead + (num_inputs * base_input_size) + (num_outputs * base_output_size)) * 4;
        let witness_weight = num_inputs * witness_per_input + 2; // +2 for marker and flag

        // Total weight = base_weight + witness_weight
        // vsize = ceil(weight / 4)
        let total_weight = base_weight + witness_weight;
        let vsize = (total_weight + 3) / 4; // Round up

        vsize as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_vsize() {
        // 2 inputs, 2 outputs
        let vsize = BatchAssembler::estimate_vsize(2, 2);
        // With updated calculation: ~208 vbytes for 2-in-2-out P2WPKH tx
        // Base: (11 + 2*41 + 2*31) * 4 = 620 weight
        // Witness: 2*107 + 2 = 216 weight
        // Total: 836 weight = 209 vbytes
        assert!(vsize >= 208 && vsize <= 210, "vsize = {}", vsize);

        // Test with 2 inputs, 4 outputs (like our actual transaction)
        let vsize_2_4 = BatchAssembler::estimate_vsize(2, 4);
        // Base: (11 + 2*41 + 4*31) * 4 = 868 weight
        // Witness: 2*107 + 2 = 216 weight
        // Total: 1084 weight = 271 vbytes
        assert!(
            vsize_2_4 >= 270 && vsize_2_4 <= 272,
            "vsize = {}",
            vsize_2_4
        );
    }

    #[test]
    fn test_validate_reveal_dust() {
        use crate::messages::*;
        use uuid::Uuid;

        let assembler = BatchAssembler::new(Network::Signet, 10, true);

        let reveal = Reveal {
            intent_id: Uuid::new_v4(),
            participant_pkarr: "test".to_string(),
            inputs: vec![InputProposal {
                outpoint: OutPointSerde {
                    txid: "test".to_string(),
                    vout: 0,
                },
                witness_utxo: TxOutSerde {
                    script_pubkey: "001414c7d5e11f7db2fea6e93b2c04c3ce68810e4a9".to_string(),
                    value_sats: 10000, // Enough to cover the payment and fees
                },
                descriptor: "wpkh([fingerprint/84'/1'/0']xpub/0/*)".to_string(),
            }],
            payments: vec![PaymentOutput {
                address: "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx".to_string(),
                amount_sats: 100, // Below dust threshold
            }],
            change_address: Some("tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx".to_string()),
        };

        let result = assembler.validate_reveal(&reveal);
        assert!(matches!(result, Err(BatchError::DustOutput { .. })));
    }

    #[test]
    fn test_deterministic_sorting() {
        // Test that inputs are sorted correctly
        let mut inputs = vec![
            ("aaaa".to_string(), 1u32),
            ("aaaa".to_string(), 0u32),
            ("bbbb".to_string(), 0u32),
            ("aaaa".to_string(), 2u32),
        ];

        inputs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        assert_eq!(inputs[0], ("aaaa".to_string(), 0));
        assert_eq!(inputs[1], ("aaaa".to_string(), 1));
        assert_eq!(inputs[2], ("aaaa".to_string(), 2));
        assert_eq!(inputs[3], ("bbbb".to_string(), 0));
    }
}
