use bdk::blockchain::ElectrumBlockchain;
use bdk::electrum_client::{Client as ElectrumClient, ElectrumApi};
use bdk::keys::{
    bip39::{Language, Mnemonic},
    DerivableKey, ExtendedKey,
};
use bdk::wallet::AddressIndex;
use bdk::{database::MemoryDatabase, SyncOptions, Wallet};
use bitcoin::{consensus::encode::deserialize, Address, Network};
use common::{
    assembler::BatchAssembler, commitment::generate_commitment, messages::*,
    psbt_builder::PsbtBuilder, transport::Transport, BatchError, Result,
};
use hex;
use std::str::FromStr;
use tokio::time::{sleep, timeout, Duration};
use tracing::{debug, info, warn};
use uuid::Uuid;

// Configurable polling intervals (in milliseconds)
// These control how frequently the participant checks for coordinator messages
const MESSAGE_CLEAR_DELAY_MS: u64 = 100; // Delay after clearing messages before proceeding
const CURRENT_INTENT_INITIAL_WAIT_MS: u64 = 200; // Initial wait before requesting current intent
const CURRENT_INTENT_POLL_INTERVAL_MS: u64 = 200; // How often to poll for current intent response
const REQUEST_REVEAL_POLL_INTERVAL_MS: u64 = 100; // How often to check for reveal request
const TEMPLATE_POLL_INTERVAL_MS: u64 = 500; // How often to check for template
const FINAL_TX_POLL_INTERVAL_MS: u64 = 500; // How often to check for final transaction

