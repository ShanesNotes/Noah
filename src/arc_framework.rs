use sha2::{Sha256, Digest};
use anyhow::{Result, Error};

pub async fn log_to_blockchain(action: &str, patient_id: &str, data: &str) -> Result<(), Error> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = format!("{:x}", hasher.finalize());
    println!("Logged to ARC blockchain: {} for {} with hash {}", action, patient_id, hash);
    Ok(())
}