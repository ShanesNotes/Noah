use std::collections::HashMap;
use reqwest::Client;
use serde_json::json;
use anyhow::{Result, Error};

pub struct AiService {
    arc_client: Client,
    rig_client: Client,
    arc_api_key: String,
    rig_api_key: String,
}

impl AiService {
    pub fn new(arc_api_key: String, rig_api_key: String) -> Self {
        Self {
            arc_client: Client::new(),
            rig_client: Client::new(),
            arc_api_key,
            rig_api_key,
        }
    }
    
    pub async fn analyze_vitals(&self, vitals: &HashMap<String, f64>) -> Result<String> {
        // Implementation for AI analysis using Arc
        let response = self.arc_client
            .post("https://api.arc.io/v1/analyze")
            .header("Authorization", format!("Bearer {}", self.arc_api_key))
            .json(&json!({
                "vitals": vitals,
                "analysis_type": "clinical"
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
            
        Ok(response["analysis"].as_str().unwrap_or("No analysis available").to_string())
    }
    
    pub async fn get_clinical_recommendation(&self, patient_data: &crate::models::patient::PatientData) -> Result<String> {
        // Implementation for AI recommendations using Rig
        let response = self.rig_client
            .post("https://api.rig.dev/v1/recommend")
            .header("Authorization", format!("Bearer {}", self.rig_api_key))
            .json(&json!({
                "patient_data": patient_data,
                "model": "claude-3-sonnet"
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
            
        Ok(response["recommendation"].as_str().unwrap_or("No recommendation available").to_string())
    }
} 