use std::collections::HashMap;
use crate::models::patient::{PatientData, Medication, Order, OrderStatus, InputOutput};

pub fn process_vitals_data(raw_data: &str) -> Result<HashMap<String, f64>, serde_json::Error> {
    let data: serde_json::Value = serde_json::from_str(raw_data)?;
    let mut vitals = HashMap::new();
    
    if let Some(obj) = data.get("vitals").and_then(|v| v.as_object()) {
        for (key, value) in obj {
            if let Some(num) = value.as_f64() {
                vitals.insert(key.clone(), num);
            }
        }
    }
    
    Ok(vitals)
}

pub fn analyze_vitals(vitals: &HashMap<String, f64>) -> Vec<String> {
    let mut alerts = Vec::new();
    
    // Check for critical values
    if let Some(&hr) = vitals.get("HR") {
        if hr > 120.0 {
            alerts.push(format!("High heart rate: {} bpm", hr));
        } else if hr < 50.0 {
            alerts.push(format!("Low heart rate: {} bpm", hr));
        }
    }
    
    if let Some(&map) = vitals.get("MAP") {
        if map < 65.0 {
            alerts.push(format!("Low MAP: {} mmHg", map));
        }
    }
    
    if let Some(&spo2) = vitals.get("SpO2") {
        if spo2 < 92.0 {
            alerts.push(format!("Low oxygen saturation: {}%", spo2));
        }
    }
    
    alerts
} 