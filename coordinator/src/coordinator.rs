use bdk::electrum_client::{Client as ElectrumClient, ElectrumApi};
use bitcoin::{consensus::encode::serialize_hex, Network};
use common::{
    assembler::BatchAssembler, commitment::verify_commitment, messages::*,
    psbt_builder::PsbtBuilder, transport::Transport, BatchError, Result,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, timeout, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::ban_manager::{BanManager, Offense};

// Re-export the multi-batch coordinator
pub use crate::coordinator_v2::run_multi_batch;

// Configurable polling intervals (in milliseconds)
// These control how frequently the coordinator checks for new messages during each phase
const COMMITMENT_POLL_INTERVAL_MS: u64 = 100; // How often to check for new commitments
const REVEAL_POLL_INTERVAL_MS: u64 = 100; // How often to check for new reveals
const SIGNATURE_POLL_INTERVAL_MS: u64 = 100; // How often to check for new signatures

pub async fn run(
    recovery_method: &str,
    recovery_value: &str,
    pass: &str,
    network_str: &str,
    fee_rate_sat_vb: u64,
    min_participants: usize,
    max_participants: usize,
    deadline_ms: u64,
    allow_change: bool,
    electrum_host: &str,
    electrum_port: u16,
    electrum_proto: &str,
    participants_str: Option<&str>,
    seed_follows_str: Option<&str>,
    broadcast_to_followers: bool,
    ban_manager: Arc<BanManager>,
) -> Result<()> {
    info!("Starting coordinator");
    info!(
        "Network: {}, Fee rate: {} sat/vB",
        network_str, fee_rate_sat_vb
    );
    info!(
        "Participants: {}-{}, Deadline: {}ms",
        min_participants, max_participants, deadline_ms
    );

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

    // Connect to Electrum server
    info!(
        "Connecting to Electrum server at {}://{}:{}...",
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

    // Derive coordinator public key from messenger
    let coordinator_pkarr = transport.public_key_string();
    info!("Coordinator pubky: {}", coordinator_pkarr);

    // Clear any old messages from previous runs
    info!("Clearing old messages from previous coordinator sessions...");
    if let Err(e) = transport.clear_all_messages().await {
        warn!("Failed to clear old messages: {}", e);
        // Continue anyway - this is just cleanup
    }

    // Seed follow list if provided
    if let Some(seed_list) = seed_follows_str {
        let pubkys: Vec<String> = seed_list
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if !pubkys.is_empty() {
            info!("Seeding follow list with {} pubkys...", pubkys.len());
            match transport.seed_follows_from_list(pubkys).await {
                Ok(()) => info!("Follow list seeded successfully"),
                Err(e) => warn!("Failed to seed follow list: {}", e),
            }
        }
    }

    // Discover peers from Pubky follow graph
    info!("Discovering peers from Pubky follow graph...");
    match transport.discover_peers().await {
        Ok(discovered) => {
            if discovered.is_empty() {
                info!("No peers discovered from follow graph");
            } else {
                info!("Discovered {} peers from follow graph:", discovered.len());
                for peer in &discovered {
                    info!("  - {}", peer);
                }
            }
        }
        Err(e) => {
            warn!("Failed to discover peers from follow graph: {}", e);
        }
    }

    // Add manually specified participants to transport
    if let Some(participants) = participants_str {
        for participant_pkarr in participants.split(',') {
            let participant_pkarr = participant_pkarr.trim();
            if !participant_pkarr.is_empty() {
                transport.add_known_peer(participant_pkarr.to_string());
                info!("Added known participant (manual): {}", participant_pkarr);
            }
        }
    }

    // Generate intent ID
    let intent_id = Uuid::new_v4();
    info!("Intent ID: {}", intent_id);

    // 1️⃣ Broadcast Intent
    let intent = Intent {
        intent_id,
        network: NetworkSpec::from_bdk_network(network),
        fee_rate_sat_vb,
        min_participants,
        max_participants,
        deadline_ms,
        allow_change,
        coordinator_pkarr: coordinator_pkarr.to_string(),
        fee_model: FeeModel::WeightBased, // Using fair weight-based fee calculation
    };

    info!("Intent created");
    // Optionally broadcast intent to all discovered peers
    if broadcast_to_followers {
        info!(
            "Broadcasting intent to {} discovered peers...",
            transport.get_known_peers().len()
        );
        let current_intent = CurrentIntent {
            intent: intent.clone(),
            status: BatchStatus::Filling,
            current_participants: 0,
        };

        for peer in transport.get_known_peers() {
            if let Err(e) = transport
                .send_dm(&peer, &WireMsg::CurrentIntent(current_intent.clone()))
                .await
            {
                debug!("Failed to broadcast intent to {}: {}", peer, e);
            } else {
                info!("Broadcast intent to {}", peer);
            }
        }
    } else {
        info!("Not broadcasting to followers (use --broadcast-to-followers to enable)");
    }

    // 2️⃣ Collect Commitments
    info!(
        "Phase 1: Collecting commitments (min: {}, max: {}, deadline: {}ms)...",
        min_participants, max_participants, deadline_ms
    );
    let mut commitments: HashMap<String, Commitment> = HashMap::new();

    // Collect commitments with proper deadline handling
    let commitment_result = collect_commitments_with_deadline(
        &transport,
        &intent,
        &mut commitments,
        ban_manager.clone(),
    )
    .await;

    match commitment_result {
        Ok(_) => {
            info!("Successfully collected {} commitments", commitments.len());
        }
        Err(_e) => {
            // Check if we have at least minimum participants
            if commitments.len() < min_participants {
                error!(
                    "Failed to reach minimum participants by deadline: {} < {}",
                    commitments.len(),
                    min_participants
                );

                // Notify any participants who committed that the batch is abandoned
                for participant in commitments.keys() {
                    let reject = Reject {
                        intent_id,
                        participant_pkarr: participant.clone(),
                        reason: format!(
                            "Batch abandoned: insufficient participants ({} < {})",
                            commitments.len(),
                            min_participants
                        ),
                    };
                    let _ = transport
                        .send_dm(participant, &WireMsg::Reject(reject))
                        .await;
                }

                return Err(BatchError::Other(format!(
                    "Batch abandoned: insufficient participants ({} < {}) by deadline",
                    commitments.len(),
                    min_participants
                )));
            }
            // If we have at least min but hit deadline, continue
            info!(
                "Deadline reached with {} participants (min: {}, max: {})",
                commitments.len(),
                min_participants,
                max_participants
            );
        }
    }

    info!(
        "Commitment phase complete with {} participants",
        commitments.len()
    );

    // 3️⃣ Request Reveals from committed participants
    info!(
        "Phase 2: Requesting reveals from {} committed participants...",
        commitments.len()
    );
    let request_reveal = RequestReveal {
        intent_id,
        committed_participants: commitments.keys().cloned().collect(),
    };

    // Send reveal request to all committed participants
    for participant in commitments.keys() {
        info!("Sending RequestReveal to participant: {}", participant);
        match transport
            .send_dm(participant, &WireMsg::RequestReveal(request_reveal.clone()))
            .await
        {
            Ok(_) => info!("Successfully sent RequestReveal to {}", participant),
            Err(e) => error!("Failed to send RequestReveal to {}: {}", participant, e),
        }
    }

    // 4️⃣ Collect Reveals
    info!("Waiting for reveals from committed participants...");
    let mut reveals: HashMap<String, Reveal> = HashMap::new();

    let reveal_result = timeout(
        Duration::from_millis(deadline_ms / 2), // Use remaining half for reveals
        collect_reveals(
            &transport,
            intent_id,
            &commitments,
            &mut reveals,
            ban_manager.clone(),
        ),
    )
    .await;

    match reveal_result {
        Ok(_) => {
            info!("Collected {} reveals", reveals.len());
        }
        Err(_) => {
            warn!(
                "Reveal phase timed out, proceeding with {} reveals",
                reveals.len()
            );

            // Record offense for participants who failed to reveal
            for (participant, _) in &commitments {
                if !reveals.contains_key(participant) {
                    match ban_manager
                        .record_offense(participant, Offense::FailedToReveal { intent_id })
                        .await
                    {
                        Ok(crate::ban_manager::BanAction::PermanentBan) => {
                            // Unfollow permanently banned participants
                            if let Err(e) = transport.unfollow(participant).await {
                                warn!(
                                    "Failed to unfollow banned participant {}: {}",
                                    participant, e
                                );
                            }
                        }
                        _ => {}
                    }
                    warn!(
                        "Participant {} failed to reveal after commitment",
                        participant
                    );
                }
            }
        }
    }

    if reveals.len() < min_participants {
        error!(
            "Insufficient participants with valid reveals: {} < {}",
            reveals.len(),
            min_participants
        );
        return Err(BatchError::Other(format!(
            "Insufficient reveals: {} < {}",
            reveals.len(),
            min_participants
        )));
    }

    info!("Reveal phase complete with {} participants", reveals.len());

    // 5️⃣ Build Deterministic Template
    info!("Building deterministic transaction template...");
    let assembler = BatchAssembler::new(network, fee_rate_sat_vb, allow_change);

    // Validate all reveals
    // Note: Participants should validate locally before sending commitment.
    // Invalid reveals here indicate either a bug or malicious participant bypassing client-side validation.
    let mut invalid_participants = Vec::new();
    for (participant, reveal) in &reveals {
        if let Err(e) = assembler.validate_reveal(reveal) {
            error!("Invalid reveal from {}: {}", participant, e);

            // Check if this is an insufficient funds error - this indicates malicious bypass of client validation
            let error_str = e.to_string();
            if error_str.contains("Insufficient funds") {
                warn!(
                    "Participant {} bypassed client-side validation with insufficient funds!",
                    participant
                );

                // Calculate amounts for offense record
                let available = reveal
                    .inputs
                    .iter()
                    .map(|i| i.witness_utxo.value_sats)
                    .sum::<u64>();
                let required = reveal.payments.iter().map(|p| p.amount_sats).sum::<u64>();

                // Record the offense - this is a critical offense (permanent ban)
                match ban_manager
                    .record_offense(
                        participant,
                        Offense::InsufficientFunds {
                            intent_id,
                            available,
                            required,
                        },
                    )
                    .await
                {
                    Ok(crate::ban_manager::BanAction::PermanentBan) => {
                        warn!("Permanently banned participant {} for bypassing client validation with insufficient funds", participant);
                        // Unfollow permanently banned participants
                        if let Err(e) = transport.unfollow(participant).await {
                            warn!(
                                "Failed to unfollow banned participant {}: {}",
                                participant, e
                            );
                        }
                    }
                    Ok(action) => {
                        warn!(
                            "Banned participant {} for insufficient funds: {:?}",
                            participant, action
                        );
                    }
                    Err(e) => {
                        error!("Failed to record offense for {}: {}", participant, e);
                    }
                }
            }

            invalid_participants.push((participant.clone(), format!("Invalid reveal: {}", e)));
        }
    }

    // Verify all UTXOs exist on-chain
    info!("Verifying UTXOs on-chain...");
    for (participant, reveal) in &reveals {
        for input in &reveal.inputs {
            use bitcoin::ScriptBuf;

            // Parse script pubkey from hex
            let script = match ScriptBuf::from_hex(&input.witness_utxo.script_pubkey) {
                Ok(s) => s,
                Err(e) => {
                    error!("Invalid script_pubkey hex from {}: {}", participant, e);
                    invalid_participants.push((
                        participant.clone(),
                        format!("Invalid script_pubkey hex: {}", e),
                    ));
                    continue;
                }
            };

            // Check if UTXO exists
            match electrum_client.script_get_history(&script) {
                Ok(history) => {
                    let utxo_exists = history
                        .iter()
                        .any(|tx| tx.tx_hash.to_string() == input.outpoint.txid);

                    if !utxo_exists {
                        error!(
                            "UTXO not found for participant {}: {}:{}",
                            participant, input.outpoint.txid, input.outpoint.vout
                        );

                        // Record fake UTXO offense
                        match ban_manager
                            .record_offense(
                                participant,
                                Offense::FakeUtxo {
                                    intent_id,
                                    outpoint: format!(
                                        "{}:{}",
                                        input.outpoint.txid, input.outpoint.vout
                                    ),
                                },
                            )
                            .await
                        {
                            Ok(crate::ban_manager::BanAction::PermanentBan) => {
                                // Unfollow permanently banned participants
                                if let Err(e) = transport.unfollow(participant).await {
                                    warn!(
                                        "Failed to unfollow banned participant {}: {}",
                                        participant, e
                                    );
                                }
                            }
                            _ => {}
                        }

                        invalid_participants.push((
                            participant.clone(),
                            format!(
                                "UTXO not found: {}:{}",
                                input.outpoint.txid, input.outpoint.vout
                            ),
                        ));
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to verify UTXO for participant {}: {}",
                        participant, e
                    );
                    invalid_participants.push((
                        participant.clone(),
                        format!("UTXO verification failed: {}", e),
                    ));
                }
            }
        }
    }

    // Remove invalid reveals and notify participants
    for (participant, reason) in invalid_participants {
        reveals.remove(&participant);

        let reject = Reject {
            intent_id,
            participant_pkarr: participant.clone(),
            reason,
        };
        let _ = transport
            .send_dm(&participant, &WireMsg::Reject(reject))
            .await;
    }

    let (unsigned_tx, input_map) = assembler.build_transaction(&reveals)?;

    info!(
        "Built transaction: {} inputs, {} outputs",
        unsigned_tx.input.len(),
        unsigned_tx.output.len()
    );

    // Build PSBT template
    let psbt_builder = PsbtBuilder::new(network);
    let psbt_template = psbt_builder.build_template(unsigned_tx.clone(), &reveals, &input_map)?;

    let template = Template {
        intent_id,
        unsigned_tx_hex: serialize_hex(&unsigned_tx),
        psbt_base64: PsbtBuilder::to_base64(&psbt_template)?,
        input_map: input_map.clone(),
    };

    // Send template to all participants
    info!("Sending template to {} participants...", reveals.len());
    for participant in reveals.keys() {
        let _ = transport
            .send_dm(participant, &WireMsg::Template(template.clone()))
            .await;
    }

    // 6️⃣ Collect Signature Fragments
    info!("Waiting for signature fragments...");
    let mut sig_fragments: HashMap<String, SigFragment> = HashMap::new();

    let sig_result = timeout(
        Duration::from_millis(deadline_ms / 2),
        collect_sig_fragments(
            &transport,
            intent_id,
            &reveals,
            &mut sig_fragments,
            ban_manager.clone(),
        ),
    )
    .await;

    match sig_result {
        Ok(_) => {
            info!("Collected {} signature fragments", sig_fragments.len());
        }
        Err(_) => {
            warn!(
                "Signature phase timed out, proceeding with {} signatures",
                sig_fragments.len()
            );

            // Record offense for participants who failed to sign
            for (participant, _) in &reveals {
                if !sig_fragments.contains_key(participant) {
                    match ban_manager
                        .record_offense(participant, Offense::FailedToSign { intent_id })
                        .await
                    {
                        Ok(crate::ban_manager::BanAction::PermanentBan) => {
                            // Unfollow permanently banned participants
                            if let Err(e) = transport.unfollow(participant).await {
                                warn!(
                                    "Failed to unfollow banned participant {}: {}",
                                    participant, e
                                );
                            }
                        }
                        _ => {}
                    }
                    warn!("Participant {} failed to sign after reveal", participant);
                }
            }
        }
    }

    if sig_fragments.len() < min_participants {
        error!(
            "Insufficient signatures: {} < {}",
            sig_fragments.len(),
            min_participants
        );
        return Err(BatchError::Other(format!(
            "Insufficient signatures: {} < {}",
            sig_fragments.len(),
            min_participants
        )));
    }

    // 7️⃣ Merge and Finalize PSBT
    info!("Merging PSBT fragments...");
    let mut final_psbt = psbt_template.clone();

    let mut fragments = Vec::new();
    for (participant, frag) in &sig_fragments {
        match PsbtBuilder::from_base64(&frag.psbt_fragment_base64) {
            Ok(psbt) => {
                info!(
                    "Successfully deserialized PSBT fragment from {}",
                    participant
                );
                fragments.push(psbt);
            }
            Err(e) => {
                error!(
                    "Failed to deserialize PSBT fragment from {}: {}",
                    participant, e
                );
            }
        }
    }

    info!(
        "Successfully parsed {} of {} fragments",
        fragments.len(),
        sig_fragments.len()
    );
    psbt_builder.merge_fragments(&mut final_psbt, fragments)?;

    info!("Finalizing PSBT...");
    psbt_builder.finalize(&mut final_psbt)?;

    let final_tx = psbt_builder.extract_tx(&final_psbt)?;
    let txid = final_tx.txid();

    info!("Final transaction: {}", txid);
    info!(
        "Size: {} bytes, vsize: {} vB",
        final_tx.size(),
        final_tx.vsize()
    );

    // Broadcast transaction
    info!("Broadcasting transaction...");
    let raw_tx_hex = serialize_hex(&final_tx);

    match electrum_client.transaction_broadcast(&final_tx) {
        Ok(broadcast_txid) => {
            info!("Transaction broadcast successful!");
            info!("Broadcast TXID: {}", broadcast_txid);
            if broadcast_txid != txid {
                warn!(
                    "Broadcast TXID mismatch: expected {}, got {}",
                    txid, broadcast_txid
                );
            }
        }
        Err(e) => {
            error!("Failed to broadcast transaction: {}", e);
            info!("Raw TX (for manual broadcast): {}", raw_tx_hex);
            return Err(BatchError::Other(format!("Broadcast failed: {}", e)));
        }
    }

    // Send FinalTx to all participants
    let final_tx_msg = FinalTx {
        intent_id,
        txid: txid.to_string(),
        raw_tx_hex: raw_tx_hex.clone(),
    };

    for participant in reveals.keys() {
        let _ = transport
            .send_dm(participant, &WireMsg::FinalTx(final_tx_msg.clone()))
            .await;
    }

    info!("Batch coordination complete!");
    info!("TXID: {}", txid);
    info!("Participants: {}", reveals.len());

    // Clean up messages after successful batch
    info!("Cleaning up messages after successful batch...");
    for participant in reveals.keys() {
        if let Err(e) = transport.clear_messages_with_peer(participant).await {
            debug!(
                "Failed to clear messages with participant {}: {}",
                participant, e
            );
            // Continue - cleanup errors are non-fatal
        }
    }
    info!("Message cleanup complete");

    Ok(())
}

async fn collect_commitments_with_deadline(
    transport: &Transport,
    intent: &Intent,
    commitments: &mut HashMap<String, Commitment>,
    ban_manager: Arc<BanManager>,
) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_millis(intent.deadline_ms);
    let intent_id = intent.intent_id;
    let min = intent.min_participants;
    let max = intent.max_participants;

    info!("Starting commitment collection phase...");
    info!("- Minimum participants required: {}", min);
    info!("- Maximum participants allowed: {}", max);
    info!("- Deadline: {}ms", intent.deadline_ms);

    loop {
        // Check if we've hit the deadline
        if std::time::Instant::now() >= deadline {
            if commitments.len() >= min {
                info!(
                    "Deadline reached with {} participants (>= min)",
                    commitments.len()
                );
                return Ok(());
            } else {
                error!(
                    "Deadline reached with insufficient participants: {} < {}",
                    commitments.len(),
                    min
                );
                return Err(BatchError::Other(format!(
                    "Deadline reached with insufficient participants: {} < {}",
                    commitments.len(),
                    min
                )));
            }
        }

        // Poll for messages
        debug!("Polling for commitments from known peers...");
        let messages = transport.receive_all().await?;
        debug!("Received {} messages", messages.len());

        for (sender, msg) in messages {
            // Check if sender is banned
            if ban_manager.is_banned(&sender).await {
                debug!("Ignoring message from banned participant: {}", sender);
                continue;
            }

            debug!("Processing message from {}: {:?}", sender, msg);
            match msg {
                WireMsg::Commitment(commitment) => {
                    if commitment.intent_id == intent_id {
                        if !commitments.contains_key(&sender) {
                            info!(
                                "✅ Received commitment {} from {}",
                                commitments.len() + 1,
                                sender
                            );
                            commitments.insert(sender.clone(), commitment);

                            // Send Ack for successful commitment
                            let ack = Ack {
                                intent_id,
                                participant_pkarr: sender.clone(),
                                message: "Commitment received".to_string(),
                            };
                            let _ = transport.send_dm(&sender, &WireMsg::Ack(ack)).await;

                            // Check if we've reached maximum participants
                            if commitments.len() >= max {
                                info!("Maximum participants ({}) reached! Starting batch immediately.", max);
                                return Ok(());
                            }

                            // Update status
                            info!(
                                "Current participants: {}/{} (min: {})",
                                commitments.len(),
                                max,
                                min
                            );
                        }
                    } else {
                        debug!("Ignoring commitment with wrong intent_id");
                    }
                }
                WireMsg::GetCurrentIntent(_get_intent) => {
                    // Respond with current batch information
                    debug!("Received GetCurrentIntent from {}", sender);

                    let current_intent = CurrentIntent {
                        intent: intent.clone(),
                        status: if commitments.len() >= max {
                            BatchStatus::Full
                        } else if commitments.len() >= min {
                            BatchStatus::Ready
                        } else {
                            BatchStatus::Filling
                        },
                        current_participants: commitments.len(),
                    };

                    if let Err(e) = transport
                        .send_dm(&sender, &WireMsg::CurrentIntent(current_intent))
                        .await
                    {
                        error!("Failed to send CurrentIntent to {}: {}", sender, e);
                    } else {
                        info!("Sent CurrentIntent to {}", sender);
                    }
                }
                _ => {
                    debug!("Ignoring other message type during commitment phase");
                }
            }
        }

        // Poll for new commitments
        sleep(Duration::from_millis(COMMITMENT_POLL_INTERVAL_MS)).await;
    }
}

async fn collect_reveals(
    transport: &Transport,
    intent_id: Uuid,
    commitments: &HashMap<String, Commitment>,
    reveals: &mut HashMap<String, Reveal>,
    ban_manager: Arc<BanManager>,
) -> Result<()> {
    while reveals.len() < commitments.len() {
        // Poll for new reveals
        sleep(Duration::from_millis(REVEAL_POLL_INTERVAL_MS)).await;

        debug!(
            "Polling for reveals from {} committed participants (have {} so far)...",
            commitments.len(),
            reveals.len()
        );
        let messages = transport.receive_all().await?;
        debug!("Received {} messages", messages.len());

        for (sender, msg) in messages {
            // Check if sender is banned
            if ban_manager.is_banned(&sender).await {
                debug!("Ignoring message from banned participant: {}", sender);
                continue;
            }

            debug!("Processing message from {}: {:?}", sender, msg);
            if let WireMsg::Reveal(reveal) = msg {
                if reveal.intent_id == intent_id {
                    // Only accept reveals from participants who committed
                    if let Some(commitment) = commitments.get(&sender) {
                        // Verify the reveal matches the commitment
                        match verify_commitment(&reveal, &commitment.commitment_hash) {
                            Ok(_) => {
                                if !reveals.contains_key(&sender) {
                                    info!("✅ Verified reveal from {}", sender);
                                    reveals.insert(sender.clone(), reveal);

                                    // Send Ack for successful reveal
                                    let ack = Ack {
                                        intent_id,
                                        participant_pkarr: sender.clone(),
                                        message: "Reveal verified".to_string(),
                                    };
                                    let _ = transport.send_dm(&sender, &WireMsg::Ack(ack)).await;
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "Invalid reveal from {} (commitment mismatch): {}",
                                    sender, e
                                );

                                // Record the offense
                                match ban_manager
                                    .record_offense(
                                        &sender,
                                        Offense::InvalidCommitment { intent_id },
                                    )
                                    .await
                                {
                                    Ok(crate::ban_manager::BanAction::PermanentBan) => {
                                        // Unfollow permanently banned participants
                                        if let Err(e) = transport.unfollow(&sender).await {
                                            warn!(
                                                "Failed to unfollow banned participant {}: {}",
                                                sender, e
                                            );
                                        }
                                    }
                                    _ => {}
                                }

                                // Send rejection
                                let reject = Reject {
                                    intent_id,
                                    participant_pkarr: sender.clone(),
                                    reason: format!("Reveal does not match commitment: {}", e),
                                };
                                let _ = transport.send_dm(&sender, &WireMsg::Reject(reject)).await;
                            }
                        }
                    } else {
                        warn!("Reveal from non-committed participant: {}", sender);
                        // Send rejection
                        let reject = Reject {
                            intent_id,
                            participant_pkarr: sender.clone(),
                            reason: "You did not commit in the commitment phase".to_string(),
                        };
                        let _ = transport.send_dm(&sender, &WireMsg::Reject(reject)).await;
                    }
                } else {
                    debug!(
                        "Ignoring reveal with wrong intent_id: {} (expected {})",
                        reveal.intent_id, intent_id
                    );
                }
            }
        }
    }
    Ok(())
}

async fn collect_sig_fragments(
    transport: &Transport,
    intent_id: Uuid,
    reveals: &HashMap<String, Reveal>,
    fragments: &mut HashMap<String, SigFragment>,
    ban_manager: Arc<BanManager>,
) -> Result<()> {
    while fragments.len() < reveals.len() {
        // Poll for new signature fragments
        sleep(Duration::from_millis(SIGNATURE_POLL_INTERVAL_MS)).await;

        let messages = transport.receive_all().await?;
        for (sender, msg) in messages {
            // Check if sender is banned
            if ban_manager.is_banned(&sender).await {
                debug!("Ignoring message from banned participant: {}", sender);
                continue;
            }

            if let WireMsg::SigFragment(frag) = msg {
                if frag.intent_id == intent_id && reveals.contains_key(&sender) {
                    debug!("Received signature fragment from {}", sender);
                    fragments.insert(sender, frag);
                }
            }
        }
    }
    Ok(())
}