pub async fn run(
    recovery_method: &str,
    recovery_value: &str,
    pass: &str,
    coordinator_pkarr: &str,
    mnemonic_words: &str,
    account: u32,
    network_str: &str,
    payments_str: &str,
    select_str: Option<&str>,
    intent_id_str: Option<&str>,
    no_wait: bool,
    electrum_host: &str,
    electrum_port: u16,
    electrum_proto: &str,
) -> Result<()> {
    info!("Starting participant");

    // Parse network
    let network = match network_str {
        "bitcoin" => Network::Bitcoin,
        "testnet" => Network::Testnet,
        "signet" => Network::Signet,
        "regtest" => Network::Regtest,
        _ => {
            return Err(BatchError::Other(format!(
                "Unknown network: {}",
                network_str
            )))
        }
    };

    info!("Network: {}", network_str);

    // Initialize messenger and transport
    info!("Initializing pubky-messenger...");
    let transport = match recovery_method {
        "file" => Transport::from_recovery_file(recovery_value, pass).await?,
        "phrase" => Transport::from_recovery_phrase(recovery_value, Some(pass)).await?,
        _ => {
            return Err(BatchError::Other(format!(
                "Unknown recovery method: {}",
                recovery_method
            )))
        }
    };

    let participant_pubkey = transport.public_key_string();
    info!("Participant pubky: {}", participant_pubkey);

    // Convert mnemonic to BIP84 descriptors
    info!("Generating BIP84 descriptors from mnemonic...");
    let (external_desc, internal_desc) = mnemonic_to_descriptors(mnemonic_words, network, account)?;

    info!("External descriptor: {}", external_desc);
    info!("Internal descriptor: {}", internal_desc);

    // Clear any old messages with the coordinator from previous runs
    info!("Clearing old messages from previous participant sessions...");
    if let Err(e) = transport.clear_messages_with_peer(coordinator_pkarr).await {
        debug!("Failed to clear old messages with coordinator: {}", e);
        // Continue anyway - this is just cleanup
    }

    // Wait for Intent from coordinator
    let intent = if let Some(id_str) = intent_id_str {
        // Use provided intent ID - query coordinator for the actual intent details
        let intent_id = Uuid::parse_str(id_str)
            .map_err(|e| BatchError::Other(format!("Invalid intent ID: {}", e)))?;

        info!(
            "Using intent ID: {}, querying coordinator for details...",
            intent_id
        );

        // Clear any old messages
        info!("Clearing received message queue before querying...");
        let _ = transport.receive_from(coordinator_pkarr).await;
        sleep(Duration::from_millis(MESSAGE_CLEAR_DELAY_MS)).await;

        // Query coordinator for the specific intent details
        let get_intent = GetCurrentIntent {
            participant_pkarr: participant_pubkey.clone(),
        };

        let request_time = std::time::Instant::now();
        transport
            .send_dm(coordinator_pkarr, &WireMsg::GetCurrentIntent(get_intent))
            .await?;

        // Wait for the CurrentIntent response
        match wait_for_current_intent_after(&transport, coordinator_pkarr, request_time).await {
            Ok(current_intent) => {
                // Verify the intent ID matches what we expected
                if current_intent.intent.intent_id != intent_id {
                    return Err(BatchError::Other(format!(
                        "Coordinator returned intent {} but expected {}. The batch may have already completed or been replaced.",
                        current_intent.intent.intent_id, intent_id
                    )));
                }
                info!(
                    "Received intent details: status={:?}, participants={}/{}",
                    current_intent.status,
                    current_intent.current_participants,
                    current_intent.intent.max_participants
                );
                current_intent.intent
            }
            Err(e) => {
                warn!("Failed to get intent details from coordinator: {}", e);
                return Err(BatchError::Other(format!(
                    "Could not retrieve intent {} from coordinator. The batch may have already completed.",
                    intent_id
                )));
            }
        }
    } else {
        // Clear any messages we might have already received from coordinator
        // This ensures we start fresh and don't get stale CurrentIntent messages
        info!("Clearing received message queue before querying...");
        let _ = transport.receive_from(coordinator_pkarr).await;

        // Short delay to ensure message queue is cleared
        sleep(Duration::from_millis(MESSAGE_CLEAR_DELAY_MS)).await;

        // Try to query coordinator for current intent (multi-batch mode)
        info!("Querying coordinator for current batch intent...");

        // Send request for current intent
        let get_intent = GetCurrentIntent {
            participant_pkarr: participant_pubkey.clone(),
        };

        // Record the time we sent the request
        let request_time = std::time::Instant::now();
        transport
            .send_dm(coordinator_pkarr, &WireMsg::GetCurrentIntent(get_intent))
            .await?;

        // Try to wait for CurrentIntent response, but fall back to waiting for Intent broadcast
        // Pass the request time so we only accept messages that arrive after our request
        match wait_for_current_intent_after(&transport, coordinator_pkarr, request_time).await {
            Ok(current_intent) => {
                info!(
                    "Received current intent: {} (status: {:?}, participants: {}/{})",
                    current_intent.intent.intent_id,
                    current_intent.status,
                    current_intent.current_participants,
                    current_intent.intent.max_participants
                );
                current_intent.intent
            }
            Err(e) => {
                // This shouldn't happen anymore since both coordinator modes now support GetCurrentIntent
                warn!("Failed to get current intent: {}", e);
                return Err(e);
            }
        }
    };

    info!("Received intent: {}", intent.intent_id);
    info!(
        "Fee model: {:?} (fees based on transaction weight contribution)",
        intent.fee_model
    );
    info!("Fee rate: {} sat/vB", intent.fee_rate_sat_vb);

    // Parse payments
    let payments = parse_payments(payments_str)?;
    info!("Payments: {} outputs", payments.len());

    // Connect to Electrum
    info!(
        "Connecting to Electrum at {}://{}:{}...",
        electrum_proto, electrum_host, electrum_port
    );
    let electrum_url = format!("{}://{}:{}", electrum_proto, electrum_host, electrum_port);
    let electrum_client = ElectrumClient::new(&electrum_url)
        .map_err(|e| BatchError::Other(format!("Failed to connect to Electrum: {}", e)))?;

    let height = electrum_client
        .block_headers_subscribe()
        .map_err(|e| BatchError::Other(format!("Failed to subscribe to block headers: {}", e)))?
        .height;
    info!("Connected to Electrum at height {}", height);

    let blockchain = ElectrumBlockchain::from(electrum_client);

    // Initialize BDK wallet
    info!("Initializing BDK wallet...");
    let wallet = Wallet::new(
        &external_desc,
        Some(&internal_desc),
        network,
        MemoryDatabase::default(),
    )
    .map_err(|e| BatchError::Other(format!("Failed to create wallet: {}", e)))?;

    // Sync wallet with blockchain
    info!("Syncing wallet with blockchain...");
    wallet
        .sync(&blockchain, SyncOptions::default())
        .map_err(|e| BatchError::Other(format!("Failed to sync wallet: {}", e)))?;

    let balance = wallet
        .get_balance()
        .map_err(|e| BatchError::Other(format!("Failed to get balance: {}", e)))?;
    info!(
        "Wallet balance: {} sats (confirmed: {}, pending: {})",
        balance.confirmed + balance.trusted_pending + balance.untrusted_pending,
        balance.confirmed,
        balance.trusted_pending + balance.untrusted_pending
    );

    // Build inputs from UTXOs (auto-select or manual)
    let inputs = if let Some(manual_select) = select_str {
        // Manual UTXO selection
        info!("Using manually selected UTXOs");
        let selected_utxos = parse_selected_utxos(manual_select)?;
        build_inputs_from_selection(&selected_utxos, &wallet, &blockchain)?
    } else {
        // Automatic UTXO selection
        info!("Using automatic UTXO selection");
        build_inputs_from_wallet(&wallet)?
    };

    // Get change address if needed
    let change_address = if intent.allow_change {
        // Get next unused change address
        let addr_info = wallet
            .get_address(AddressIndex::New)
            .map_err(|e| BatchError::Other(format!("Failed to get address: {}", e)))?;
        Some(addr_info.address.to_string())
    } else {
        None
    };

    // Build reveal
    let reveal = Reveal {
        intent_id: intent.intent_id,
        participant_pkarr: participant_pubkey.clone(),
        inputs,
        payments,
        change_address,
    };

    // Validate locally BEFORE sending commitment to avoid wasting coordinator's time
    info!("Validating reveal locally before commitment...");
    let assembler = BatchAssembler::new(network, intent.fee_rate_sat_vb, intent.allow_change);
    if let Err(e) = assembler.validate_reveal(&reveal) {
        return Err(BatchError::Other(format!(
            "Cannot participate in batch: {}. Please add more funds to your wallet or reduce payment amounts.",
            e
        )));
    }
    info!("✅ Local validation passed - sufficient funds for participation");

    // 2️⃣ Send Commitment
    info!("Generating and sending commitment...");
    let commitment = generate_commitment(&reveal);
    let commitment_hash = commitment.commitment_hash.clone();
    info!(
        "Sending commitment to coordinator {} for intent {}",
        coordinator_pkarr, intent.intent_id
    );
    transport
        .send_dm(coordinator_pkarr, &WireMsg::Commitment(commitment.clone()))
        .await?;
    info!(
        "Commitment sent (hash: {}), waiting for coordinator to signal reveal phase...",
        commitment_hash
    );

    // 3️⃣ Wait for RequestReveal signal
    let request_reveal = wait_for_reveal_request(
        &transport,
        coordinator_pkarr,
        intent.intent_id,
        intent.deadline_ms,
    )
    .await?;

    // Verify we're in the list of committed participants
    if !request_reveal
        .committed_participants
        .contains(&participant_pubkey)
    {
        return Err(BatchError::Other(
            "We're not in the list of committed participants".to_string(),
        ));
    }

    info!("Received reveal request, sending reveal to coordinator...");

    // 4️⃣ Send Reveal
    transport
        .send_dm(coordinator_pkarr, &WireMsg::Reveal(reveal.clone()))
        .await?;

    // 5️⃣ Wait for Template
    info!("Waiting for template...");
    let template = wait_for_template(
        &transport,
        coordinator_pkarr,
        intent.intent_id,
        intent.deadline_ms,
    )
    .await?;

    info!("Received template");

    // Verify template matches our reveal
    verify_template(&template, &reveal)?;

    // 6️⃣ Sign PSBT
    info!("Signing PSBT...");
    let mut psbt = PsbtBuilder::from_base64(&template.psbt_base64)?;

    // MANUAL SIGNING: BDK's wallet.sign() requires PSBT metadata (bip32_derivation)
    // which the coordinator didn't provide. So we manually sign by deriving keys from mnemonic.
    info!("Manually signing PSBT inputs that belong to our wallet...");

    use bdk::bitcoin::bip32::DerivationPath;
    use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
    use bitcoin::sighash::{EcdsaSighashType, SighashCache};
    use bitcoin::PublicKey as BitcoinPublicKey;

    let secp = Secp256k1::new();
    let mut sighash_cache = SighashCache::new(&psbt.unsigned_tx);

    // Derive master key from mnemonic
    let mnemonic = Mnemonic::parse_in(Language::English, mnemonic_words)
        .map_err(|e| BatchError::Other(format!("Invalid mnemonic: {}", e)))?;
    let xkey: ExtendedKey = mnemonic
        .into_extended_key()
        .map_err(|e| BatchError::Other(format!("Failed to derive extended key: {}", e)))?;
    let master_xprv = xkey
        .into_xprv(network)
        .ok_or_else(|| BatchError::Other("Failed to get xprv".to_string()))?;

    // Derive account-level key (m/84'/coin_type'/account')
    let coin_type = match network {
        Network::Bitcoin => 0,
        Network::Testnet | Network::Signet | Network::Regtest => 1,
        _ => 1,
    };
    let account_path_str = format!("m/84h/{}h/{}h", coin_type, account);
    let account_path: DerivationPath = account_path_str
        .parse()
        .map_err(|e| BatchError::Other(format!("Invalid derivation path: {}", e)))?;
    let account_xprv = master_xprv
        .derive_priv(&secp, &account_path)
        .map_err(|e| BatchError::Other(format!("Derivation failed: {}", e)))?;

    // Build a map of scriptPubKey -> (derivation_index, private_key) for quick lookup
    let mut script_to_key: std::collections::HashMap<bitcoin::ScriptBuf, SecretKey> =
        std::collections::HashMap::new();

    // Try first 1000 addresses (this should cover most use cases)
    for index in 0..1000 {
        let external_path: DerivationPath = format!("m/0/{}", index)
            .parse()
            .map_err(|e| BatchError::Other(format!("Invalid path: {}", e)))?;
        let child_xprv = account_xprv
            .derive_priv(&secp, &external_path)
            .map_err(|e| BatchError::Other(format!("Child derivation failed: {}", e)))?;

        let private_key = child_xprv.private_key;
        let public_key = private_key.public_key(&secp);
        let bitcoin_pubkey = BitcoinPublicKey::new(public_key);

        // P2WPKH scriptPubKey
        let addr = Address::p2wpkh(&bitcoin_pubkey, network)
            .map_err(|e| BatchError::Other(format!("Failed to create P2WPKH address: {}", e)))?;
        let script_pubkey = addr.script_pubkey();
        script_to_key.insert(script_pubkey, private_key);
    }

    // Sign each input that belongs to us
    for (input_idx, psbt_input) in psbt.inputs.iter_mut().enumerate() {
        if let Some(ref witness_utxo) = psbt_input.witness_utxo {
            // Check if we have a private key for this scriptPubKey
            if let Some(private_key) = script_to_key.get(&witness_utxo.script_pubkey) {
                info!("Input {} matches our wallet", input_idx);

                // Compute sighash for this input (P2WPKH uses SIGHASH_ALL)
                let sighash_type = EcdsaSighashType::All;

                // For P2WPKH, we need to use the scriptCode (which is the P2PKH script)
                // The scriptCode for P2WPKH is: OP_DUP OP_HASH160 <pubkey_hash> OP_EQUALVERIFY OP_CHECKSIG
                // P2WPKH scriptPubKey format: 0x0014{20-byte-pubkey-hash}
                let script_bytes = witness_utxo.script_pubkey.as_bytes();
                if script_bytes.len() != 22 || script_bytes[0] != 0x00 || script_bytes[1] != 0x14 {
                    debug!(
                        "Input {} has invalid P2WPKH script format, skipping",
                        input_idx
                    );
                    continue;
                }

                let mut pubkey_hash = [0u8; 20];
                pubkey_hash.copy_from_slice(&script_bytes[2..22]);

                let script_code = bitcoin::blockdata::script::Builder::new()
                    .push_opcode(bitcoin::blockdata::opcodes::all::OP_DUP)
                    .push_opcode(bitcoin::blockdata::opcodes::all::OP_HASH160)
                    .push_slice(&pubkey_hash)
                    .push_opcode(bitcoin::blockdata::opcodes::all::OP_EQUALVERIFY)
                    .push_opcode(bitcoin::blockdata::opcodes::all::OP_CHECKSIG)
                    .into_script();

                let sighash = sighash_cache
                    .segwit_signature_hash(
                        input_idx,
                        &script_code,
                        witness_utxo.value,
                        sighash_type,
                    )
                    .map_err(|e| BatchError::Other(format!("Failed to compute sighash: {}", e)))?;

                let message = Message::from_slice(sighash.as_ref())
                    .map_err(|e| BatchError::Other(format!("Invalid sighash message: {}", e)))?;

                // Sign with this private key
                let signature = secp.sign_ecdsa(&message, private_key);

                // Convert to bitcoin::ecdsa::Signature
                use bitcoin::ecdsa::Signature as EcdsaSignature;
                let ecdsa_sig = EcdsaSignature {
                    sig: signature,
                    hash_ty: sighash_type,
                };

                // Get the public key for this private key
                let public_key = private_key.public_key(&secp);
                let bitcoin_pubkey = BitcoinPublicKey::new(public_key);

                // Add to PSBT
                psbt_input.partial_sigs.insert(bitcoin_pubkey, ecdsa_sig);
                info!("✅ Signed input {} with pubkey {:?}", input_idx, public_key);
            } else {
                debug!(
                    "Input {} does not belong to our wallet, skipping",
                    input_idx
                );
            }
        }
    }

    // Log PSBT state after signing
    info!("PSBT after signing has {} inputs", psbt.inputs.len());
    for (idx, input) in psbt.inputs.iter().enumerate() {
        info!(
            "  Input {} has {} partial_sigs AFTER signing",
            idx,
            input.partial_sigs.len()
        );
    }

    let total_sigs = psbt
        .inputs
        .iter()
        .map(|i| i.partial_sigs.len())
        .sum::<usize>();
    info!("Signed PSBT (added {} signatures total)", total_sigs);

    if total_sigs == 0 {
        return Err(BatchError::Other(
            "Failed to sign any inputs - no matching keys found".to_string(),
        ));
    }

    // Send signature fragment
    let sig_fragment = SigFragment {
        intent_id: intent.intent_id,
        participant_pkarr: participant_pubkey.clone(),
        psbt_fragment_base64: PsbtBuilder::to_base64(&psbt)?,
    };

    transport
        .send_dm(coordinator_pkarr, &WireMsg::SigFragment(sig_fragment))
        .await?;

    // 7️⃣ Wait for FinalTx (optional)
    if no_wait {
        info!("✅ Signature sent successfully!");
        info!(
            "You can leave now - the coordinator will broadcast when all signatures are collected."
        );
        info!(
            "Monitor your inputs/outputs on the blockchain to see when the transaction confirms."
        );
        info!("Your payment(s):");
        for payment in &reveal.payments {
            info!("  → {} sats to {}", payment.amount_sats, payment.address);
        }
        if let Some(change_addr) = &reveal.change_address {
            info!("Your change address: {}", change_addr);
        }

        // NOTE: We do NOT clear messages here when using --no-wait
        // The coordinator still needs to receive our signature fragment!
        // Messages will be cleared on next startup instead

        return Ok(());
    }

    info!("Waiting for final transaction...");
    let final_tx = wait_for_final_tx(&transport, coordinator_pkarr, intent.intent_id).await?;

    info!("Batch transaction complete!");
    info!("TXID: {}", final_tx.txid);
    info!("You can broadcast manually if needed:");
    info!("{}", final_tx.raw_tx_hex);

    // Clean up messages with coordinator after successful batch
    info!("Cleaning up messages with coordinator...");
    if let Err(e) = transport.clear_messages_with_peer(coordinator_pkarr).await {
        debug!("Failed to clear messages with coordinator: {}", e);
        // Continue - cleanup errors are non-fatal
    }
    info!("Message cleanup complete");

    Ok(())
}

