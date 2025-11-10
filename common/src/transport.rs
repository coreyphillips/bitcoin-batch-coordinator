use crate::messages::WireMsg;
use crate::{BatchError, Result};
use blake3;
use pkarr::PublicKey;
use pubky_messenger::PrivateMessengerClient;
use std::fs;
use tracing::{debug, error, warn};

/// Transport layer wrapper for pubky-messenger
pub struct Transport {
    messenger: PrivateMessengerClient,
    /// Track known peers to poll for messages (for coordinator)
    known_peers: std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
    /// Track processed message IDs to avoid duplicates
    processed_messages: std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
}

impl Transport {
    /// Create transport from recovery file
    pub async fn from_recovery_file(recovery_path: &str, passphrase: &str) -> Result<Self> {
        // Read recovery file
        let recovery_bytes = fs::read(recovery_path)
            .map_err(|e| BatchError::Transport(format!("Failed to read recovery file: {}", e)))?;

        // Create messenger client with optional passphrase
        let messenger =
            PrivateMessengerClient::from_recovery_file(&recovery_bytes, Some(passphrase))
                .map_err(|e| BatchError::Transport(format!("Failed to create messenger: {}", e)))?;

        // Sign in to establish session
        messenger
            .sign_in()
            .await
            .map_err(|e| BatchError::Transport(format!("Failed to sign in: {}", e)))?;

        Ok(Self {
            messenger,
            known_peers: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
            processed_messages: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
        })
    }

    /// Create transport from recovery phrase
    pub async fn from_recovery_phrase(mnemonic: &str, passphrase: Option<&str>) -> Result<Self> {
        // Create messenger client from recovery phrase
        let messenger = PrivateMessengerClient::from_recovery_phrase(
            mnemonic, passphrase, None, // Use default language (English)
        )
        .map_err(|e| {
            BatchError::Transport(format!("Failed to create messenger from phrase: {}", e))
        })?;

        // Sign in to establish session
        messenger
            .sign_in()
            .await
            .map_err(|e| BatchError::Transport(format!("Failed to sign in: {}", e)))?;

        Ok(Self {
            messenger,
            known_peers: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
            processed_messages: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
        })
    }

    /// Add a known peer to track (for coordinator receiving from participants)
    pub fn add_known_peer(&self, peer_pkarr: String) {
        if let Ok(mut peers) = self.known_peers.write() {
            peers.insert(peer_pkarr);
        }
    }

