use async_trait::async_trait;
use anyhow::{Result, Error};

#[async_trait]
pub trait Tool: Send + Sync {
    async fn execute(&self, input: &str) -> Result<String, Error>;
}

pub struct VitalsChecker;
#[async_trait]
impl Tool for VitalsChecker {
    async fn execute(&self, _input: &str) -> Result<String, Error> {
        Ok("HR: 72, SpO2: 98".to_string())
    }
}

pub struct ChartAssessment;
#[async_trait]
impl Tool for ChartAssessment {
    async fn execute(&self, input: &str) -> Result<String, Error> {
        Ok(format!("Assessment for {}: Stable.", input))
    }
}

pub struct RecordIO;
#[async_trait]
impl Tool for RecordIO {
    async fn execute(&self, input: &str) -> Result<String, Error> {
        Ok(format!("I/O for {}: Intake 500ml, Output 400ml.", input))
    }
}

pub struct CheckLabValues;
#[async_trait]
impl Tool for CheckLabValues {
    async fn execute(&self, input: &str) -> Result<String, Error> {
        Ok(format!("Labs for {}: Glucose 95.", input))
    }
}

pub struct AdministerMedication;
#[async_trait]
impl Tool for AdministerMedication {
    async fn execute(&self, input: &str) -> Result<String, Error> {
        Ok(format!("Administered to {}: Aspirin 81mg (Pharmacy).", input))
    }
}

pub struct AnalyzeTestResults;
#[async_trait]
impl Tool for AnalyzeTestResults {
    async fn execute(&self, input: &str) -> Result<String, Error> {
        Ok(format!("Results for {}: Normal.", input))
    }
}