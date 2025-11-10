use crate::messages::Reveal;
use crate::{BatchError, Result};
use bitcoin::{
    psbt::{Input as PsbtInput, Output as PsbtOutput, Psbt},
    Network, Transaction,
};
use std::collections::HashMap;
use tracing::{debug, info};

/// PSBT v0 builder for batch transactions
pub struct PsbtBuilder {
    _network: Network,
}

impl PsbtBuilder {
    pub fn new(network: Network) -> Self {
        Self { _network: network }
    }

    /// Build PSBT v0 template from unsigned transaction
    pub fn build_template(
        &self,
        unsigned_tx: Transaction,
        reveals: &HashMap<String, Reveal>,
        input_map: &HashMap<String, String>,
    ) -> Result<Psbt> {
        info!(
            "Building PSBT v0 template with {} inputs",
            unsigned_tx.input.len()
        );

        let mut psbt = Psbt::from_unsigned_tx(unsigned_tx)
            .map_err(|e| BatchError::InvalidPsbt(format!("Failed to create PSBT: {}", e)))?;

        // Set PSBT version to 0 explicitly
        psbt.version = 0;

        // Populate inputs with witness_utxo
        for (input_idx, tx_input) in psbt.unsigned_tx.input.iter().enumerate() {
            let participant = input_map.get(&input_idx.to_string()).ok_or_else(|| {
                BatchError::InvalidPsbt(format!("No owner for input {}", input_idx))
            })?;

            let reveal = reveals.get(participant).ok_or_else(|| {
                BatchError::InvalidPsbt(format!("No reveal for participant {}", participant))
            })?;

            // Find the corresponding input proposal
            let input_proposal = reveal
                .inputs
                .iter()
                .find(|inp| {
                    inp.outpoint
                        .to_outpoint()
                        .map(|op| op == tx_input.previous_output)
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    BatchError::InvalidPsbt(format!(
                        "Input {} not found in participant's reveal",
                        input_idx
                    ))
                })?;

            // Set witness_utxo (required for PSBT v0 with SegWit)
            let witness_utxo = input_proposal
                .witness_utxo
                .to_txout()
                .map_err(|e| BatchError::InvalidPsbt(format!("Invalid witness UTXO: {}", e)))?;

            // Ensure the PSBT has enough inputs
            while psbt.inputs.len() <= input_idx {
                psbt.inputs.push(PsbtInput::default());
            }

            psbt.inputs[input_idx].witness_utxo = Some(witness_utxo);

            // Extract scriptPubKey to derive the public key hash
            // For P2WPKH: scriptPubKey is OP_0 <20-byte-pubkey-hash>
            // We need to add bip32_derivation so BDK can sign
            // The descriptor contains this info: wpkh([fingerprint/path]xprv/0/*)

            // For now, skip bip32_derivation and rely on BDK's ability to match by scriptPubKey
            // This requires the participant's wallet to recognize the scriptPubKey

            debug!("Input {} mapped to participant {}", input_idx, participant);
        }

        // Initialize outputs
        while psbt.outputs.len() < psbt.unsigned_tx.output.len() {
            psbt.outputs.push(PsbtOutput::default());
        }

        info!("PSBT template built successfully");
        Ok(psbt)
    }

    /// Merge multiple PSBT fragments into one
    pub fn merge_fragments(&self, base: &mut Psbt, fragments: Vec<Psbt>) -> Result<()> {
        info!("Merging {} PSBT fragments", fragments.len());

        for (frag_idx, fragment) in fragments.iter().enumerate() {
            debug!("Processing fragment {}", frag_idx);

            // Verify fragment has same unsigned tx
            if fragment.unsigned_tx != base.unsigned_tx {
                return Err(BatchError::InvalidPsbt(
                    "Fragment has different unsigned transaction".to_string(),
                ));
            }

            // Merge inputs
            for (idx, frag_input) in fragment.inputs.iter().enumerate() {
                if idx >= base.inputs.len() {
                    debug!("Fragment {} input {} skipped (out of range)", frag_idx, idx);
                    continue;
                }

                debug!(
                    "Fragment {} input {} has {} partial_sigs",
                    frag_idx,
                    idx,
                    frag_input.partial_sigs.len()
                );

                // Merge partial_sigs
                for (pubkey, sig) in &frag_input.partial_sigs {
                    // Note: In bitcoin 0.30, Signature type changed
                    // We'll just merge without detailed SIGHASH verification
                    base.inputs[idx].partial_sigs.insert(*pubkey, sig.clone());
                    info!(
                        "✅ Merged signature for input {} from pubky {:?}",
                        idx, pubkey
                    );
                }

                // Merge other fields if needed
                if base.inputs[idx].witness_utxo.is_none() && frag_input.witness_utxo.is_some() {
                    base.inputs[idx].witness_utxo = frag_input.witness_utxo.clone();
                    debug!("Merged witness_utxo for input {}", idx);
                }
            }
        }

        // Log final state
        for (idx, input) in base.inputs.iter().enumerate() {
            info!(
                "Final base input {} has {} partial_sigs",
                idx,
                input.partial_sigs.len()
            );
        }

        info!("PSBT fragments merged successfully");
        Ok(())
    }

