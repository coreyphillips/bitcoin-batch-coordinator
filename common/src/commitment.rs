use crate::messages::{Commitment, Reveal};
use crate::{BatchError, Result};
use blake3;

/// Generate commitment from a reveal
pub fn generate_commitment(reveal: &Reveal) -> Commitment {
    Commitment {
        intent_id: reveal.intent_id,
        participant_pkarr: reveal.participant_pkarr.clone(),
        commitment_hash: commit_reveal(reveal),
    }
}

/// Generate commitment hash from a reveal (also used as generate_commitment_hash for compatibility)
pub fn generate_commitment_hash(reveal: &Reveal) -> String {
    commit_reveal(reveal)
}

/// Generate commitment hash from a reveal
pub fn commit_reveal(reveal: &Reveal) -> String {
    let canonical = serialize_reveal_canonical(reveal);
    let hash = blake3::hash(canonical.as_bytes());
    hex::encode(hash.as_bytes())
}

/// Verify that a reveal matches a commitment
pub fn verify_commitment(reveal: &Reveal, commitment_hash: &str) -> Result<()> {
    let computed = commit_reveal(reveal);
    if computed != commitment_hash {
        return Err(BatchError::InvalidCommitment {
            expected: commitment_hash.to_string(),
            actual: computed,
        });
    }
    Ok(())
}

/// Serialize reveal in canonical form for commitment
fn serialize_reveal_canonical(reveal: &Reveal) -> String {
    // Create a deterministic JSON representation
    // Sort inputs by outpoint, sort payments by address
    let mut reveal_sorted = reveal.clone();

    reveal_sorted.inputs.sort_by(|a, b| {
        a.outpoint
            .txid
            .cmp(&b.outpoint.txid)
            .then(a.outpoint.vout.cmp(&b.outpoint.vout))
    });

    reveal_sorted.payments.sort_by(|a, b| {
        a.address
            .cmp(&b.address)
            .then(a.amount_sats.cmp(&b.amount_sats))
    });

    // Serialize to JSON (serde_json ensures consistent key ordering)
    serde_json::to_string(&reveal_sorted).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::*;
    use uuid::Uuid;

    #[test]
    fn test_commitment_consistency() {
        let reveal = create_test_reveal();
        let hash1 = commit_reveal(&reveal);
        let hash2 = commit_reveal(&reveal);
        assert_eq!(hash1, hash2, "Commitment should be deterministic");
    }

    #[test]
    fn test_commitment_verification() {
        let reveal = create_test_reveal();
        let commitment = commit_reveal(&reveal);
        assert!(verify_commitment(&reveal, &commitment).is_ok());
    }

    #[test]
    fn test_commitment_mismatch() {
        let reveal = create_test_reveal();
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            verify_commitment(&reveal, wrong_hash),
            Err(BatchError::InvalidCommitment { .. })
        ));
    }

    #[test]
    fn test_input_order_independence() {
        // Create a fixed intent_id to use for both reveals
        let intent_id = Uuid::new_v4();

        // Create two reveals with same data but different input order
        let mut reveal1 = create_test_reveal();
        reveal1.intent_id = intent_id; // Use same intent_id

        let mut reveal2 = create_test_reveal();
        reveal2.intent_id = intent_id; // Use same intent_id

        // Swap inputs in reveal2
        if reveal2.inputs.len() >= 2 {
            reveal2.inputs.swap(0, 1);
        }

        let hash1 = commit_reveal(&reveal1);
        let hash2 = commit_reveal(&reveal2);

        // Should produce same commitment due to canonical sorting
        assert_eq!(hash1, hash2, "Commitment should be order-independent");
    }

    fn create_test_reveal() -> Reveal {
        Reveal {
            intent_id: Uuid::new_v4(),
            participant_pkarr: "test_participant".to_string(),
            inputs: vec![
                InputProposal {
                    outpoint: OutPointSerde {
                        txid: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                        vout: 0,
                    },
                    witness_utxo: TxOutSerde {
                        value_sats: 100000,
                        script_pubkey: "0014abcd".to_string(),
                    },
                    descriptor: "wpkh(test)".to_string(),
                },
                InputProposal {
                    outpoint: OutPointSerde {
                        txid: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                            .to_string(),
                        vout: 1,
                    },
                    witness_utxo: TxOutSerde {
                        value_sats: 50000,
                        script_pubkey: "0014dcba".to_string(),
                    },
                    descriptor: "wpkh(test2)".to_string(),
                },
            ],
            payments: vec![PaymentOutput {
                address: "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx".to_string(),
                amount_sats: 10000,
            }],
            change_address: Some(
                "tb1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3q0sl5k7".to_string(),
            ),
        }
    }
}
