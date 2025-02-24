use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio;
use serde::{Serialize, Deserialize};
use clap::{Parser, Subcommand};
use rig_core::{agent::AgentBuilder, completion::{CompletionModel, CompletionRequest, CompletionResponse, Message, CompletionError}};
use reqwest::Client;
use serde_json::json;
use anyhow::{bail, Result, Error};
use dotenv::dotenv;

mod arc_framework {
    use sha2::{Sha256, Digest};
    use anyhow::{Result, Error};

    pub async fn log_to_blockchain(action: &str, patient_id: &str, data: &str) -> Result<String, Error> {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());
        Ok(format!("Logged to ARC blockchain: {} for {} with hash {}", action, patient_id, hash))
    }
}

mod external_apis {
    use std::collections::HashMap;
    use rand::Rng;
    use anyhow::{Result, Error};

    pub async fn fetch_vitals(patient_id: &str) -> Result<HashMap<String, f32>, Error> {
        println!("SIMULATION: Fetching vitals for {}. Requires Philips integration.", patient_id);
        let mut rng = rand::thread_rng();
        let mut vitals = HashMap::from([
            ("HR".to_string(), rng.gen_range(60.0..100.0)),
            ("SpO2".to_string(), rng.gen_range(90.0..100.0)),
            ("RR".to_string(), rng.gen_range(12.0..20.0)),
            ("NIBP".to_string(), rng.gen_range(90.0..140.0)),
            ("ABP".to_string(), rng.gen_range(70.0..90.0)),
            ("CVP".to_string(), rng.gen_range(2.0..8.0)),
        ]);
        if patient_id == "P003" {
            vitals.insert("PAP".to_string(), rng.gen_range(15.0..30.0));
        }
        Ok(vitals)
    }

    pub async fn fetch_labs(patient_id: &str) -> Result<HashMap<String, f32>, Error> {
        println!("SIMULATION: Fetching labs for {}. Requires Epic integration.", patient_id);
        Ok(HashMap::from([
            ("creatinine".to_string(), 1.2),
            ("glucose".to_string(), 95.0),
        ]))
    }

