use sha2::{Sha256, Digest};
use anyhow::{Result, Error};
use tracing::{info, debug, error};
use tokio::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlockchainEntry {
    pub timestamp: u64,
    pub action: String,
    pub patient_id: String,
    pub data: String,
    pub hash: String,
    pub previous_hash: Option<String>,
}

// Thread-safe global storage for the last hash
lazy_static::lazy_static! {
    static ref LAST_HASH: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
}

/// Logs an action to the blockchain with patient ID and data
/// 
/// Returns the hash of the entry that was created
pub async fn log_to_blockchain(action: &str, patient_id: &str, data: &str) -> Result<String, Error> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs();
    
    // Create hash of the current data
    let mut hasher = Sha256::new();
    hasher.update(format!("{}:{}:{}:{}", timestamp, action, patient_id, data));
    
    // Get previous hash and include it in the new hash calculation
    let previous_hash = {
        let mut last_hash = LAST_HASH.lock().unwrap();
        let prev = last_hash.clone();
        if let Some(ref prev_hash) = prev {
            hasher.update(prev_hash);
        }
        prev
    };
    
    let hash = format!("{:x}", hasher.finalize());
    
    // Update the last hash
    {
        let mut last_hash = LAST_HASH.lock().unwrap();
        *last_hash = Some(hash.clone());
    }
    
    // Create blockchain entry
    let entry = BlockchainEntry {
        timestamp,
        action: action.to_string(),
        patient_id: patient_id.to_string(),
        data: data.to_string(),
        hash: hash.clone(),
        previous_hash,
    };
    
    // Log the entry
    info!("Logged to ARC blockchain: {} for {} with hash {}", action, patient_id, hash);
    debug!("Blockchain entry: {:?}", entry);
    
    // In a real implementation, you would persist this entry to storage
    // store_entry(&entry).await?;
    
    Ok(hash)
}

/// Logs multiple events to the blockchain in parallel
/// 
/// Returns a vector of hashes for each entry created
pub async fn batch_log_to_blockchain(events: Vec<(&str, &str, &str)>) -> Result<Vec<String>, Error> {
    let (tx, mut rx) = mpsc::channel(events.len());
    
    for (action, patient_id, data) in events {
        let tx = tx.clone();
        tokio::spawn(async move {
            match log_to_blockchain(action, patient_id, data).await {
                Ok(hash) => {
                    if let Err(e) = tx.send(Ok(hash)).await {
                        error!("Failed to send batch log result: {}", e);
                    }
                },
                Err(e) => {
                    if let Err(send_err) = tx.send(Err(e)).await {
                        error!("Failed to send batch log error: {}", send_err);
                    }
                }
            }
        });
    }
    
    // Drop the original sender to allow the channel to close when all spawned tasks are done
    drop(tx);
    
    let mut hashes = Vec::new();
    while let Some(result) = rx.recv().await {
        hashes.push(result?);
    }
    
    Ok(hashes)
}

/// Queries the blockchain for entries related to a patient
/// 
/// The action_filter can be "all" to get all actions or a specific action type
pub async fn query_blockchain(patient_id: &str, action_filter: &str) -> Result<Vec<String>, Error> {
    // In a real implementation, you would query your blockchain storage
    // For now, we'll return a simulated result
    if action_filter == "all" {
        Ok(vec![
            format!("Vitals update at {}", SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() - 3600),
            format!("Medication administered at {}", SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() - 1800),
            format!("Lab results at {}", SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() - 900),
        ])
    } else {
        Ok(vec![format!("{} events for {} at {}", 
            action_filter, 
            patient_id, 
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        )])
    }
}

/// Verifies the integrity of the blockchain
/// 
/// Returns true if the blockchain is valid, false otherwise
pub async fn verify_blockchain_integrity() -> Result<bool, Error> {
    // In a real implementation, you would verify the entire chain
    // by checking that each entry's hash matches its contents and
    // that the previous_hash references are correct
    Ok(true)
}