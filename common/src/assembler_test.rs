#[cfg(test)]
mod weight_based_fee_tests {
    use crate::assembler::BatchAssembler;
    use crate::messages::{InputProposal, OutPointSerde, PaymentOutput, Reveal, TxOutSerde};
    use std::collections::HashMap;

    fn create_test_input(txid: &str, vout: u32, value: u64) -> InputProposal {
        InputProposal {
            outpoint: OutPointSerde {
                txid: txid.to_string(),
                vout,
            },
            witness_utxo: TxOutSerde {
                script_pubkey: "001414c7d5e11f7db2fea6e93b2c04c3ce68810e4a9".to_string(),
                value_sats: value,
            },
            descriptor: "wpkh([fingerprint/84'/1'/0']xpub/0/*)".to_string(),
        }
    }

    fn create_test_payment(address: &str, amount: u64) -> PaymentOutput {
        PaymentOutput {
            address: address.to_string(),
            amount_sats: amount,
        }
    }

    #[test]
    fn test_weight_based_fees_are_fair() {
        // Test that a participant with many inputs/outputs pays more fees
        let mut reveals = HashMap::new();

        // Participant A: Simple transaction (1 input, 1 output)
        reveals.insert(
            "participant_a".to_string(),
            Reveal {
                intent_id: uuid::Uuid::new_v4(),
                participant_pkarr: "participant_a".to_string(),
                inputs: vec![create_test_input("aaaa", 0, 100_000)],
                payments: vec![create_test_payment("bcrt1qtest1", 50_000)],
                change_address: Some("bcrt1qchange1".to_string()),
            },
        );

        // Participant B: Complex transaction (5 inputs, 10 outputs)
        reveals.insert(
            "participant_b".to_string(),
            Reveal {
                intent_id: uuid::Uuid::new_v4(),
                participant_pkarr: "participant_b".to_string(),
                inputs: vec![
                    create_test_input("bbbb", 0, 20_000),
                    create_test_input("bbbb", 1, 20_000),
                    create_test_input("bbbb", 2, 20_000),
                    create_test_input("bbbb", 3, 20_000),
                    create_test_input("bbbb", 4, 20_000),
                ],
                payments: vec![
                    create_test_payment("bcrt1qtest2", 5_000),
                    create_test_payment("bcrt1qtest3", 5_000),
                    create_test_payment("bcrt1qtest4", 5_000),
                    create_test_payment("bcrt1qtest5", 5_000),
                    create_test_payment("bcrt1qtest6", 5_000),
                    create_test_payment("bcrt1qtest7", 5_000),
                    create_test_payment("bcrt1qtest8", 5_000),
                    create_test_payment("bcrt1qtest9", 5_000),
                    create_test_payment("bcrt1qtest10", 5_000),
                    create_test_payment("bcrt1qtest11", 5_000),
                ],
                change_address: Some("bcrt1qchange2".to_string()),
            },
        );

        let total_fee = 1000; // 1000 sats total fee
        let fees = BatchAssembler::calculate_weight_based_fees(&reveals, total_fee);

        let fee_a = fees.get("participant_a").unwrap();
        let fee_b = fees.get("participant_b").unwrap();

        println!("Weight-based fee distribution:");
        println!("  Participant A (1 input, 2 outputs): {} sats", fee_a);
        println!("  Participant B (5 inputs, 11 outputs): {} sats", fee_b);

        // Participant B should pay significantly more
        assert!(fee_b > fee_a, "Complex transaction should pay more fees");

        // Rough calculation check:
        // A: 1 input (~271 weight units) + 2 outputs (~248 weight units) = ~519 weight units
        // B: 5 inputs (~1355 weight units) + 11 outputs (~1364 weight units) = ~2719 weight units
        // B should pay roughly 5x more than A

        let ratio = (*fee_b as f64) / (*fee_a as f64);
        println!("  Fee ratio (B/A): {:.2}x", ratio);

        assert!(
            ratio > 4.0 && ratio < 6.0,
            "Fee ratio should be approximately 5x (actual: {:.2}x)",
            ratio
        );

        // Verify total fees are distributed correctly
        assert_eq!(
            fee_a + fee_b,
            total_fee,
            "Total fees should equal requested amount"
        );
    }