    pub async fn log_to_ehr(patient_id: &str, action: &str, data: &str) -> Result<String, Error> {
        println!("SIMULATION: Logging '{}' for {}. Requires Epic API.", action, patient_id);
        Ok(format!("Logged to EHR: {} - {}", action, data))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatientChart {
    patient_id: String,
    vitals: HashMap<String, f32>,
    labs: HashMap<String, f32>,
    meds: Vec<Medication>,
    tasks: Vec<PatientTask>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Medication {
    name: String,
    dose: f32,
    route: String,
    time_due: SystemTime,
    parameters: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PatientTask {
    id: u32,
    description: String,
    severity: u8,
    due_time: SystemTime,
}

#[derive(Parser)]
#[command(name = "Noah", about = "Critical Care Nurse AI Agent Toolkit")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    MonitorVitals { patient_id: String },
    ReviewMeds { patient_id: String },
    ChartAssessment { patient_id: String, assessment: String },
    ChartVitals { patient_id: String },
    ChartIO { patient_id: String, intake: f32, output: f32 },
    MonitorLabs { patient_id: String },
    AddTask { patient_id: String, task: String, severity: u8, due_hours: u32 },
    LogNote { patient_id: String, note: String },
}

// Custom Gemini model using Google's Gemini API
#[derive(Clone)]
struct GeminiModel {
    client: Client,
    api_key: String,
    model: String,
}

impl GeminiModel {
    fn new(api_key: String, model: String) -> Self {
        GeminiModel {
            client: Client::new(),
            api_key,
            model,
        }
    }
}

#[async_trait::async_trait]
impl CompletionModel for GeminiModel {
    type Response = CompletionResponse<Message>;

    fn completion(&self, request: CompletionRequest) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, CompletionError>> + Send>> {
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let prompt = request.prompt;

        Box::pin(async move {
            let response = client
                .post(format!("https://generativelanguage.googleapis.com/v1/models/{}:generateContent", model))
                .header("x-goog-api-key", &api_key)
                .json(&json!({
                    "contents": [
                        {
                            "parts": [
                                {"text": prompt}
                            ]
                        }
                    ],
                    "generationConfig": {
                        "temperature": 0.7,
                        "maxOutputTokens": 150
                    }
                }))
                .send()
                .await
                .map_err(|e| CompletionError::Request(e.to_string()))?;

            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| CompletionError::Parse(e.to_string()))?;

            let content = json["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .ok_or_else(|| CompletionError::Parse("No text in Gemini response".to_string()))?
                .to_string();

            Ok(CompletionResponse {
                choice: Message {
                    role: "assistant".to_string(),
                    content,
                },
                raw_response: prompt,
            })
        })
    }
}

struct NoahAgent<M: CompletionModel> {
    chart: PatientChart,
    rig_agent: rig_core::agent::Agent<M>,
}

impl<M: CompletionModel> NoahAgent<M> {
    fn new(chart: PatientChart, training_data: Vec<String>, model: M) -> Self {
        let rig_agent = AgentBuilder::new(model)
            .preamble("You are Noah, a critical care nurse AI assisting RNs with empathy and precision.")
            .context(&training_data.join("\n"))
            .build();
        NoahAgent { chart, rig_agent }
    }

    async fn monitor_vitals(&mut self) -> Result<String, Error> {
        let vitals = external_apis::fetch_vitals(&self.chart.patient_id).await?;
        println!("DEBUG: Fetched vitals: {:?}", vitals);
        self.chart.vitals = vitals.clone();
        let mut response = String::new();
        for (key, value) in &vitals {
            let norm = match key.as_str() {
                "HR" => (60.0, 100.0),
                "SpO2" => (90.0, 100.0),
                "RR" => (12.0, 20.0),
                "NIBP" => (90.0, 140.0),
                "ABP" => (70.0, 90.0),
                "CVP" => (2.0, 8.0),
                "PAP" => (15.0, 30.0),
                _ => (0.0, 0.0),
            };
            let status = if *value < norm.0 || *value > norm.1 { "abnormal" } else { "normal" };
            response.push_str(&format!("{}: {} ({}), ", key, value, status));
        }
        self.log_to_ehr("Vitals Monitored", &response).await?;
        Ok(format!("[SIMULATED DATA] {}", response.trim_end_matches(", ")))
    }

    async fn review_meds(&mut self) -> Result<String, Error> {
        let vitals = external_apis::fetch_vitals(&self.chart.patient_id).await?;
        self.chart.vitals = vitals.clone();
        let mut response = String::new();
        for med in &self.chart.meds {
            let hr = vitals.get("HR").unwrap_or(&0.0);
            let recommendation = if med.parameters.contains("Hold if HR < 60") && *hr < 60.0 {
                "Hold recommended"
            } else {
                "Administer as ordered"
            };
            response.push_str(&format!("{}: {} ({}), ", med.name, med.parameters, recommendation));
        }
        self.log_to_ehr("Medication Review", &response).await?;
        Ok(format!("[SIMULATED DATA] {}", response.trim_end_matches(", ")))
    }

    async fn chart_assessment(&mut self, assessment: &str) -> Result<String, Error> {
        let vitals = external_apis::fetch_vitals(&self.chart.patient_id).await?;
        self.chart.vitals = vitals.clone();
        let context = serde_json::to_string(&self.chart)?;
        let prompt = format!(
            "You are Noah, a critical care nurse AI. Based on this patient data: {}, refine this assessment into a concise, professional note: {}",
            context, assessment
        );
        let llm_response = self.rig_agent.completion(prompt.into()).await?;
        let refined_assessment = llm_response.choice.content;
        let response = format!("Assessment: {}", refined_assessment);
        self.log_to_ehr("Chart Assessment", &response).await?;
        self.log_to_arc("Chart Assessment", &response).await?;
        Ok(format!("[LLM-ENHANCED] {}", response))
    }

    async fn chart_vitals(&mut self) -> Result<String, Error> {
        let vitals = external_apis::fetch_vitals(&self.chart.patient_id).await?;
        self.chart.vitals = vitals.clone();
        let response = vitals.iter().map(|(k, v)| format!("{}: {}", k, v)).collect::<Vec<String>>().join(", ");
        self.log_to_ehr("Chart Vitals", &response).await?;
        self.log_to_arc("Chart Vitals", &response).await?;
        Ok(format!("[SIMULATED DATA] {}", response))
    }

    async fn chart_io(&mut self, intake: f32, output: f32) -> Result<String, Error> {
        let response = format!("Intake: {} mL, Output: {} mL", intake, output);
        self.log_to_ehr("Chart I&O", &response).await?;
        self.log_to_arc("Chart I&O", &response).await?;
        Ok(format!("[SIMULATED DATA] {}", response))
    }

    async fn monitor_labs(&mut self) -> Result<String, Error> {
        let labs = external_apis::fetch_labs(&self.chart.patient_id).await?;
        self.chart.labs = labs.clone();
        let response = labs.iter().map(|(k, v)| format!("{}: {}", k, v)).collect::<Vec<String>>().join(", ");
        self.log_to_ehr("Monitor Labs", &response).await?;
        Ok(format!("[SIMULATED DATA] {}", response))
    }

    async fn add_task(&mut self, task: String, severity: u8, due_hours: u32) -> Result<String, Error> {
        let task = PatientTask {
            id: self.chart.tasks.len() as u32 + 1,
            description: task.clone(),
            severity,
            due_time: SystemTime::now() + Duration::from_secs(due_hours as u64 * 3600),
        };
        self.chart.tasks.push(task.clone());
        let response = format!("Added task: {} (Severity: {})", task.description, task.severity);
        self.log_to_ehr("Add Task", &response).await?;
        Ok(format!("[SIMULATED DATA] {}", response))
    }

    async fn log_provider_note(&mut self, note: &str) -> Result<String, Error> {
        self.chart.notes.push(note.to_string());
        let response = format!("Provider Note: {}", note);
        self.log_to_ehr("Log Provider Note", &response).await?;
        self.log_to_arc("Log Provider Note", &response).await?;
        Ok(format!("[SIMULATED DATA] {}", response))
    }

    async fn log_to_ehr(&self, action: &str, data: &str) -> Result<String, Error> {
        external_apis::log_to_ehr(&self.chart.patient_id, action, data).await
    }

    async fn log_to_arc(&self, action: &str, data: &str) -> Result<String, Error> {
        arc_framework::log_to_blockchain(action, &self.chart.patient_id, data).await
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv().ok(); // Load .env file if present
    let cli = Cli::parse();
    let chart = load_patient_chart(&cli.command).await?;

    // Load Gemini API key from environment variable
    let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not set");
    let gemini_model = GeminiModel::new(api_key, "gemini-1.5-pro".to_string()); // Use Gemini 1.5 Pro

    let training_data = vec!["Default training data for Noah".to_string()];
    let mut agent = NoahAgent::new(chart, training_data, gemini_model);

    match cli.command {
        Commands::MonitorVitals { patient_id } => {
            agent.chart.patient_id = patient_id;
            println!("Vitals: {}", agent.monitor_vitals().await?);
        }
        Commands::ReviewMeds { patient_id } => {
            agent.chart.patient_id = patient_id;
            println!("Meds: {}", agent.review_meds().await?);
        }
        Commands::ChartAssessment { patient_id, assessment } => {
            agent.chart.patient_id = patient_id;
            println!("Assessment: {}", agent.chart_assessment(&assessment).await?);
        }
        Commands::ChartVitals { patient_id } => {
            agent.chart.patient_id = patient_id;
            println!("Vitals Charted: {}", agent.chart_vitals().await?);
        }
        Commands::ChartIO { patient_id, intake, output } => {
            agent.chart.patient_id = patient_id;
            println!("I&O: {}", agent.chart_io(intake, output).await?);
        }
        Commands::MonitorLabs { patient_id } => {
            agent.chart.patient_id = patient_id;
            println!("Labs: {}", agent.monitor_labs().await?);
        }
        Commands::AddTask { patient_id, task, severity, due_hours } => {
            agent.chart.patient_id = patient_id;
            println!("Task: {}", agent.add_task(task, severity, due_hours).await?);
        }
        Commands::LogNote { patient_id, note } => {
            agent.chart.patient_id = patient_id;
            println!("Note: {}", agent.log_provider_note(note).await?);
        }
    }
    Ok(())
}

async fn load_patient_chart(command: &Commands) -> Result<PatientChart, Error> {
    let patient_id = match command {
        Commands::MonitorVitals { patient_id } => patient_id.clone(),
        Commands::ReviewMeds { patient_id } => patient_id.clone(),
        Commands::ChartAssessment { patient_id, .. } => patient_id.clone(),
        Commands::ChartVitals { patient_id } => patient_id.clone(),
        Commands::ChartIO { patient_id, .. } => patient_id.clone(),
        Commands::MonitorLabs { patient_id } => patient_id.clone(),
        Commands::AddTask { patient_id, .. } => patient_id.clone(),
        Commands::LogNote { patient_id, .. } => patient_id.clone(),
    };
    Ok(PatientChart {
        patient_id,
        vitals: HashMap::new(),
        labs: HashMap::new(),
        meds: vec![Medication {
            name: "Aspirin".to_string(),
            dose: 81.0,
            route: "Oral".to_string(),
            time_due: SystemTime::now() + Duration::from_secs(3600),
            parameters: "Hold if HR < 60".to_string(),
        }],
        tasks: Vec::new(),
        notes: Vec::new(),
    })
}
