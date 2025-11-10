use bitcoin::{Network, OutPoint, TxOut};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Wire protocol message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WireMsg {
    Intent(Intent),
    Commitment(Commitment),
    RequestReveal(RequestReveal),
    Reveal(Reveal),
    Ack(Ack),
    Template(Template),
    SigFragment(SigFragment),
    FinalTx(FinalTx),
    Reject(Reject),
    GetCurrentIntent(GetCurrentIntent),
    CurrentIntent(CurrentIntent),
}

/// Fee calculation model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeeModel {
    /// Fees are proportional to transaction weight contribution (inputs and outputs)
    WeightBased,
}

/// Intent broadcast by coordinator to start batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub intent_id: Uuid,
    pub network: NetworkSpec,
    pub fee_rate_sat_vb: u64,
    pub min_participants: usize,
    pub max_participants: usize,
    pub deadline_ms: u64,
    pub allow_change: bool,
    pub coordinator_pkarr: String,
    pub fee_model: FeeModel, // How fees are calculated
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkSpec {
    Bitcoin,
    Testnet,
    Signet,
    Regtest,
}

impl NetworkSpec {
    pub fn to_bdk_network(&self) -> Network {
        match self {
            NetworkSpec::Bitcoin => Network::Bitcoin,
            NetworkSpec::Testnet => Network::Testnet,
            NetworkSpec::Signet => Network::Signet,
            NetworkSpec::Regtest => Network::Regtest,
        }
    }

    pub fn from_bdk_network(network: Network) -> Self {
        match network {
            Network::Bitcoin => NetworkSpec::Bitcoin,
            Network::Testnet => NetworkSpec::Testnet,
            Network::Signet => NetworkSpec::Signet,
            Network::Regtest => NetworkSpec::Regtest,
            _ => NetworkSpec::Regtest,
        }
    }
}

/// Commitment: hash of reveal proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commitment {
    pub intent_id: Uuid,
    pub participant_pkarr: String,
    pub commitment_hash: String, // hex-encoded BLAKE3 hash
}

/// RequestReveal: coordinator signals participants to send reveals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestReveal {
    pub intent_id: Uuid,
    pub committed_participants: Vec<String>, // List of participants who committed
}

/// Reveal: full proposal with inputs and outputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reveal {
    pub intent_id: Uuid,
    pub participant_pkarr: String,
    pub inputs: Vec<InputProposal>,
    pub payments: Vec<PaymentOutput>,
    pub change_address: Option<String>, // if allow_change
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputProposal {
    pub outpoint: OutPointSerde,
    pub witness_utxo: TxOutSerde,
    pub descriptor: String, // Must be P2WPKH for v1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentOutput {
    pub address: String,
    pub amount_sats: u64,
}

/// Serializable OutPoint
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct OutPointSerde {
    pub txid: String, // hex
    pub vout: u32,
}

impl OutPointSerde {
    pub fn from_outpoint(op: &OutPoint) -> Self {
        Self {
            txid: op.txid.to_string(),
            vout: op.vout,
        }
    }

    pub fn to_outpoint(&self) -> Result<OutPoint, Box<dyn std::error::Error>> {
        use std::str::FromStr;
        let txid = bitcoin::Txid::from_str(&self.txid)?;
        Ok(OutPoint {
            txid,
            vout: self.vout,
        })
    }
}

/// Serializable TxOut
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxOutSerde {
    pub value_sats: u64,
    pub script_pubkey: String, // hex
}

impl TxOutSerde {
    pub fn from_txout(txout: &TxOut) -> Self {
        Self {
            value_sats: txout.value,
            script_pubkey: txout.script_pubkey.to_hex_string(),
        }
    }

    pub fn to_txout(&self) -> Result<TxOut, Box<dyn std::error::Error>> {
        use bitcoin::ScriptBuf;

        let script = ScriptBuf::from_hex(&self.script_pubkey)?;
        Ok(TxOut {
            value: self.value_sats,
            script_pubkey: script,
        })
    }
}

/// Ack from coordinator after receiving commitment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ack {
    pub intent_id: Uuid,
    pub participant_pkarr: String,
    pub message: String,
}

/// Template: unsigned tx + input ownership map
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub intent_id: Uuid,
    pub unsigned_tx_hex: String,
    pub psbt_base64: String, // PSBT v0 with all inputs/outputs, no sigs
    pub input_map: HashMap<String, String>, // input_index (as string) -> participant_pkarr
}

/// SigFragment: partial signatures from participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigFragment {
    pub intent_id: Uuid,
    pub participant_pkarr: String,
    pub psbt_fragment_base64: String, // PSBT with only this participant's signatures
}

/// FinalTx: broadcast transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalTx {
    pub intent_id: Uuid,
    pub txid: String,
    pub raw_tx_hex: String,
}

/// Reject: coordinator rejects participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reject {
    pub intent_id: Uuid,
    pub participant_pkarr: String,
    pub reason: String,
}

/// GetCurrentIntent: participant requests current batch intent from coordinator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetCurrentIntent {
    pub participant_pkarr: String,
}

/// CurrentIntent: coordinator response with current/next available batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentIntent {
    pub intent: Intent,
    pub status: BatchStatus,
    pub current_participants: usize,
}

/// BatchStatus: current state of a batch
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BatchStatus {
    Filling,    // Still accepting participants
    Ready,      // Has min participants, waiting for deadline
    Processing, // Building transaction
    Completed,  // Transaction broadcast
    Full,       // Max participants reached
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_outpoint_serde_roundtrip() {
        use std::str::FromStr;
        let txid = bitcoin::Txid::from_str(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
        )
        .unwrap();
        let op = OutPoint { txid, vout: 42 };
        let serde_op = OutPointSerde::from_outpoint(&op);
        let back = serde_op.to_outpoint().unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn test_txout_serde_roundtrip() {
        use bitcoin::ScriptBuf;
        let txout = TxOut {
            value: 100000,
            script_pubkey: ScriptBuf::new(),
        };
        let serde_txout = TxOutSerde::from_txout(&txout);
        let back = serde_txout.to_txout().unwrap();
        assert_eq!(txout, back);
    }
}