    #[test]
    fn test_weight_based_vs_input_value_based() {
        // Demonstrate the difference between weight-based and the old input-value-based approach
        let mut reveals = HashMap::new();

        // Both participants have same input value (100k sats) but different complexity
        reveals.insert(
            "simple".to_string(),
            Reveal {
                intent_id: uuid::Uuid::new_v4(),
                participant_pkarr: "simple".to_string(),
                inputs: vec![
                    create_test_input("aaaa", 0, 100_000), // One large input
                ],
                payments: vec![create_test_payment("bcrt1qtest1", 90_000)],
                change_address: Some("bcrt1qchange1".to_string()),
            },
        );

        reveals.insert(
            "complex".to_string(),
            Reveal {
                intent_id: uuid::Uuid::new_v4(),
                participant_pkarr: "complex".to_string(),
                inputs: vec![
                    // Many small inputs totaling 100k
                    create_test_input("bbbb", 0, 10_000),
                    create_test_input("bbbb", 1, 10_000),
                    create_test_input("bbbb", 2, 10_000),
                    create_test_input("bbbb", 3, 10_000),
                    create_test_input("bbbb", 4, 10_000),
                    create_test_input("bbbb", 5, 10_000),
                    create_test_input("bbbb", 6, 10_000),
                    create_test_input("bbbb", 7, 10_000),
                    create_test_input("bbbb", 8, 10_000),
                    create_test_input("bbbb", 9, 10_000),
                ],
                payments: vec![
                    // Many small outputs
                    create_test_payment("bcrt1qtest2", 9_000),
                    create_test_payment("bcrt1qtest3", 9_000),
                    create_test_payment("bcrt1qtest4", 9_000),
                    create_test_payment("bcrt1qtest5", 9_000),
                    create_test_payment("bcrt1qtest6", 9_000),
                    create_test_payment("bcrt1qtest7", 9_000),
                    create_test_payment("bcrt1qtest8", 9_000),
                    create_test_payment("bcrt1qtest9", 9_000),
                    create_test_payment("bcrt1qtest10", 9_000),
                    create_test_payment("bcrt1qtest11", 9_000),
                ],
                change_address: None, // No change needed
            },
        );

        let total_fee = 2000;
        let weight_fees = BatchAssembler::calculate_weight_based_fees(&reveals, total_fee);

        let simple_fee = weight_fees.get("simple").unwrap();
        let complex_fee = weight_fees.get("complex").unwrap();

        println!("\nComparison with same input values (100k sats each):");
        println!("  Old input-value-based: both would pay 1000 sats (50% each)");
        println!("  New weight-based:");
        println!("    Simple (1 input, 2 outputs): {} sats", simple_fee);
        println!("    Complex (10 inputs, 10 outputs): {} sats", complex_fee);

        // With weight-based fees, complex pays much more
        assert!(
            *complex_fee > *simple_fee * 3,
            "Complex transaction should pay at least 3x more with weight-based fees"
        );

        println!(
            "  Fairness improvement: Complex now pays {:.1}x more (fair!)",
            (*complex_fee as f64) / (*simple_fee as f64)
        );
    }

    #[test]
    fn test_insufficient_funds_validation() {
        use crate::assembler::BatchAssembler;
        use bitcoin::Network;

        let assembler = BatchAssembler::new(Network::Bitcoin, 10, true); // 10 sat/vB

        // Create a reveal with insufficient funds for fees
        // Trying to pay 99,999 sats with 100,000 sats input (only 1 sat for fees!)
        let underfunded_reveal = Reveal {
            intent_id: uuid::Uuid::new_v4(),
            participant_pkarr: "attacker".to_string(),
            inputs: vec![
                create_test_input("aaaa", 0, 100_000), // 100k sats input
            ],
            payments: vec![
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 99_999), // 99,999 sats payment
            ],
            change_address: Some("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf".to_string()),
        };

        // This should fail validation (simulates what participant does client-side)
        let result = assembler.validate_reveal(&underfunded_reveal);
        assert!(result.is_err(), "Should reject underfunded reveal");