async fn _wait_for_intent(transport: &Transport, coordinator: &str) -> Result<Intent> {
    // This is a fallback function - should rarely be called now that coordinators support GetCurrentIntent
    loop {
        sleep(Duration::from_millis(100)).await;

        let messages = transport.receive_from(coordinator).await?;
        for msg in messages {
            if let WireMsg::Intent(intent) = msg {
                return Ok(intent);
            }
        }
    }
}

async fn _wait_for_current_intent(
    transport: &Transport,
    coordinator: &str,
) -> Result<CurrentIntent> {
    // Record when we started waiting for fresh messages
    let start_time = std::time::Instant::now();

    let result = timeout(Duration::from_secs(10), async {
        loop {
            sleep(Duration::from_millis(CURRENT_INTENT_POLL_INTERVAL_MS)).await;

            let messages = transport.receive_from(coordinator).await?;
            for msg in messages {
                if let WireMsg::CurrentIntent(current_intent) = msg {
                    // Accept the message - we're using message deduplication in Transport
                    // which ensures we only get new messages we haven't seen before
                    info!(
                        "Received CurrentIntent for batch {} after {}ms",
                        current_intent.intent.intent_id,
                        start_time.elapsed().as_millis()
                    );
                    return Ok::<CurrentIntent, BatchError>(current_intent);
                }
            }
        }
    })
    .await;

    match result {
        Ok(Ok(ci)) => Ok(ci),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(BatchError::Timeout("current_intent".to_string())),
    }
}

