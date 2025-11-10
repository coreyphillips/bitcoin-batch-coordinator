use crate::ban_manager::{BanManager, Offense};
use crate::batch_manager::BatchManager;
use bdk::electrum_client::{Client as ElectrumClient, ElectrumApi};
use bitcoin::{consensus::encode::serialize_hex, Network};
use common::{
    assembler::BatchAssembler, commitment::verify_commitment, messages::*,
    psbt_builder::PsbtBuilder, transport::Transport, BatchError, Result,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Main coordinator loop that manages multiple concurrent batches
pub async fn run_multi_batch(
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
    info!("Starting multi-batch coordinator");
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

    // Wrap ElectrumClient in Arc<Mutex<>> for thread-safe sharing across batches
    let electrum_client = Arc::new(Mutex::new(electrum_client));

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

    let coordinator_pkarr = transport.public_key_string();
    info!("Coordinator pubky: {}", coordinator_pkarr);

    // Clear old messages
    info!("Clearing old messages from previous coordinator sessions...");
    if let Err(e) = transport.clear_all_messages().await {
        warn!("Failed to clear old messages: {}", e);
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

    // Discover peers
    info!("Discovering peers from Pubky follow graph...");
    match transport.discover_peers().await {
        Ok(discovered) => {
            if !discovered.is_empty() {
                info!("Discovered {} peers from follow graph", discovered.len());

                // Optionally broadcast initial intent to discovered peers
                if broadcast_to_followers {
                    info!("Broadcasting initial intent to discovered peers...");
                    // Note: Initial intent will be sent when batch is created
                    info!("Broadcast will occur when first batch is created");
                } else {
                    info!("Not broadcasting to followers (use --broadcast-to-followers to enable)");
                }
            }
        }
        Err(e) => {
            warn!("Failed to discover peers: {}", e);
        }
    }

    // Add manually specified participants
    if let Some(participants) = participants_str {
        for participant_pkarr in participants.split(',') {
            let participant_pkarr = participant_pkarr.trim();
            if !participant_pkarr.is_empty() {
                transport.add_known_peer(participant_pkarr.to_string());
                info!("Added known participant: {}", participant_pkarr);
            }
        }
    }

    // Wrap Transport in Arc<Mutex<>> for thread-safe concurrent operations
    let transport = Arc::new(tokio::sync::Mutex::new(transport));

    // Initialize BatchManager
    let batch_manager = Arc::new(BatchManager::new(
        NetworkSpec::from_bdk_network(network),
        fee_rate_sat_vb,
        min_participants,
        max_participants,
        deadline_ms,
        allow_change,
        coordinator_pkarr.clone(),
    ));

    // Create initial batch
    let initial_intent = batch_manager.create_new_batch().await?;
    info!(
        "Created initial batch with intent_id: {}",
        initial_intent.intent_id
    );

    // Start main coordinator loop
    loop {
        // Check for new messages
        let messages = {
            let transport_lock = transport.lock().await;
            transport_lock.receive_all().await?
        };

        if !messages.is_empty() {
            debug!("Received {} messages this iteration", messages.len());
        }

        for (sender, msg) in messages {
            // Check if sender is banned
            if ban_manager.is_banned(&sender).await {
                debug!("Ignoring message from banned participant: {}", sender);
                continue;
            }

            match msg {
                // Handle requests for current intent
                WireMsg::GetCurrentIntent(get_intent) => {
                    info!(
                        "Received GetCurrentIntent from {} (participant: {})",
                        sender, get_intent.participant_pkarr
                    );

                    // Get or create current batch
                    let current_intent = batch_manager.get_or_create_current_intent().await?;

                    // Send response
                    let response = WireMsg::CurrentIntent(current_intent);
                    let send_result = {
                        let transport_lock = transport.lock().await;
                        transport_lock.send_dm(&sender, &response).await
                    };
                    if let Err(e) = send_result {
                        error!("Failed to send current intent to {}: {}", sender, e);
                    } else {
                        info!(
                            "Sent current intent {} to {}",
                            if let WireMsg::CurrentIntent(ref ci) = response {
                                ci.intent.intent_id
                            } else {
                                Uuid::nil()
                            },
                            sender
                        );
                    }
                }

                // Handle commitments
                WireMsg::Commitment(commitment) => {
                    info!(
                        "Received commitment from {} for intent {}",
                        sender, commitment.intent_id
                    );

                    // Try to add to batch
                    match batch_manager
                        .add_commitment(commitment.intent_id, commitment.clone())
                        .await
                    {
                        Ok(_) => {
                            // Send acknowledgment
                            let ack = Ack {
                                intent_id: commitment.intent_id,
                                participant_pkarr: sender.clone(),
                                message: "Commitment received".to_string(),
                            };
                            {
                                let transport_lock = transport.lock().await;
                                let _ = transport_lock.send_dm(&sender, &WireMsg::Ack(ack)).await;
                            }

                            // Check if batch needs processing
                            if let Some(batch) = batch_manager.get_batch(commitment.intent_id).await
                            {
                                if batch.is_full()
                                    || (batch.has_minimum()
                                        && batch.created_at.elapsed().as_millis() as u64
                                            >= deadline_ms)
                                {
                                    // Mark batch as ready for processing
                                    info!("Batch {} ready for processing", commitment.intent_id);
                                    batch_manager
                                        .update_batch_status(
                                            commitment.intent_id,
                                            BatchStatus::Processing,
                                        )
                                        .await?;

                                    // Process will happen in next iteration of the main loop
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to add commitment from {}: {}", sender, e);
                            let reject = Reject {
                                intent_id: commitment.intent_id,
                                participant_pkarr: sender.clone(),
                                reason: format!("Failed to add commitment: {}", e),
                            };
                            {
                                let transport_lock = transport.lock().await;
                                let _ = transport_lock
                                    .send_dm(&sender, &WireMsg::Reject(reject))
                                    .await;
                            }
                        }
                    }
                }

                // Handle reveals (will be processed by batch processors)
                WireMsg::Reveal(reveal) => {
                    debug!(
                        "Received reveal from {} for intent {}",
                        sender, reveal.intent_id
                    );
                    // Reveals are handled by the batch processor tasks
                }

                _ => {
                    debug!("Received other message type from {}", sender);
                }
            }
        }

        // Check batch deadlines and mark for processing if needed
        let active_batches = batch_manager.get_active_batches().await;
        for (intent_id, status, participant_count) in active_batches.clone() {
            if let Some(batch) = batch_manager.get_batch(intent_id).await {
                if matches!(status, BatchStatus::Filling | BatchStatus::Ready) {
                    let age = batch.created_at.elapsed().as_millis() as u64;
                    if age >= deadline_ms && batch.has_minimum() {
                        info!(
                            "Batch {} reached deadline with {} participants",
                            intent_id, participant_count
                        );
                        batch_manager
                            .update_batch_status(intent_id, BatchStatus::Processing)
                            .await?;
                    }
                }
            }
        }

        // Process any batches marked as Processing
        for (intent_id, status, _) in active_batches {
            if matches!(status, BatchStatus::Processing) {
                info!("Processing batch {}", intent_id);
                if let Err(e) = process_batch(
                    batch_manager.clone(),
                    transport.clone(),
                    electrum_client.clone(),
                    network,
                    intent_id,
                    ban_manager.clone(),
                )
                .await
                {
                    error!("Failed to process batch {}: {}", intent_id, e);
                    // Mark as completed even on error to avoid reprocessing
                    batch_manager
                        .update_batch_status(intent_id, BatchStatus::Completed)
                        .await?;
                }
            }
        }

        // Clean up old completed batches (older than 5 minutes)
        batch_manager.cleanup_completed_batches(300).await;

        // Very short delay to prevent CPU spinning while maintaining responsiveness
        sleep(Duration::from_millis(10)).await;
    }
}

/// Process a single batch through reveal, template, signing, and broadcast
async fn process_batch(
    batch_manager: Arc<BatchManager>,
    transport: Arc<Mutex<Transport>>,
    electrum_client: Arc<Mutex<ElectrumClient>>,
    network: Network,
    intent_id: Uuid,
    ban_manager: Arc<BanManager>,
) -> Result<()> {
    info!("Starting batch processing for intent {}", intent_id);

    // Update status to processing
    batch_manager
        .update_batch_status(intent_id, BatchStatus::Processing)
        .await?;

    // Get the batch
    let batch = batch_manager
        .get_batch(intent_id)
        .await
        .ok_or_else(|| BatchError::Other(format!("Batch {} not found", intent_id)))?;

    // Request reveals from committed participants
    info!(
        "Requesting reveals from {} participants",
        batch.commitments.len()
    );
    let request_reveal = RequestReveal {
        intent_id,
        committed_participants: batch.commitments.keys().cloned().collect(),
    };

    for participant in batch.commitments.keys() {
        let transport_lock = transport.lock().await;
        if let Err(e) = transport_lock
            .send_dm(participant, &WireMsg::RequestReveal(request_reveal.clone()))
            .await
        {
            error!("Failed to send RequestReveal to {}: {}", participant, e);
        }
    }

    // Collect reveals
    let mut reveals = HashMap::new();
    let reveal_timeout = Duration::from_millis(batch.intent.deadline_ms / 2);
    let reveal_deadline = std::time::Instant::now() + reveal_timeout;

    while reveals.len() < batch.commitments.len() && std::time::Instant::now() < reveal_deadline {
        let messages = {
            let transport_lock = transport.lock().await;
            transport_lock.receive_all().await?
        };

        for (sender, msg) in messages {
            // Check if sender is banned
            if ban_manager.is_banned(&sender).await {
                debug!("Ignoring message from banned participant: {}", sender);
                continue;
            }

            if let WireMsg::Reveal(reveal) = msg {
                if reveal.intent_id == intent_id {
                    if let Some(commitment) = batch.commitments.get(&sender) {
                        match verify_commitment(&reveal, &commitment.commitment_hash) {
                            Ok(_) => {
                                if !reveals.contains_key(&sender) {
                                    info!("✅ Verified reveal from {}", sender);
                                    reveals.insert(sender.clone(), reveal.clone());
                                    batch_manager.add_reveal(intent_id, reveal).await?;

                                    let ack = Ack {
                                        intent_id,
                                        participant_pkarr: sender.clone(),
                                        message: "Reveal verified".to_string(),
                                    };
                                    {
                                        let transport_lock = transport.lock().await;
                                        let _ = transport_lock
                                            .send_dm(&sender, &WireMsg::Ack(ack))
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Invalid reveal from {}: {}", sender, e);

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
                                        let transport_lock = transport.lock().await;
                                        if let Err(e) = transport_lock.unfollow(&sender).await {
                                            warn!(
                                                "Failed to unfollow banned participant {}: {}",
                                                sender, e
                                            );
                                        }
                                    }
                                    _ => {}
                                }

                                let reject = Reject {
                                    intent_id,
                                    participant_pkarr: sender.clone(),
                                    reason: format!("Reveal does not match commitment: {}", e),
                                };
                                {
                                    let transport_lock = transport.lock().await;
                                    let _ = transport_lock
                                        .send_dm(&sender, &WireMsg::Reject(reject))
                                        .await;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Poll frequently during reveal collection to minimize latency
        sleep(Duration::from_millis(25)).await;
    }

    // Record offense for participants who failed to reveal
    for (participant, _) in &batch.commitments {
        if !reveals.contains_key(participant) {
            match ban_manager
                .record_offense(participant, Offense::FailedToReveal { intent_id })
                .await
            {
                Ok(crate::ban_manager::BanAction::PermanentBan) => {
                    // Unfollow permanently banned participants
                    let transport_lock = transport.lock().await;
                    if let Err(e) = transport_lock.unfollow(participant).await {
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

    if reveals.len() < batch.intent.min_participants {
        error!(
            "Insufficient reveals for batch {}: {} < {}",
            intent_id,
            reveals.len(),
            batch.intent.min_participants
        );
        batch_manager
            .update_batch_status(intent_id, BatchStatus::Completed)
            .await?;
        return Err(BatchError::Other(format!(
            "Insufficient reveals: {} < {}",
            reveals.len(),
            batch.intent.min_participants
        )));
    }

    info!(
        "Collected {} reveals for batch {}",
        reveals.len(),
        intent_id
    );

    // Build transaction
    info!("Building transaction for batch {}", intent_id);
    let assembler = BatchAssembler::new(
        network,
        batch.intent.fee_rate_sat_vb,
        batch.intent.allow_change,
    );

    // Validate reveals and verify UTXOs
    let mut invalid_participants = Vec::new();
    for (participant, reveal) in &reveals {
        if let Err(e) = assembler.validate_reveal(reveal) {
            error!("Invalid reveal from {}: {}", participant, e);
            invalid_participants.push((participant.clone(), format!("Invalid reveal: {}", e)));
            continue;
        }

        // Verify UTXOs on-chain
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

            // Check if UTXO exists using the locked ElectrumClient
            let utxo_exists = {
                let electrum = electrum_client.lock().await;
                match electrum.script_get_history(&script) {
                    Ok(history) => history
                        .iter()
                        .any(|tx| tx.tx_hash.to_string() == input.outpoint.txid),
                    Err(e) => {
                        error!(
                            "Failed to verify UTXO for participant {}: {}",
                            participant, e
                        );
                        false
                    }
                }
            };

            if !utxo_exists {
                error!(
                    "UTXO not found for participant {}: {}:{}",
                    participant, input.outpoint.txid, input.outpoint.vout
                );
                invalid_participants.push((
                    participant.clone(),
                    format!(
                        "UTXO not found: {}:{}",
                        input.outpoint.txid, input.outpoint.vout
                    ),
                ));
            }
        }
    }

    // Remove invalid reveals
    for (participant, reason) in invalid_participants {
        reveals.remove(&participant);
        let reject = Reject {
            intent_id,
            participant_pkarr: participant.clone(),
            reason,
        };
        {
            let transport_lock = transport.lock().await;
            let _ = transport_lock
                .send_dm(&participant, &WireMsg::Reject(reject))
                .await;
        }
    }

    let (unsigned_tx, input_map) = assembler.build_transaction(&reveals)?;
    info!(
        "Built transaction with {} inputs, {} outputs",
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

    // Send template to participants
    for participant in reveals.keys() {
        let transport_lock = transport.lock().await;
        if let Err(e) = transport_lock
            .send_dm(participant, &WireMsg::Template(template.clone()))
            .await
        {
            error!("Failed to send template to {}: {}", participant, e);
        }
    }

    // Collect signatures
    info!(
        "Collecting signatures from participants for batch {}",
        intent_id
    );
    let mut fragments = HashMap::new();
    let sig_deadline =
        std::time::Instant::now() + Duration::from_millis(batch.intent.deadline_ms / 4);

    while fragments.len() < reveals.len() && std::time::Instant::now() < sig_deadline {
        let messages = {
            let transport_lock = transport.lock().await;
            transport_lock.receive_all().await?
        };

        for (sender, msg) in messages {
            if let WireMsg::SigFragment(frag) = msg {
                if frag.intent_id == intent_id && reveals.contains_key(&sender) {
                    debug!("Received signature fragment from {}", sender);
                    fragments.insert(sender, frag);
                }
            }
        }

        // Poll frequently during reveal collection to minimize latency
        sleep(Duration::from_millis(25)).await;
    }

    if fragments.is_empty() {
        error!("No signatures received for batch {}", intent_id);
        batch_manager
            .update_batch_status(intent_id, BatchStatus::Completed)
            .await?;
        return Err(BatchError::Other("No signatures received".to_string()));
    }

    info!(
        "Collected {} signatures for batch {}",
        fragments.len(),
        intent_id
    );

    // Merge PSBT fragments
    let mut final_psbt = psbt_template.clone();
    let mut psbt_fragments = Vec::new();

    for (participant, frag) in &fragments {
        match PsbtBuilder::from_base64(&frag.psbt_fragment_base64) {
            Ok(psbt) => {
                info!(
                    "Successfully deserialized PSBT fragment from {}",
                    participant
                );
                psbt_fragments.push(psbt);
            }
            Err(e) => {
                error!(
                    "Failed to deserialize PSBT fragment from {}: {}",
                    participant, e
                );
            }
        }
    }

    psbt_builder.merge_fragments(&mut final_psbt, psbt_fragments)?;

    // Finalize PSBT
    psbt_builder.finalize(&mut final_psbt)?;
    let final_tx = psbt_builder.extract_tx(&final_psbt)?;

    // Broadcast transaction
    let txid = final_tx.txid();
    info!("Broadcasting transaction {} for batch {}", txid, intent_id);

    // Broadcast using the locked ElectrumClient
    let broadcast_result = {
        let electrum = electrum_client.lock().await;
        electrum.transaction_broadcast(&final_tx)
    };

    match broadcast_result {
        Ok(broadcast_txid) => {
            info!("✅ Transaction {} broadcast successfully!", broadcast_txid);
            if broadcast_txid != txid {
                warn!(
                    "Broadcast TXID mismatch: expected {}, got {}",
                    txid, broadcast_txid
                );
            }

            let final_msg = FinalTx {
                intent_id,
                txid: txid.to_string(),
                raw_tx_hex: serialize_hex(&final_tx),
            };

            // Notify all participants
            for participant in reveals.keys() {
                let transport_lock = transport.lock().await;
                let _ = transport_lock
                    .send_dm(participant, &WireMsg::FinalTx(final_msg.clone()))
                    .await;
            }

            // Mark batch as completed
            batch_manager
                .update_batch_status(intent_id, BatchStatus::Completed)
                .await?;
            Ok(())
        }
        Err(e) => {
            error!(
                "Failed to broadcast transaction for batch {}: {}",
                intent_id, e
            );
            let raw_tx_hex = serialize_hex(&final_tx);
            info!("Raw TX (for manual broadcast): {}", raw_tx_hex);

            // Still mark as completed to avoid reprocessing
            batch_manager
                .update_batch_status(intent_id, BatchStatus::Completed)
                .await?;
            Err(BatchError::Other(format!("Broadcast failed: {}", e)))
        }
    }
}
