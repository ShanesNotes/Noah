use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Mutex};
use sqlx::SqlitePool;
use clap::{Parser, Subcommand};
use anyhow::{Result, Error};
use rig_core::agent::{Agent, AgentBuilder};
use serde::{Serialize, Deserialize};
use async_trait::async_trait;

trait CompletionModel: Send + Sync {}
#[derive(Clone)]
struct GeminiModel;
impl CompletionModel for GeminiModel {}

mod external_apis {
    pub async fn text_to_speech(text: &str, language: &str) -> String {
        format!("[Voice in {}] {}", language, text)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PatientChart {
    patient_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PatientTask {
    id: u32,
    patient_id: String,
    description: String,
    severity: u8,
    time_due: SystemTime,
}

#[derive(Clone)]
struct UserPreferences {
    language: String,
    voice_enabled: bool,
}

enum AgentMode {
    Nurse,
    Patient,
}

#[async_trait]
trait Tool: Send + Sync {
    async fn execute(&self, input: &str) -> Result<String, Error>;
}

struct VitalsChecker;
#[async_trait]
impl Tool for VitalsChecker {
    async fn execute(&self, _input: &str) -> Result<String, Error> {
        Ok("HR: 72, SpO2: 98".to_string())
    }
}

struct ChartAssessment;
#[async_trait]
impl Tool for ChartAssessment {
    async fn execute(&self, input: &str) -> Result<String, Error> {
        Ok(format!("Assessment for {}: Stable.", input))
    }
}

struct RecordIO;
#[async_trait]
impl Tool for RecordIO {
    async fn execute(&self, input: &str) -> Result<String, Error> {
        Ok(format!("I/O for {}: Intake 500ml, Output 400ml.", input))
    }
}

struct CheckLabValues;
#[async_trait]
impl Tool for CheckLabValues {
    async fn execute(&self, input: &str) -> Result<String, Error> {
        Ok(format!("Labs for {}: Glucose 95.", input))
    }
}

struct AdministerMedication;
#[async_trait]
impl Tool for AdministerMedication {
    async fn execute(&self, input: &str) -> Result<String, Error> {
        Ok(format!("Administered to {}: Aspirin 81mg.", input))
    }
}

struct AnalyzeTestResults;
#[async_trait]
impl Tool for AnalyzeTestResults {
    async fn execute(&self, input: &str) -> Result<String, Error> {
        Ok(format!("Results for {}: Normal.", input))
    }
}

struct HospitalNetwork {
    messages: Arc<Mutex<Vec<(String, String, String)>>>,
}

impl HospitalNetwork {
    fn new() -> Self {
        Self { messages: Arc::new(Mutex::new(Vec::new())) }
    }
    async fn send_message(&self, from: &str, to: &str, message: &str) {
        self.messages.lock().unwrap().push((from.to_string(), to.to_string(), message.to_string()));
    }
}

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(long, default_value = "sqlite")]
    db_type: String,
    #[arg(long, default_value = "nurse")]
    mode: String,
    #[arg(long)]
    voice: bool,
}

#[derive(Subcommand)]
enum Commands {
    MonitorVitals { patient_id: String },
}

enum DatabaseBackend {
    SQLite(SqlitePool),
}

#[derive(Clone)]
struct NoahAgent<M: CompletionModel> {
    chart: Arc<PatientChart>,
    rig_agent: Agent<M>,
    db: DatabaseBackend,
    preferences: UserPreferences,
    mode: AgentMode,
    tools: HashMap<String, Arc<dyn Tool>>,
    task_queue: Arc<Mutex<VecDeque<PatientTask>>>,
    hospital_network: HospitalNetwork,
}

impl<M: CompletionModel + Send + Sync> NoahAgent<M> {
    async fn new_with_db(
        chart: Arc<PatientChart>,
        training_data: Vec<String>,
        model: M,
        db_type: &str,
        db_url: &str,
        mode: AgentMode,
        voice_enabled: bool,
    ) -> Result<Self> {
        let rig_agent = AgentBuilder::new(model)
            .preamble("You are Noah, a healthcare AI.")
            .context(&training_data.join("\n"))
            .build();
        let db = match db_type {
            "sqlite" => DatabaseBackend::SQLite(SqlitePool::connect(db_url).await?),
            _ => return Err(anyhow::anyhow!("Unsupported DB_TYPE: {}", db_type)),
        };
        let tools = HashMap::from([
            ("vitals_checker".to_string(), Arc::new(VitalsChecker) as Arc<dyn Tool>),
            ("chart_assessment".to_string(), Arc::new(ChartAssessment) as Arc<dyn Tool>),
            ("record_io".to_string(), Arc::new(RecordIO) as Arc<dyn Tool>),
            ("check_lab_values".to_string(), Arc::new(CheckLabValues) as Arc<dyn Tool>),
            ("administer_medication".to_string(), Arc::new(AdministerMedication) as Arc<dyn Tool>),
            ("analyze_test_results".to_string(), Arc::new(AnalyzeTestResults) as Arc<dyn Tool>),
        ]);
        Ok(Self {
            chart,
            rig_agent,
            db,
            preferences: UserPreferences { language: "English".to_string(), voice_enabled },
            mode,
            tools,
            task_queue: Arc::new(Mutex::new(VecDeque::new())),
            hospital_network: HospitalNetwork::new(),
        })
    }

    async fn monitor_vitals(&self) -> Result<String> {
        let checker = self.tools.get("vitals_checker").unwrap();
        let vitals = checker.execute(&self.chart.patient_id).await?;
        let response = match self.mode {
            AgentMode::Nurse => format!("Vitals for nurse review: {}", vitals),
            AgentMode::Patient => format!("Your vitals are: {}", vitals),
        };
        Ok(if self.preferences.voice_enabled {
            external_apis::text_to_speech(&response, &self.preferences.language).await
        } else {
            response
        })
    }

    async fn add_task(&self, description: &str, severity: u8) -> Result<()> {
        let task = PatientTask {
            id: self.task_queue.lock().unwrap().len() as u32 + 1,
            patient_id: self.chart.patient_id.clone(),
            description: description.to_string(),
            severity,
            time_due: SystemTime::now() + Duration::from_secs(3600),
        };
        self.task_queue.lock().unwrap().push_back(task);
        Ok(())
    }

    async fn prioritize_tasks(&self) -> Result<String> {
        let mut queue = self.task_queue.lock().unwrap().clone();
        queue.make_contiguous().sort_by(|a, b| b.severity.cmp(&a.severity));
        Ok(queue.front().map_or("No tasks".to_string(), |t| t.description.clone()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let cli = Cli::parse();
    let patient_id = match &cli.command {
        Commands::MonitorVitals { patient_id } => patient_id.clone(),
    };
    let chart = Arc::new(PatientChart { patient_id });
    let gemini_model = GeminiModel;
    let training_data = vec!["Default training data".to_string()];
    let mode = match cli.mode.as_str() {
        "nurse" => AgentMode::Nurse,
        "patient" => AgentMode::Patient,
        _ => AgentMode::Nurse,
    };
    let agent = NoahAgent::new_with_db(chart, training_data, gemini_model, &cli.db_type, "noah.db", mode, cli.voice).await?;

    match cli.command {
        Commands::MonitorVitals { patient_id: _ } => {
            agent.add_task("Check vitals", 5).await?;
            println!("Task: {}", agent.prioritize_tasks().await?);
            println!("Vitals: {}", agent.monitor_vitals().await?);
        }
    }
    Ok(())
}