async fn wait_for_current_intent_after(
    transport: &Transport,
    coordinator: &str,
    request_time: std::time::Instant,
) -> Result<CurrentIntent> {
    let result = timeout(Duration::from_secs(10), async {
        // Short initial delay to give coordinator time to process and respond
        sleep(Duration::from_millis(CURRENT_INTENT_INITIAL_WAIT_MS)).await;

        loop {
            let messages = transport.receive_from(coordinator).await?;
            for msg in messages {
                if let WireMsg::CurrentIntent(current_intent) = msg {
                    // We accept this message since Transport handles deduplication
                    // and we've already cleared old messages before sending our request
                    info!(
                        "Received CurrentIntent for batch {} after {}ms from request",
                        current_intent.intent.intent_id,
                        request_time.elapsed().as_millis()
                    );
                    return Ok::<CurrentIntent, BatchError>(current_intent);
                }
            }

            // Poll with moderate frequency to balance responsiveness and CPU usage
            sleep(Duration::from_millis(CURRENT_INTENT_POLL_INTERVAL_MS)).await;
        }
    })
    .await;

    match result {
        Ok(Ok(ci)) => Ok(ci),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(BatchError::Timeout("current_intent".to_string())),
    }
}

async fn wait_for_template(
    transport: &Transport,
    coordinator: &str,
    intent_id: Uuid,
    deadline_ms: u64,
) -> Result<Template> {
    // Wait for the full deadline period plus 60 seconds buffer for:
    // - Network/DHT delays
    // - Coordinator processing time (verification, transaction building, PSBT creation)
    // - Message propagation
    let timeout_ms = deadline_ms + 60_000;

    info!(
        "Template timeout set to {}ms (deadline {}ms + 60s buffer)",
        timeout_ms, deadline_ms
    );

    let result = timeout(Duration::from_millis(timeout_ms), async {
        loop {
            // Poll more frequently for template to reduce latency
            sleep(Duration::from_millis(TEMPLATE_POLL_INTERVAL_MS)).await;

            let messages = transport.receive_from(coordinator).await?;
            for msg in messages {
                match msg {
                    WireMsg::Template(template) if template.intent_id == intent_id => {
                        return Ok::<Template, BatchError>(template);
                    }
                    WireMsg::Reject(reject) if reject.intent_id == intent_id => {
                        return Err(BatchError::ParticipantDropped(reject.reason));
                    }
                    _ => {}
                }
            }
        }
    })
    .await;

    match result {
        Ok(Ok(t)) => Ok(t),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(BatchError::Timeout("template".to_string())),
    }
}