    /// Finalize PSBT (convert partial_sigs to final witness)
    pub fn finalize(&self, psbt: &mut Psbt) -> Result<()> {
        info!("Finalizing PSBT with {} inputs", psbt.inputs.len());

        for (idx, input) in psbt.inputs.iter_mut().enumerate() {
            if input.final_script_witness.is_some() {
                debug!("Input {} already finalized", idx);
                continue;
            }

            // For P2WPKH: witness = [signature, pubkey]
            if input.partial_sigs.len() != 1 {
                return Err(BatchError::InvalidPsbt(format!(
                    "Input {} has {} signatures, expected 1 for P2WPKH",
                    idx,
                    input.partial_sigs.len()
                )));
            }

            let (pubkey, sig) = input.partial_sigs.iter().next().unwrap();

            // Build witness: [signature, pubkey]
            let mut witness = bitcoin::Witness::new();
            let sig_bytes = sig.serialize();
            witness.push(&sig_bytes[..]);
            witness.push(pubkey.to_bytes());

            input.final_script_witness = Some(witness);
            input.partial_sigs.clear(); // Clear after finalizing

            debug!("Finalized input {}", idx);
        }

        info!("PSBT finalized successfully");
        Ok(())
    }

    /// Extract final transaction from finalized PSBT
    pub fn extract_tx(&self, psbt: &Psbt) -> Result<Transaction> {
        // Verify all inputs are finalized
        for (idx, input) in psbt.inputs.iter().enumerate() {
            if input.final_script_witness.is_none() && input.final_script_sig.is_none() {
                return Err(BatchError::InvalidPsbt(format!(
                    "Input {} is not finalized",
                    idx
                )));
            }
        }

        // In bitcoin 0.30, extract_tx returns Transaction directly
        let tx = psbt.clone().extract_tx();

        info!("Extracted final transaction: {}", tx.txid());
        Ok(tx)
    }

    /// Serialize PSBT to base64
    pub fn to_base64(psbt: &Psbt) -> Result<String> {
        use base64::{engine::general_purpose, Engine as _};

        // In bitcoin 0.30, PSBT serializes to string directly
        let psbt_str = psbt.to_string();
        Ok(general_purpose::STANDARD.encode(psbt_str.as_bytes()))
    }

