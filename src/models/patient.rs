use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatientData {
    pub vitals: HashMap<String, f64>,
    pub labs: HashMap<String, f64>,
    pub medications: Vec<Medication>,
    pub orders: Vec<Order>,
    pub io: InputOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Medication {
    pub name: String,
    pub dose: String,
    pub route: String,
    pub frequency: String,
    pub last_administered: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub description: String,
    pub status: OrderStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderStatus {
    Pending,
    Active,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputOutput {
    pub intake: HashMap<String, f64>,
    pub output: HashMap<String, f64>,
} 