async fn wait_for_reveal_request(
    transport: &Transport,
    coordinator: &str,
    intent_id: Uuid,
    deadline_ms: u64,
) -> Result<RequestReveal> {
    let result = timeout(Duration::from_millis(deadline_ms), async {
        loop {
            // Poll frequently for reveal request to minimize latency
            sleep(Duration::from_millis(REQUEST_REVEAL_POLL_INTERVAL_MS)).await;

            let messages = transport.receive_from(coordinator).await?;
            for msg in messages {
                match msg {
                    WireMsg::RequestReveal(request) => {
                        if request.intent_id == intent_id {
                            return Ok::<RequestReveal, BatchError>(request);
                        }
                    }
                    WireMsg::Reject(reject) => {
                        if reject.intent_id == intent_id {
                            return Err(BatchError::Other(format!(
                                "Rejected by coordinator: {}",
                                reject.reason
                            )));
                        }
                    }
                    _ => {}
                }
            }
        }
    })
    .await;

    match result {
        Ok(Ok(r)) => Ok(r),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(BatchError::Timeout("reveal_request".to_string())),
    }
}

async fn wait_for_final_tx(
    transport: &Transport,
    coordinator: &str,
    intent_id: Uuid,
) -> Result<FinalTx> {
    let result = timeout(Duration::from_secs(60), async {
        loop {
            // Poll moderately for final transaction (less critical path)
            sleep(Duration::from_millis(FINAL_TX_POLL_INTERVAL_MS)).await;

            let messages = transport.receive_from(coordinator).await?;
            for msg in messages {
                if let WireMsg::FinalTx(final_tx) = msg {
                    if final_tx.intent_id == intent_id {
                        return Ok::<FinalTx, BatchError>(final_tx);
                    }
                }
            }
        }
    })
    .await;

    match result {
        Ok(Ok(t)) => Ok(t),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(BatchError::Timeout("final_tx".to_string())),
    }
}