        if let Err(e) = result {
            println!("Client-side validation correctly rejected: {}", e);
            assert!(e.to_string().contains("Insufficient funds"));
        }

        // Create a reveal with just enough funds (should pass)
        let funded_reveal = Reveal {
            intent_id: uuid::Uuid::new_v4(),
            participant_pkarr: "honest".to_string(),
            inputs: vec![
                create_test_input("bbbb", 0, 100_000), // 100k sats input
            ],
            payments: vec![
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 95_000), // 95k sats payment
            ],
            change_address: Some("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf".to_string()),
        };

        // This should pass validation (5000 sats available for fees and change)
        let result = assembler.validate_reveal(&funded_reveal);
        assert!(
            result.is_ok(),
            "Should accept properly funded reveal: {:?}",
            result
        );
    }

    #[test]
    fn test_complex_transaction_sufficient_funds() {
        use crate::assembler::BatchAssembler;
        use bitcoin::Network;

        let assembler = BatchAssembler::new(Network::Bitcoin, 10, true); // 10 sat/vB

        // Complex transaction with many inputs/outputs
        // Must have enough funds to cover the higher weight-based fee
        let complex_reveal = Reveal {
            intent_id: uuid::Uuid::new_v4(),
            participant_pkarr: "complex".to_string(),
            inputs: vec![
                // 10 inputs of 20k each = 200k total
                create_test_input("cccc", 0, 20_000),
                create_test_input("cccc", 1, 20_000),
                create_test_input("cccc", 2, 20_000),
                create_test_input("cccc", 3, 20_000),
                create_test_input("cccc", 4, 20_000),
                create_test_input("cccc", 5, 20_000),
                create_test_input("cccc", 6, 20_000),
                create_test_input("cccc", 7, 20_000),
                create_test_input("cccc", 8, 20_000),
                create_test_input("cccc", 9, 20_000),
            ],
            payments: vec![
                // 10 outputs of 18.5k each = 185k total (leaving 15k for fees)
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 18_500),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 18_500),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 18_500),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 18_500),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 18_500),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 18_500),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 18_500),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 18_500),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 18_500),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 18_500),
            ],
            change_address: Some("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf".to_string()),
        };

        // With 10 inputs and 11 outputs (including change), this needs significant fees
        // Should have 10k sats available for fees (200k - 190k)
        let result = assembler.validate_reveal(&complex_reveal);
        assert!(
            result.is_ok(),
            "Complex transaction with sufficient funds should pass: {:?}",
            result
        );

        // Now try with insufficient funds for the complexity
        let underfunded_complex = Reveal {
            intent_id: uuid::Uuid::new_v4(),
            participant_pkarr: "underfunded_complex".to_string(),
            inputs: vec![
                // Same 10 inputs
                create_test_input("dddd", 0, 20_000),
                create_test_input("dddd", 1, 20_000),
                create_test_input("dddd", 2, 20_000),
                create_test_input("dddd", 3, 20_000),
                create_test_input("dddd", 4, 20_000),
                create_test_input("dddd", 5, 20_000),
                create_test_input("dddd", 6, 20_000),
                create_test_input("dddd", 7, 20_000),
                create_test_input("dddd", 8, 20_000),
                create_test_input("dddd", 9, 20_000),
            ],
            payments: vec![
                // But trying to pay out 199k (only 1k for fees - not enough!)
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 19_900),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 19_900),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 19_900),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 19_900),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 19_900),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 19_900),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 19_900),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 19_900),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 19_900),
                create_test_payment("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf", 19_900),
            ],
            change_address: Some("bc1q7cyrfmck2ffu2ud3rn5l5a8yv6f0chkp0zpemf".to_string()),
        };

        let result = assembler.validate_reveal(&underfunded_complex);
        assert!(
            result.is_err(),
            "Complex transaction without sufficient funds should fail"
        );

        if let Err(e) = result {
            println!("Correctly rejected complex underfunded transaction: {}", e);
            assert!(e.to_string().contains("Insufficient funds"));
        }
    }
}