    /// Get known peers
    pub fn get_known_peers(&self) -> Vec<String> {
        if let Ok(peers_lock) = self.known_peers.read() {
            peers_lock.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Get the public key string for this transport
    pub fn public_key_string(&self) -> String {
        self.messenger.public_key_string()
    }

    /// Discover peers from Pubky follow graph
    /// Returns list of discovered peer public keys
    pub async fn discover_peers(&self) -> Result<Vec<String>> {
        // Get users who follow us
        let followers = self
            .messenger
            .get_followed_users()
            .await
            .map_err(|e| BatchError::Transport(format!("Failed to get followers: {}", e)))?;

        let mut discovered = Vec::new();
        for follower in followers {
            self.add_known_peer(follower.pubky.clone());
            discovered.push(follower.pubky);
        }

        Ok(discovered)
    }

    /// Seed follow list from provided pubkys and recursively follow their follows
    pub async fn seed_follows_from_list(&self, seed_pubkys: Vec<String>) -> Result<()> {
        use std::collections::HashSet;
        use tracing::info;

        if seed_pubkys.is_empty() {
            return Ok(());
        }

        info!(
            "Starting follow seeding with {} initial pubkys",
            seed_pubkys.len()
        );

        // Get our current follows to avoid duplicates
        let current_follows =
            self.messenger.get_followed_users().await.map_err(|e| {
                BatchError::Transport(format!("Failed to get current follows: {}", e))
            })?;

        let mut already_following: HashSet<String> =
            current_follows.iter().map(|f| f.pubky.clone()).collect();

        // Also consider ourselves as "already following" to avoid self-follow
        already_following.insert(self.messenger.public_key_string());

        let mut to_follow = Vec::new();

        // Process each seed pubky
        for seed_pubky in seed_pubkys {
            // First, follow the seed pubky itself if we're not already following
            if !already_following.contains(&seed_pubky) {
                info!("Following seed pubky: {}", seed_pubky);
                self.messenger.put_follow(&seed_pubky).await.map_err(|e| {
                    BatchError::Transport(format!("Failed to follow {}: {}", seed_pubky, e))
                })?;
                already_following.insert(seed_pubky.clone());
            } else {
                info!("Already following seed pubky: {}", seed_pubky);
            }

            // Get who this seed pubky follows
            info!("Getting follows for seed pubky: {}", seed_pubky);
            let seed_follows = self
                .messenger
                .get_followed_users_for(&seed_pubky)
                .await
                .map_err(|e| {
                    BatchError::Transport(format!(
                        "Failed to get follows for {}: {}",
                        seed_pubky, e
                    ))
                })?;

            info!("Seed {} follows {} users", seed_pubky, seed_follows.len());

            // Add their follows to our list if we're not already following them
            for followed in seed_follows {
                if !already_following.contains(&followed.pubky) {
                    to_follow.push(followed.pubky.clone());
                }
            }
        }

        // Now follow everyone we discovered
        info!("Following {} new users from seed list", to_follow.len());
        for pubky in to_follow {
            info!("Following discovered user: {}", pubky);
            self.messenger
                .put_follow(&pubky)
                .await
                .map_err(|e| BatchError::Transport(format!("Failed to follow {}: {}", pubky, e)))?;
            already_following.insert(pubky);
        }

        info!(
            "Follow seeding completed. Now following {} users total",
            already_following.len() - 1
        );
        Ok(())
    }

    /// Unfollow a specific pubky
    pub async fn unfollow(&self, pubky: &str) -> Result<()> {
        use tracing::info;

        info!("Unfollowing pubky: {}", pubky);
        self.messenger
            .delete_follow(pubky)
            .await
            .map_err(|e| BatchError::Transport(format!("Failed to unfollow {}: {}", pubky, e)))?;

        // Also remove from known peers
        if let Ok(mut peers) = self.known_peers.write() {
            peers.remove(pubky);
        }

        info!("Successfully unfollowed: {}", pubky);
        Ok(())
    }

    /// Send a direct message to a peer
    pub async fn send_dm(&self, peer_pkarr: &str, msg: &WireMsg) -> Result<()> {
        let payload = serde_json::to_string(msg).map_err(|e| BatchError::Serialization(e))?;

        let peer_pubkey = PublicKey::try_from(peer_pkarr)
            .map_err(|e| BatchError::Transport(format!("Invalid pkarr: {}", e)))?;

        debug!("Sending message to {}: {:?}", peer_pkarr, msg);

        self.messenger
            .send_message(&peer_pubkey, &payload)
            .await
            .map_err(|e| BatchError::Transport(format!("Failed to send DM: {}", e)))?;

        Ok(())
    }

    /// Receive messages from a specific peer
    pub async fn receive_from(&self, peer_pkarr: &str) -> Result<Vec<WireMsg>> {
        let peer_pubkey = PublicKey::try_from(peer_pkarr)
            .map_err(|e| BatchError::Transport(format!("Invalid pkarr: {}", e)))?;

        debug!("Fetching messages from peer: {}", peer_pkarr);
        let messages = self
            .messenger
            .get_messages(&peer_pubkey)
            .await
            .map_err(|e| BatchError::Transport(format!("Failed to receive messages: {}", e)))?;

        debug!("Fetched {} messages from {}", messages.len(), peer_pkarr);

        let mut parsed = Vec::new();
        for decrypted_msg in messages {
            // Generate a unique message ID from sender + timestamp + content hash
            let message_id = format!(
                "{}-{}-{}",
                peer_pkarr,
                decrypted_msg.timestamp,
                blake3::hash(decrypted_msg.content.as_bytes()).to_hex()
            );

            // Check if we've already processed this message
            let is_duplicate = {
                if let Ok(processed) = self.processed_messages.read() {
                    processed.contains(&message_id)
                } else {
                    false
                }
            };

            if is_duplicate {
                debug!(
                    "Skipping duplicate message from {} at timestamp {}",
                    peer_pkarr, decrypted_msg.timestamp
                );
                continue;
            }

            match serde_json::from_str::<WireMsg>(&decrypted_msg.content) {
                Ok(wire_msg) => {
                    debug!(
                        "Parsed new message from {} at timestamp {}: {:?}",
                        peer_pkarr, decrypted_msg.timestamp, wire_msg
                    );

                    // Mark this message as processed
                    if let Ok(mut processed) = self.processed_messages.write() {
                        processed.insert(message_id);
                    }

                    // Auto-track this peer for future receive_all() calls
                    self.add_known_peer(peer_pkarr.to_string());

                    parsed.push(wire_msg);
                }
                Err(e) => {
                    warn!("Failed to parse message from {}: {}", peer_pkarr, e);
                    warn!("Message content: {}", decrypted_msg.content);
                }
            }
        }

        Ok(parsed)
    }

    /// Receive messages from all known peers (for coordinator)
    ///
    /// This polls all known peers and returns messages. Peers are automatically
    /// tracked when they send messages. For the initial discovery phase, the
    /// coordinator needs to know participant public keys (e.g., from a bulletin board).
    pub async fn receive_all(&self) -> Result<Vec<(String, WireMsg)>> {
        let mut all_parsed = Vec::new();

        // Get snapshot of known peers
        let peers: Vec<String> = {
            if let Ok(peers_lock) = self.known_peers.read() {
                peers_lock.iter().cloned().collect()
            } else {
                Vec::new()
            }
        };

        // Poll each known peer
        for peer_pkarr in peers {
            match self.receive_from(&peer_pkarr).await {
                Ok(messages) => {
                    for msg in messages {
                        all_parsed.push((peer_pkarr.clone(), msg));
                    }
                }
                Err(e) => {
                    debug!("Failed to receive from {}: {}", peer_pkarr, e);
                }
            }
        }

        Ok(all_parsed)
    }

    /// Broadcast a message to multiple peers
    pub async fn broadcast(&self, peers: &[String], msg: &WireMsg) -> Result<()> {
        for peer in peers {
            if let Err(e) = self.send_dm(peer, msg).await {
                error!("Failed to send to {}: {}", peer, e);
                // Continue broadcasting to others
            }
        }
        Ok(())
    }

    /// Clear all messages with a specific peer (deletes all sent messages in the conversation)
    pub async fn clear_messages_with_peer(&self, peer_pkarr: &str) -> Result<()> {
        let peer_pubkey = PublicKey::try_from(peer_pkarr)
            .map_err(|e| BatchError::Transport(format!("Invalid pkarr: {}", e)))?;

        debug!("Clearing all messages with peer: {}", peer_pkarr);

        self.messenger
            .clear_messages(&peer_pubkey)
            .await
            .map_err(|e| BatchError::Transport(format!("Failed to clear messages: {}", e)))?;

        // Also clear processed message IDs for this peer to free memory
        if let Ok(mut processed) = self.processed_messages.write() {
            processed.retain(|id| !id.starts_with(&format!("{}-", peer_pkarr)));
        }

        Ok(())
    }

    /// Delete specific messages with a peer
    pub async fn delete_messages_with_peer(
        &self,
        peer_pkarr: &str,
        message_ids: Vec<String>,
    ) -> Result<()> {
        let peer_pubkey = PublicKey::try_from(peer_pkarr)
            .map_err(|e| BatchError::Transport(format!("Invalid pkarr: {}", e)))?;

        debug!(
            "Deleting {} messages with peer: {}",
            message_ids.len(),
            peer_pkarr
        );

        self.messenger
            .delete_messages(message_ids, &peer_pubkey)
            .await
            .map_err(|e| BatchError::Transport(format!("Failed to delete messages: {}", e)))?;

        Ok(())
    }

    /// Delete a single message with a peer
    pub async fn delete_message_with_peer(&self, peer_pkarr: &str, message_id: &str) -> Result<()> {
        let peer_pubkey = PublicKey::try_from(peer_pkarr)
            .map_err(|e| BatchError::Transport(format!("Invalid pkarr: {}", e)))?;

        debug!("Deleting message {} with peer: {}", message_id, peer_pkarr);

        self.messenger
            .delete_message(message_id, &peer_pubkey)
            .await
            .map_err(|e| BatchError::Transport(format!("Failed to delete message: {}", e)))?;

        Ok(())
    }

    /// Clear messages with all known peers (useful for cleanup on startup or completion)
    pub async fn clear_all_messages(&self) -> Result<()> {
        let peers = self.get_known_peers();
        debug!("Clearing messages with all {} known peers", peers.len());

        for peer_pkarr in peers {
            if let Err(e) = self.clear_messages_with_peer(&peer_pkarr).await {
                warn!("Failed to clear messages with peer {}: {}", peer_pkarr, e);
                // Continue with other peers
            }
        }

        // Clear all processed message IDs to free memory
        if let Ok(mut processed) = self.processed_messages.write() {
            processed.clear();
            debug!("Cleared all processed message IDs");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_wire_msg_serialization() {
        use crate::messages::*;
        use uuid::Uuid;

        let intent = Intent {
            intent_id: Uuid::new_v4(),
            network: NetworkSpec::Signet,
            fee_rate_sat_vb: 10,
            min_participants: 2,
            max_participants: 10,
            deadline_ms: 60000,
            allow_change: true,
            coordinator_pkarr: "test_pkarr".to_string(),
            fee_model: FeeModel::WeightBased,
        };

        let msg = WireMsg::Intent(intent);
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: WireMsg = serde_json::from_str(&json).unwrap();

        match parsed {
            WireMsg::Intent(i) => assert_eq!(i.fee_rate_sat_vb, 10),
            _ => panic!("Wrong message type"),
        }
    }
}