fn parse_payments(payments_str: &str) -> Result<Vec<PaymentOutput>> {
    let mut payments = Vec::new();

    for pair in payments_str.split(',') {
        let parts: Vec<&str> = pair.split(':').collect();
        if parts.len() != 2 {
            return Err(BatchError::Other(format!(
                "Invalid payment format: {}",
                pair
            )));
        }

        let address = parts[0].to_string();
        let amount_sats = parts[1]
            .parse::<u64>()
            .map_err(|e| BatchError::Other(format!("Invalid amount: {}", e)))?;

        payments.push(PaymentOutput {
            address,
            amount_sats,
        });
    }

    Ok(payments)
}

fn parse_selected_utxos(select_str: &str) -> Result<Vec<(String, u32)>> {
    let mut utxos = Vec::new();

    for pair in select_str.split(',') {
        let parts: Vec<&str> = pair.split(':').collect();
        if parts.len() != 2 {
            return Err(BatchError::Other(format!("Invalid UTXO format: {}", pair)));
        }

        let txid = parts[0].to_string();
        let vout = parts[1]
            .parse::<u32>()
            .map_err(|e| BatchError::Other(format!("Invalid vout: {}", e)))?;

        utxos.push((txid, vout));
    }

    Ok(utxos)
}

fn build_inputs_from_wallet(wallet: &Wallet<MemoryDatabase>) -> Result<Vec<InputProposal>> {
    let unspent = wallet
        .list_unspent()
        .map_err(|e| BatchError::Other(format!("Failed to list unspent: {}", e)))?;

    if unspent.is_empty() {
        return Err(BatchError::Other(
            "No UTXOs available in wallet".to_string(),
        ));
    }

    // Get external descriptor from wallet (for now, we'll use a simple approach)
    // In production, you'd track which descriptor each UTXO belongs to
    let external_desc = format!(
        "{}",
        wallet.get_descriptor_for_keychain(bdk::KeychainKind::External)
    );

    let mut inputs = Vec::new();

    for utxo in unspent {
        inputs.push(InputProposal {
            outpoint: OutPointSerde {
                txid: utxo.outpoint.txid.to_string(),
                vout: utxo.outpoint.vout,
            },
            witness_utxo: TxOutSerde {
                value_sats: utxo.txout.value,
                script_pubkey: hex::encode(utxo.txout.script_pubkey.as_bytes()),
            },
            descriptor: external_desc.clone(),
        });
    }

    info!(
        "Selected {} UTXOs for total of {} sats",
        inputs.len(),
        inputs
            .iter()
            .map(|i| i.witness_utxo.value_sats)
            .sum::<u64>()
    );

    Ok(inputs)
}