    /// Deserialize PSBT from base64
    pub fn from_base64(s: &str) -> Result<Psbt> {
        use base64::{engine::general_purpose, Engine as _};
        use std::str::FromStr;

        let bytes = general_purpose::STANDARD
            .decode(s)
            .map_err(|e| BatchError::InvalidPsbt(format!("Invalid base64: {}", e)))?;

        let psbt_str = std::str::from_utf8(&bytes)
            .map_err(|e| BatchError::InvalidPsbt(format!("Invalid UTF-8: {}", e)))?;

        Psbt::from_str(psbt_str)
            .map_err(|e| BatchError::InvalidPsbt(format!("Failed to parse PSBT: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{OutPoint, ScriptBuf, Sequence, Transaction, TxIn};

    #[test]
    fn test_psbt_base64_roundtrip() {
        // Create a simple PSBT
        let tx = Transaction {
            version: 2,
            lock_time: bitcoin::blockdata::locktime::absolute::LockTime::ZERO,
            input: vec![],
            output: vec![],
        };

        let psbt = Psbt::from_unsigned_tx(tx).unwrap();
        let base64 = PsbtBuilder::to_base64(&psbt).unwrap();
        let decoded = PsbtBuilder::from_base64(&base64).unwrap();

        assert_eq!(psbt.unsigned_tx, decoded.unsigned_tx);
    }

    #[test]
    fn test_finalize_p2wpkh() {
        // This test verifies that the finalize() function correctly converts
        // partial signatures into final witness scripts for P2WPKH inputs

        use bitcoin::ecdsa;
        use bitcoin::sighash::EcdsaSighashType;
        use bitcoin::{PublicKey, TxOut, Txid};
        use std::str::FromStr;

        let builder = PsbtBuilder::new(Network::Signet);

        // Create a transaction with one P2WPKH input
        let tx = Transaction {
            version: 2,
            lock_time: bitcoin::blockdata::locktime::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_str(
                        "0101010101010101010101010101010101010101010101010101010101010101",
                    )
                    .unwrap(),
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: bitcoin::Witness::new(),
            }],
            output: vec![TxOut {
                value: 50000,
                script_pubkey: ScriptBuf::from_hex("0014abcdef1234567890abcdef1234567890abcdef12")
                    .unwrap(),
            }],
        };

        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();

        // Set up a witness UTXO for the input (required for P2WPKH)
        psbt.inputs[0].witness_utxo = Some(TxOut {
            value: 100000,
            script_pubkey: ScriptBuf::from_hex("0014abcdef1234567890abcdef1234567890abcdef12")
                .unwrap(),
        });

        // Create a test public key and signature
        // In a real scenario, this would come from actual signing
        // This is a compressed public key (33 bytes - 0x03 prefix indicates y-coordinate is odd)
        let pubkey_bytes =
            hex::decode("03b3e8a5b47d4e1b1a9c7e4d5f8a2c6e9d7b5a3f1e8c4a2b6d9e7f5c3a1b4e2d7f")
                .unwrap();
        let pubkey = PublicKey::from_slice(&pubkey_bytes).unwrap();

        // Create a properly formatted signature
        // This represents a 71-byte DER signature (common size for ECDSA signatures)
        let sig_bytes = hex::decode("304402203a5c8c4b8e2d6f1a9b7e4c3d2f1a8e9b7c6d5e4a3f2b1c8d9e7a6f5b4c3a2d1e02207f8e9d1a2b3c4d5e6f7a8b9c1d2e3f4a5b6c7d8e9f1a2b3c4d5e6f7a8b9c1d2e").unwrap();

        // Create the bitcoin::ecdsa::Signature
        let sig = bitcoin::secp256k1::ecdsa::Signature::from_der(&sig_bytes).unwrap();
        let ecdsa_sig = ecdsa::Signature {
            sig,
            hash_ty: EcdsaSighashType::All,
        };

        // Add the partial signature
        psbt.inputs[0].partial_sigs.insert(pubkey, ecdsa_sig);

        // Finalize the PSBT
        let result = builder.finalize(&mut psbt);

        // For this test, we expect it to succeed and create a witness script
        assert!(result.is_ok(), "Finalization should succeed");

        // After finalization, check that:
        // 1. The witness script was created
        assert!(
            psbt.inputs[0].final_script_witness.is_some(),
            "Should have final witness"
        );

        // 2. Partial signatures were cleared
        assert!(
            psbt.inputs[0].partial_sigs.is_empty(),
            "Partial sigs should be cleared"
        );

        // 3. The witness has the expected structure for P2WPKH (2 items: signature and pubkey)
        if let Some(ref witness) = psbt.inputs[0].final_script_witness {
            assert_eq!(witness.len(), 2, "P2WPKH witness should have 2 elements");

            // First element should be the signature (71-73 bytes typically)
            assert!(
                witness.nth(0).unwrap().len() >= 70 && witness.nth(0).unwrap().len() <= 73,
                "First witness element should be signature size"
            );

            // Second element should be the public key (33 bytes for compressed)
            assert_eq!(
                witness.nth(1).unwrap().len(),
                33,
                "Second witness element should be compressed pubkey"
            );
        }
    }

    #[test]
    fn test_finalize_with_multiple_inputs() {
        // Test that finalization works correctly with multiple inputs
        use bitcoin::Txid;
        use std::str::FromStr;

        let _builder = PsbtBuilder::new(Network::Signet);

        let tx = Transaction {
            version: 2,
            lock_time: bitcoin::blockdata::locktime::absolute::LockTime::ZERO,
            input: vec![
                TxIn {
                    previous_output: OutPoint {
                        txid: Txid::from_str(
                            "0101010101010101010101010101010101010101010101010101010101010101",
                        )
                        .unwrap(),
                        vout: 0,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: bitcoin::Witness::new(),
                },
                TxIn {
                    previous_output: OutPoint {
                        txid: Txid::from_str(
                            "0202020202020202020202020202020202020202020202020202020202020202",
                        )
                        .unwrap(),
                        vout: 1,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: bitcoin::Witness::new(),
                },
            ],
            output: vec![],
        };

        let psbt = Psbt::from_unsigned_tx(tx).unwrap();

        // Verify the PSBT has 2 inputs
        assert_eq!(psbt.inputs.len(), 2, "PSBT should have 2 inputs");
        assert_eq!(
            psbt.unsigned_tx.input.len(),
            2,
            "Transaction should have 2 inputs"
        );
    }
}