fn build_inputs_from_selection(
    selected: &[(String, u32)],
    wallet: &Wallet<MemoryDatabase>,
    _blockchain: &ElectrumBlockchain,
) -> Result<Vec<InputProposal>> {
    let unspent = wallet
        .list_unspent()
        .map_err(|e| BatchError::Other(format!("Failed to list unspent: {}", e)))?;

    let external_desc = format!(
        "{}",
        wallet.get_descriptor_for_keychain(bdk::KeychainKind::External)
    );

    let mut inputs = Vec::new();

    for (txid_str, vout) in selected {
        // Find the UTXO in wallet's unspent list
        let utxo = unspent
            .iter()
            .find(|u| u.outpoint.txid.to_string() == *txid_str && u.outpoint.vout == *vout)
            .ok_or_else(|| {
                BatchError::Other(format!("UTXO not found in wallet: {}:{}", txid_str, vout))
            })?;

        inputs.push(InputProposal {
            outpoint: OutPointSerde {
                txid: txid_str.clone(),
                vout: *vout,
            },
            witness_utxo: TxOutSerde {
                value_sats: utxo.txout.value,
                script_pubkey: hex::encode(utxo.txout.script_pubkey.as_bytes()),
            },
            descriptor: external_desc.clone(),
        });
    }

    info!(
        "Selected {} specific UTXOs for total of {} sats",
        inputs.len(),
        inputs
            .iter()
            .map(|i| i.witness_utxo.value_sats)
            .sum::<u64>()
    );

    Ok(inputs)
}

fn verify_template(template: &Template, reveal: &Reveal) -> Result<()> {
    // Deserialize unsigned tx
    let unsigned_tx: bitcoin::Transaction = deserialize(
        &hex::decode(&template.unsigned_tx_hex)
            .map_err(|e| BatchError::InvalidPsbt(format!("Invalid tx hex: {}", e)))?,
    )
    .map_err(|e| BatchError::InvalidPsbt(format!("Failed to deserialize tx: {}", e)))?;

    // Verify all our inputs are present
    for input_proposal in &reveal.inputs {
        let outpoint = input_proposal
            .outpoint
            .to_outpoint()
            .map_err(|e| BatchError::InvalidPsbt(format!("Invalid outpoint: {}", e)))?;

        let found = unsigned_tx
            .input
            .iter()
            .any(|tx_in| tx_in.previous_output == outpoint);

        if !found {
            return Err(BatchError::InvalidPsbt(format!(
                "Template missing our input: {}:{}",
                input_proposal.outpoint.txid, input_proposal.outpoint.vout
            )));
        }
    }

    // Verify all our payment outputs are present
    for payment in &reveal.payments {
        let addr = Address::from_str(&payment.address)
            .map_err(|e| BatchError::InvalidPsbt(format!("Invalid address: {}", e)))?
            .assume_checked(); // Assume network is already validated in assembler

        let found = unsigned_tx.output.iter().any(|tx_out| {
            tx_out.script_pubkey == addr.script_pubkey() && tx_out.value == payment.amount_sats
        });

        if !found {
            return Err(BatchError::InvalidPsbt(format!(
                "Template missing our payment output: {} sats to {}",
                payment.amount_sats, payment.address
            )));
        }
    }

    // CRITICAL: Verify our change address is present if we specified one
    if let Some(ref change_addr_str) = reveal.change_address {
        let change_addr = Address::from_str(change_addr_str)
            .map_err(|e| BatchError::InvalidPsbt(format!("Invalid change address: {}", e)))?
            .assume_checked(); // Assume network is already validated in assembler

        // Calculate expected change amount
        let input_sum: u64 = reveal
            .inputs
            .iter()
            .map(|i| i.witness_utxo.value_sats)
            .sum();
        let payment_sum: u64 = reveal.payments.iter().map(|p| p.amount_sats).sum();

        // We can't know the exact fee share here, but we can verify the change address exists
        // and has a reasonable amount (should be at least dust threshold if present)
        let found_change = unsigned_tx
            .output
            .iter()
            .find(|tx_out| tx_out.script_pubkey == change_addr.script_pubkey());

        match found_change {
            Some(change_out) => {
                // Verify change amount is reasonable
                if change_out.value < 546 {
                    return Err(BatchError::InvalidPsbt(format!(
                        "Change output below dust threshold: {} sats",
                        change_out.value
                    )));
                }

                // Sanity check: change should be less than our total input
                if change_out.value >= input_sum {
                    return Err(BatchError::InvalidPsbt(format!(
                        "Change amount {} exceeds our input sum {}",
                        change_out.value, input_sum
                    )));
                }

                // Sanity check: change + payments shouldn't exceed inputs
                if change_out.value + payment_sum > input_sum {
                    return Err(BatchError::InvalidPsbt(format!(
                        "Change + payments ({}) exceeds our inputs ({})",
                        change_out.value + payment_sum,
                        input_sum
                    )));
                }

                info!(
                    "Change output verified: {} sats to {}",
                    change_out.value, change_addr_str
                );
            }
            None => {
                // Change address specified but not in template
                // This is OK if change amount would be below dust (< 546 sats)
                let max_possible_change = input_sum.saturating_sub(payment_sum);
                if max_possible_change >= 546 {
                    return Err(BatchError::InvalidPsbt(format!(
                        "Change address specified but not found in template. Expected change: ~{} sats to {}",
                        max_possible_change, change_addr_str
                    )));
                } else {
                    warn!("Change address specified but omitted from template (amount would be dust: ~{} sats)", max_possible_change);
                }
            }
        }
    }

    info!("Template verification passed");
    Ok(())
}

/// Convert mnemonic to BIP84 descriptors
fn mnemonic_to_descriptors(
    mnemonic_words: &str,
    network: Network,
    account: u32,
) -> Result<(String, String)> {
    // Parse mnemonic
    let mnemonic = Mnemonic::parse_in(Language::English, mnemonic_words)
        .map_err(|e| BatchError::Other(format!("Invalid mnemonic: {}", e)))?;

    // Convert to extended key
    let xkey: ExtendedKey = mnemonic
        .into_extended_key()
        .map_err(|e| BatchError::Other(format!("Failed to derive extended key: {}", e)))?;

    // Get xprv for the network
    let xprv = xkey
        .into_xprv(network)
        .ok_or_else(|| BatchError::Other("Failed to get xprv".to_string()))?;

    // Use BIP84 template for P2WPKH
    // External descriptor: wpkh([fingerprint/84'/coin_type'/account']xprv/0/*)
    // Internal descriptor: wpkh([fingerprint/84'/coin_type'/account']xprv/1/*)

    // Get coin type based on network
    let coin_type = match network {
        Network::Bitcoin => 0,
        Network::Testnet | Network::Signet | Network::Regtest => 1,
        _ => 1,
    };

    // Build descriptors manually with BIP84 derivation path
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let fingerprint = xprv.fingerprint(&secp);
    let derivation_path = format!("84'/{}'/{}'", coin_type, account);

    // Derive account-level xprv
    use bdk::bitcoin::bip32::DerivationPath;
    let path_str = format!("m/84h/{}h/{}h", coin_type, account);
    let path: DerivationPath = path_str
        .parse()
        .map_err(|e| BatchError::Other(format!("Invalid derivation path: {}", e)))?;

    let account_xprv = xprv
        .derive_priv(&bitcoin::secp256k1::Secp256k1::new(), &path)
        .map_err(|e| BatchError::Other(format!("Derivation failed: {}", e)))?;

    // Build descriptors
    let external_desc = format!(
        "wpkh([{}/{}]{}/0/*)",
        fingerprint, derivation_path, account_xprv
    );

    let internal_desc = format!(
        "wpkh([{}/{}]{}/1/*)",
        fingerprint, derivation_path, account_xprv
    );

    Ok((external_desc, internal_desc))
}
