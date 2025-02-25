use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH, Instant};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::connect_async;
use futures_util::StreamExt;
use sqlx::{SqlitePool, Pool, Sqlite, Transaction};
use clap::{Parser, Subcommand};
use anyhow::{Result, Error};
use actix_web::{web, App as ActixApp, HttpServer, Responder, http};
use actix_web::middleware::Logger;
use tracing::{info, error, warn, debug};
use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{BuildError, PrometheusBuilder, PrometheusHandle};
use once_cell::sync::OnceCell;
use ctor::ctor;
use uuid::Uuid;
use tokio::time::interval;
use dashmap::DashMap;
use bb8_redis::bb8::Pool as RedisPool;
use bb8_redis::RedisConnectionManager;
use tokio::task::spawn_blocking;
use pprof::protos::Message;
use pprof::ProfilerGuard;
use serde_json::Value;
use async_trait::async_trait;
#[cfg(feature = "clickhouse")]
use clickhouse_rs::{Pool as ClickHousePool, Block, ClientHandle};

// Modules
mod arc_framework;
mod tools;

// Constants
const BATCH_SIZE: usize = 100;
const BATCH_TIMEOUT: Duration = Duration::from_secs(2);
const REDIS_TTL: u64 = 300; // 5 minutes

// Data Models
#[derive(Debug, Serialize, Deserialize, Clone)]
struct PatientChart {
    patient_id: String,
    name: String,
    age: i32,
    sex: String,
    diagnosis: String,
    room: String,
    ventilated: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PatientTask {
    id: u32,
    patient_id: String,
    description: String,
    severity: u8,
    time_due: SystemTime,
    event_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OrderSet {
    order_id: String,
    type_: String,
    details: String,
    parameters: HashMap<String, f32>,
    interventions: Vec<String>,
    timestamp: DateTime<Utc>,
}

#[derive(Clone)]
struct UserPreferences {
    language: String,
    voice_enabled: bool,
}

// Enums
enum AgentMode {
    Nurse,
    Patient,
}

enum DatabaseBackend {
    SQLite(Pool<Sqlite>),
    #[cfg(feature = "clickhouse")]
    ClickHouse(ClickHousePool),
}

// CLI
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
    #[arg(long, default_value = "false")]
    profile: bool,
    #[arg(long, default_value = "4")]
    worker_count: usize,
    #[arg(long, default_value = "100")]
    buffer_size: usize,
}

#[derive(Subcommand)]
enum Commands {
    MonitorVitals { patient_id: String },
    MonitorVitalsStream { patient_id: String },
    LaunchFlood,
    ChartVitals { patient_id: String, interval: Option<u64> },
    ManageOrders { patient_id: String },
    AnalyzeLabs { patient_id: String },
    ChartLabs { patient_id: String },
    ManageMAR { patient_id: String },
    MonitorBlockchain { patient_id: String },
    #[cfg(feature = "clickhouse")]
    TrendVitals { patient_id: String, key: String, hours: Option<i64> },
    #[cfg(feature = "clickhouse")]
    TrendLabs { patient_id: String, key: String, hours: Option<i64> },
    #[cfg(feature = "clickhouse")]
    TrendMeds { patient_id: String, medication: String, hours: Option<i64> },
    #[cfg(feature = "clickhouse")]
    TrendIO { patient_id: String, key: String, hours: Option<i64> },
}

// Core structs
struct HospitalNetwork {
    messages: Arc<Mutex<Vec<(String, String, String)>>>,
}

impl HospitalNetwork {
    fn new() -> Self {
        Self { messages: Arc::new(Mutex::new(Vec::new())) }
    }

    async fn send_message(&self, from: &str, to: &str, message: &str) -> Result<()> {
        let mut messages = self.messages.lock().await;
        messages.push((from.to_string(), to.to_string(), message.to_string()));
        Ok(())
    }
}

struct BlockchainExecutor {
    client: Arc<Mutex<ArcClient>>, // Placeholder ARC client
}

impl BlockchainExecutor {
    async fn execute_intervention(&self, patient_id: &str, intervention: &str) -> Result<String> {
        let data = format!("Intervention: {} for {}", intervention, patient_id);
        let hash = arc_framework::log_to_blockchain("Execute Intervention", patient_id, &data).await?;
        Ok(format!("Executed {} with hash: {}", intervention, hash))
    }

    async fn batch_log(&self, events: Vec<(&str, &str, &str)>) -> Result<Vec<String>> {
        arc_framework::batch_log_to_blockchain(events).await
    }
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
    ordersets: Arc<Mutex<Vec<OrderSet>>>,
    alerts: Arc<Mutex<Vec<String>>>,
    cache: DashMap<String, Value>,
    redis_pool: RedisPool<RedisConnectionManager>,
    blockchain_executor: Arc<BlockchainExecutor>,
}

impl<M: CompletionModel + Send + Sync> NoahAgent<M> {
    async fn new(
        chart: Arc<PatientChart>,
        training_data: Vec<String>,
        model: M,
        db_type: &str,
        db_url: &str,
        mode: AgentMode,
        voice_enabled: bool,
    ) -> Result<Self> {
        let rig_agent = AgentBuilder::new(model)
            .preamble("Noah: Critical Care AI Assistant")
            .context(&training_data.join("\n"))
            .build();

        let db = match db_type {
            "sqlite" => Self::init_sqlite(db_url).await?,
            #[cfg(feature = "clickhouse")]
            "clickhouse" => Self::init_clickhouse(db_url).await?,
            _ => return Err(anyhow::anyhow!("Unsupported DB_TYPE: {}", db_type)),
        };

        let redis_manager = RedisConnectionManager::new("redis://redis:6379")?;
        let redis_pool = RedisPool::builder()
            .max_size(20)
            .build(redis_manager)
            .await?;

        Ok(Self {
            chart,
            rig_agent,
            db,
            preferences: UserPreferences { language: "English".to_string(), voice_enabled },
            mode,
            tools: Self::init_tools(),
            task_queue: Arc::new(Mutex::new(VecDeque::new())),
            hospital_network: HospitalNetwork::new(),
            ordersets: Arc::new(Mutex::new(Vec::new())),
            alerts: Arc::new(Mutex::new(Vec::new())),
            cache: DashMap::new(),
            redis_pool,
            blockchain_executor: Arc::new(BlockchainExecutor {
                client: Arc::new(Mutex::new(ArcClient)),
            }),
        })
    }

    async fn init_sqlite(db_url: &str) -> Result<DatabaseBackend> {
        let pool = SqlitePool::connect(db_url).await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS patients (id TEXT PRIMARY KEY, name TEXT, age INTEGER, sex TEXT, diagnosis TEXT, room TEXT, ventilated INTEGER); \
             CREATE TABLE IF NOT EXISTS ordersets (patient_id TEXT, order_id TEXT PRIMARY KEY, type TEXT, details TEXT, parameters TEXT, interventions TEXT, timestamp TEXT); \
             CREATE TABLE IF NOT EXISTS labs (patient_id TEXT, timestamp BIGINT, key TEXT, value REAL, event_id TEXT); \
             CREATE TABLE IF NOT EXISTS medications (patient_id TEXT, order_id TEXT, medication TEXT, administered_at TEXT, event_id TEXT); \
             CREATE TABLE IF NOT EXISTS vitals (patient_id TEXT, timestamp BIGINT, key TEXT, value REAL, event_id TEXT); \
             CREATE TABLE IF NOT EXISTS io (patient_id TEXT, timestamp BIGINT, key TEXT, value REAL, event_id TEXT);"
        ).execute(&pool).await?;
        Ok(DatabaseBackend::SQLite(pool))
    }

    #[cfg(feature = "clickhouse")]
    async fn init_clickhouse(db_url: &str) -> Result<DatabaseBackend> {
        let pool = ClickHousePool::new(db_url);
        let mut client = pool.get_handle().await?;
        let tables = [
            ("vitals", "value Float32 CODEC(ZSTD), INDEX value_idx value TYPE minmax GRANULARITY 8192"),
            ("labs", "value Float32 CODEC(ZSTD), INDEX value_idx value TYPE minmax GRANULARITY 8192"),
            ("io_outputs", "value Float32 CODEC(ZSTD), INDEX value_idx value TYPE minmax GRANULARITY 8192"),
            ("io_inputs", "value String CODEC(ZSTD)"),
            ("medications", "order_id String, medication String CODEC(ZSTD)"),
        ];

        for (table, extra_columns) in tables.iter() {
            let query = format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    patient_id String,
                    timestamp Int64,
                    key String CODEC(ZSTD),
                    {},
                    event_id String
                ) ENGINE = MergeTree()
                PARTITION BY toDate(timestamp / 1000)
                ORDER BY (patient_id, timestamp)
                SETTINGS index_granularity = 8192",
                table, extra_columns
            );
            client.execute(&query).await?;
        }
        Ok(DatabaseBackend::ClickHouse(pool))
    }

    fn init_tools() -> HashMap<String, Arc<dyn Tool>> {
        HashMap::from([
            ("vitals_checker".to_string(), Arc::new(VitalsChecker) as Arc<dyn Tool>),
            ("chart_assessment".to_string(), Arc::new(ChartAssessment) as Arc<dyn Tool>),
            ("record_io".to_string(), Arc::new(RecordIO) as Arc<dyn Tool>),
            ("check_lab_values".to_string(), Arc::new(CheckLabValues) as Arc<dyn Tool>),
            ("administer_medication".to_string(), Arc::new(AdministerMedication) as Arc<dyn Tool>),
            ("analyze_test_results".to_string(), Arc::new(AnalyzeTestResults) as Arc<dyn Tool>),
        ])
    }

    async fn store_in_redis(&self, key: String, value: &impl Serialize) -> Result<()> {
        let mut conn = self.redis_pool.get().await?;
        conn.set_ex(key, serde_json::to_string(value)?, REDIS_TTL).await?;
        Ok(())
    }

    async fn log_to_blockchain(&self, action: &str, data: &str) -> Result<String> {
        arc_framework::log_to_blockchain(action, &self.chart.patient_id, data).await
    }

    async fn update_ordersets(&self, ordersets: Vec<OrderSet>) -> Result<()> {
        let mut current_ordersets = self.ordersets.lock().await;
        current_ordersets.clear();
        current_ordersets.extend(ordersets);
        Ok(())
    }

    async fn add_alert(&self, message: &str) -> Result<()> {
        self.alerts.lock().await.push(message.to_string());
        Ok(())
    }

    async fn flush_alerts(&self) -> Vec<String> {
        self.alerts.lock().await.drain(..).collect()
    }

    async fn update_vitals(&self, vitals: HashMap<String, f32>, ordersets: Vec<OrderSet>) -> Result<()> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let event_id = Uuid::new_v4().to_string();
        let cache_key = format!("vitals_{}_{}_{}", self.chart.patient_id, timestamp, event_id);

        self.store_in_redis(cache_key.clone(), &vitals).await?;
        match &self.db {
            DatabaseBackend::SQLite(pool) => {
                let mut tx = pool.begin().await?;
                Self::insert_key_value(&mut tx, "vitals", &self.chart.patient_id, timestamp, &vitals, &event_id).await?;
                tx.commit().await?;
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(pool) => {
                let mut client = pool.get_handle().await?;
                Self::insert_clickhouse(&mut client, "vitals", &self.chart.patient_id, timestamp, &vitals, &event_id).await?;
            }
        }

        self.cache.insert(cache_key.clone(), Value::from(vitals.clone()));
        self.log_to_blockchain("Update Vitals", &serde_json::to_string(&vitals)?).await?;
        self.validate_and_alert(vitals, ordersets).await?;
        Ok(())
    }

    async fn update_labs(&self, labs: HashMap<String, f32>) -> Result<String> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let event_id = Uuid::new_v4().to_string();
        let cache_key = format!("labs_{}_{}_{}", self.chart.patient_id, timestamp, event_id);

        self.store_in_redis(cache_key.clone(), &labs).await?;
        match &self.db {
            DatabaseBackend::SQLite(pool) => {
                let mut tx = pool.begin().await?;
                Self::insert_key_value(&mut tx, "labs", &self.chart.patient_id, timestamp, &labs, &event_id).await?;
                tx.commit().await?;
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(pool) => {
                let mut client = pool.get_handle().await?;
                Self::insert_clickhouse(&mut client, "labs", &self.chart.patient_id, timestamp, &labs, &event_id).await?;
            }
        }

        self.cache.insert(cache_key.clone(), Value::from(labs.clone()));
        self.log_to_blockchain("Update Labs", &serde_json::to_string(&labs)?).await?;
        self.validate_and_alert_labs(labs).await
    }

    async fn update_medications(&self, mar: Vec<Value>) -> Result<()> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let event_id = Uuid::new_v4().to_string();
        let cache_key = format!("mar_{}_{}_{}", self.chart.patient_id, timestamp, event_id);

        self.store_in_redis(cache_key.clone(), &mar).await?;
        match &self.db {
            DatabaseBackend::SQLite(pool) => {
                let mut tx = pool.begin().await?;
                for record in &mar {
                    let order_id = record["orderId"].as_str().unwrap_or("");
                    let medication = record["medication"].as_str().unwrap_or("");
                    let administered_at = record["administeredAt"].as_i64().unwrap_or(timestamp);
                    sqlx::query("INSERT INTO medications (patient_id, order_id, medication, administered_at, event_id) VALUES (?, ?, ?, ?, ?)")
                        .bind(&self.chart.patient_id)
                        .bind(order_id)
                        .bind(medication)
                        .bind(administered_at.to_string())
                        .bind(&event_id)
                        .execute(&mut *tx)
                        .await?;
                }
                tx.commit().await?;
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(pool) => {
                let mut client = pool.get_handle().await?;
                let mut block = Block::with_capacity(BATCH_SIZE);
                for record in &mar {
                    let order_id = record["orderId"].as_str().unwrap_or("");
                    let medication = record["medication"].as_str().unwrap_or("");
                    block.push_row((
                        self.chart.patient_id.clone(),
                        timestamp,
                        "med".to_string(), // Key is fixed for medications
                        order_id.to_string(),
                        medication.to_string(),
                        event_id.clone(),
                    ))?;
                }
                client.insert("medications", block).await?;
            }
        }

        self.cache.insert(cache_key.clone(), Value::from(mar.clone()));
        self.log_to_blockchain("Update Medications", &serde_json::to_string(&mar)?).await?;
        self.validate_and_alert_mar(mar).await?;
        Ok(())
    }

    async fn update_io(&self, io_inputs: HashMap<String, Value>, io_outputs: HashMap<String, f32>) -> Result<()> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let event_id = Uuid::new_v4().to_string();
        let outputs_key = format!("io_outputs_{}_{}_{}", self.chart.patient_id, timestamp, &event_id);
        let inputs_key = format!("io_inputs_{}_{}_{}", self.chart.patient_id, timestamp, &event_id);

        self.store_in_redis(outputs_key.clone(), &io_outputs).await?;
        self.store_in_redis(inputs_key.clone(), &io_inputs).await?;

        match &self.db {
            DatabaseBackend::SQLite(pool) => {
                let mut tx = pool.begin().await?;
                Self::insert_key_value(&mut tx, "io", &self.chart.patient_id, timestamp, &io_outputs, &event_id).await?;
                tx.commit().await?;
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(pool) => {
                let mut client = pool.get_handle().await?;
                Self::insert_clickhouse(&mut client, "io_outputs", &self.chart.patient_id, timestamp, &io_outputs, &event_id).await?;

                let mut inputs_block = Block::with_capacity(BATCH_SIZE);
                for (key, value) in &io_inputs {
                    let value_str = match value {
                        Value::Number(n) => n.to_string(),
                        Value::String(s) => s.clone(),
                        _ => continue, // Skip non-numeric/string for now
                    };
                    inputs_block.push_row((
                        self.chart.patient_id.clone(),
                        timestamp,
                        key.clone(),
                        value_str,
                        event_id.clone(),
                    ))?;
                }
                if !inputs_block.is_empty() {
                    client.insert("io_inputs", inputs_block).await?;
                }
            }
        }

        self.cache.insert(outputs_key, Value::from(io_outputs.clone()));
        self.cache.insert(inputs_key, Value::from(io_inputs.clone()));
        self.log_to_blockchain("Update I/O Outputs", &serde_json::to_string(&io_outputs)?).await?;
        self.log_to_blockchain("Update I/O Inputs", &serde_json::to_string(&io_inputs)?).await?;
        self.validate_and_alert_io(io_inputs, io_outputs).await?;
        Ok(())
    }

    #[cfg(feature = "clickhouse")]
    async fn insert_clickhouse(client: &mut ClientHandle, table: &str, patient_id: &str, timestamp: i64, data: &HashMap<String, f32>, event_id: &str) -> Result<()> {
        let mut block = Block::with_capacity(BATCH_SIZE);
        for (key, value) in data {
            block.push_row((
                patient_id.to_string(),
                timestamp,
                key.clone(),
                *value,
                event_id.to_string(),
            ))?;
        }
        client.insert(table, block).await?;
        Ok(())
    }

    async fn insert_key_value(tx: &mut Transaction<'_, Sqlite>, table: &str, patient_id: &str, timestamp: i64, data: &HashMap<String, f32>, event_id: &str) -> Result<()> {
        for (key, value) in data {
            sqlx::query(&format!("INSERT INTO {} (patient_id, timestamp, key, value, event_id) VALUES (?, ?, ?, ?, ?)", table))
                .bind(patient_id)
                .bind(timestamp)
                .bind(key)
                .bind(value)
                .bind(event_id)
                .execute(&mut *tx)
                .await?;
        }
        Ok(())
    }

    async fn process_data(&self, vitals: HashMap<String, f32>, ordersets: Vec<OrderSet>, mar: Vec<Value>, labs: HashMap<String, f32>, io_inputs: HashMap<String, Value>, io_outputs: HashMap<String, f32>, chest_xray: Option<&str>) -> Result<()> {
        self.update_ordersets(ordersets.clone()).await?;
        tokio::try_join!(
            self.update_vitals(vitals, ordersets),
            self.update_io(io_inputs, io_outputs),
            self.update_labs(labs),
            self.update_medications(mar)
        )?;
        if let Some(xray) = chest_xray {
            if xray == "Performed" {
                info!("Chest X-Ray performed for {} at 6 AM", self.chart.patient_id);
            }
        }
        Ok(())
    }

    async fn validate_data(&self, vitals: HashMap<String, f32>, ordersets: Vec<OrderSet>, mar: Vec<Value>, labs: HashMap<String, f32>, io_inputs: HashMap<String, Value>, io_outputs: HashMap<String, f32>, chest_xray: Option<&str>) -> Result<(HashMap<String, f32>, Vec<OrderSet>, Vec<Value>, HashMap<String, f32>, HashMap<String, Value>, HashMap<String, f32>, Option<String>)> {
        Ok((vitals, ordersets, mar, labs, io_inputs, io_outputs, chest_xray.map(String::from)))
    }

    async fn store_and_alert(&self, data: (HashMap<String, f32>, Vec<OrderSet>, Vec<Value>, HashMap<String, f32>, HashMap<String, Value>, HashMap<String, f32>, Option<String>)) -> Result<()> {
        self.process_data(data.0, data.1, data.2, data.3, data.4, data.5, data.6.as_deref()).await
    }

    async fn validate_and_alert(&self, vitals: HashMap<String, f32>, ordersets: Vec<OrderSet>) -> Result<()> {
        let ordersets = self.ordersets.lock().await;
        for order in ordersets.iter() {
            for (param, threshold) in &order.parameters {
                if let Some(value) = vitals.get(param) {
                    match param.as_str() {
                        "MAP" if *value <= threshold => {
                            let alert = format!("MAP {} below threshold {}!", value, threshold);
                            self.add_alert(&alert).await?;
                            for intervention in &order.interventions {
                                self.add_task(intervention, 8).await?;
                            }
                        }
                        "HR" if *value >= threshold => {
                            let alert = format!("HR {} above threshold {}!", value, threshold);
                            self.add_alert(&alert).await?;
                            for intervention in &order.interventions {
                                self.add_task(intervention, 8).await?;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    async fn validate_and_alert_io(&self, io_inputs: HashMap<String, Value>, io_outputs: HashMap<String, f32>) -> Result<()> {
        let ordersets = self.ordersets.lock().await;
        for order in ordersets.iter() {
            if order.type_ == "Fluid" || order.type_ == "CRRT" {
                for (param, threshold) in &order.parameters {
                    if let Some(value) = io_outputs.get(param) {
                        if *value < threshold {
                            let alert = format!("{} {} below threshold {}!", param, value, threshold);
                            self.add_alert(&alert).await?;
                            for intervention in &order.interventions {
                                self.add_task(intervention, 8).await?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn validate_and_alert_labs(&self, labs: HashMap<String, f32>) -> Result<String> {
        let mut response = String::new();
        let ordersets = self.ordersets.lock().await;
        for order in ordersets.iter() {
            for (param, threshold) in &order.parameters {
                if let Some(value) = labs.get(param) {
                    match param.as_str() {
                        "K" if *value < threshold => {
                            response.push_str(&format!("K {} below {}, administering potassium. ", value, threshold));
                            self.add_task("Administer 20 mEq potassium", 6).await?;
                        }
                        "LacticAcid" if *value > threshold => {
                            let alert = format!("Lactic Acid {} above threshold {}!", value, threshold);
                            self.add_alert(&alert).await?;
                            for intervention in &order.interventions {
                                self.add_task(intervention, 7).await?;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(response)
    }

    async fn validate_and_alert_mar(&self, mar: Vec<Value>) -> Result<()> {
        for record in mar {
            if let Some(med) = record["medication"].as_str() {
                if med.contains("Levophed") {
                    let mut redis_conn = self.redis_pool.get().await?;
                    if let Ok(vitals_json) = redis_conn.get::<_, String>(format!("vitals_{}", self.chart.patient_id)).await {
                        if let Ok(vitals) = serde_json::from_str::<HashMap<String, f32>>(&vitals_json) {
                            if vitals.get("MAP").unwrap_or(&0.0) < &65.0 {
                                self.add_task("Titrate Levophed by 0.02 mcg/kg/min", 7).await?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn monitor_ehr(&self) -> Result<String> {
        let logs = arc_framework::query_blockchain(&self.chart.patient_id, "all").await?;
        Ok(format!("EHR Monitoring: {}", logs.join(", ")))
    }

    async fn monitor_vitals(&self) -> Result<String> {
        let checker = self.tools.get("vitals_checker").ok_or_else(|| anyhow::anyhow!("Vitals checker tool not found"))?;
        let vitals = checker.execute(&self.chart.patient_id).await?;
        let response = match self.mode {
            AgentMode::Nurse => format!("Vitals for nurse review: {}", vitals),
            AgentMode::Patient => format!("Your vitals are: {}", vitals),
        };
        Ok(if self.preferences.voice_enabled {
            text_to_speech(&response, &self.preferences.language).await.unwrap_or(response)
        } else {
            response
        })
    }

    async fn add_task(&self, description: &str, severity: u8) -> Result<()> {
        let task = PatientTask {
            id: self.task_queue.lock().await.len() as u32 + 1,
            patient_id: self.chart.patient_id.clone(),
            description: description.to_string(),
            severity,
            time_due: SystemTime::now() + Duration::from_secs(3600),
            event_id: Uuid::new_v4(),
        };
        self.task_queue.lock().await.push_back(task);
        Ok(())
    }

    async fn prioritize_tasks(&self) -> Result<String> {
        let mut queue = self.task_queue.lock().await.clone();
        queue.make_contiguous().sort_by(|a, b| b.severity.cmp(&a.severity));
        Ok(queue.front().map_or("No tasks".to_string(), |t| t.description.clone()))
    }

    async fn execute_task(&self, task_description: &str) -> Result<String> {
        match task_description {
            "Check vitals" => self.tools.get("vitals_checker").unwrap().execute(&self.chart.patient_id).await,
            "Chart assessment" => self.tools.get("chart_assessment").unwrap().execute(&self.chart.patient_id).await,
            "Record I/O" => self.tools.get("record_io").unwrap().execute(&self.chart.patient_id).await,
            "Check lab values" => self.tools.get("check_lab_values").unwrap().execute(&self.chart.patient_id).await,
            "Administer medication" => self.tools.get("administer_medication").unwrap().execute(&self.chart.patient_id).await,
            "Analyze test results" => self.tools.get("analyze_test_results").unwrap().execute(&self.chart.patient_id).await,
            "Titrate Levophed by 0.02 mcg/kg/min" => {
                self.blockchain_executor.execute_intervention(&self.chart.patient_id, task_description).await?;
                Ok("Titrating Levophed on blockchain".to_string())
            }
            "Administer 20 mEq potassium" => {
                self.blockchain_executor.execute_intervention(&self.chart.patient_id, task_description).await?;
                Ok("Administering 20 mEq potassium on blockchain".to_string())
            }
            _ => Ok("Task not recognized".to_string()),
        }
    }

    #[cfg(feature = "clickhouse")]
    async fn get_trend(&self, table: &str, key: &str, hours: i64, value_column: &str) -> Result<Vec<(i64, Value)>> {
        if let DatabaseBackend::ClickHouse(pool) = &self.db {
            let mut client = pool.get_handle().await?;
            let since = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64 - (hours * 3600);
            let query = format!(
                "SELECT toUnixTimestamp(toStartOfHour(fromUnixTimestamp(timestamp))) AS hour,
                        {} AS value
                 FROM {}
                 WHERE patient_id = ?
                   AND key = ?
                   AND timestamp >= ?
                 GROUP BY hour
                 ORDER BY hour ASC",
                if value_column == "value" { "AVG(value)" } else { "arrayJoin([value])" },
                table
            );
            let rows = client.query(&query)
                .bind(&self.chart.patient_id)
                .bind(key)
                .bind(since)
                .fetch_all().await?;
            let result: Vec<(i64, Value)> = rows.into_iter()
                .map(|row| {
                    let hour: i64 = row.get(0)?;
                    let value = if value_column == "value" {
                        Value::Number(serde_json::Number::from_f64(row.get::<f32, _>(1)? as f64).unwrap())
                    } else {
                        Value::String(row.get::<String, _>(1)?)
                    };
                    Ok((hour, value))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(result)
        } else {
            Err(anyhow::anyhow!("ClickHouse not enabled"))
        }
    }

    #[cfg(feature = "clickhouse")]
    async fn get_vitals_trend(&self, key: &str, hours: i64) -> Result<Vec<(i64, Value)>> {
        self.get_trend("vitals", key, hours, "value").await
    }

    #[cfg(feature = "clickhouse")]
    async fn get_labs_trend(&self, key: &str, hours: i64) -> Result<Vec<(i64, Value)>> {
        self.get_trend("labs", key, hours, "value").await
    }

    #[cfg(feature = "clickhouse")]
    async fn get_meds_trend(&self, medication: &str, hours: i64) -> Result<Vec<(i64, Value)>> {
        if let DatabaseBackend::ClickHouse(pool) = &self.db {
            let mut client = pool.get_handle().await?;
            let since = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64 - (hours * 3600);
            let query = "
                SELECT timestamp, order_id
                FROM medications
                WHERE patient_id = ?
                  AND medication = ?
                  AND timestamp >= ?
                ORDER BY timestamp ASC";
            let rows = client.query(query)
                .bind(&self.chart.patient_id)
                .bind(medication)
                .bind(since)
                .fetch_all().await?;
            let result: Vec<(i64, Value)> = rows.into_iter()
                .map(|row| Ok((row.get(0)?, Value::String(row.get(1)?))))
                .collect::<Result<Vec<_>>>()?;
            Ok(result)
        } else {
            Err(anyhow::anyhow!("ClickHouse not enabled"))
        }
    }

    #[cfg(feature = "clickhouse")]
    async fn get_io_trend(&self, key: &str, hours: i64) -> Result<Vec<(i64, Value)>> {
        let outputs = self.get_trend("io_outputs", key, hours, "value").await;
        if outputs.is_ok() {
            return outputs;
        }
        self.get_trend("io_inputs", key, hours, "value").await
    }
}

struct FloodApp {
    agent: NoahAgent<GeminiModel>,
    selected_tab: String,
    patient_data: DashMap<String, String>,
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(String),
    UpdateData(String, String),
}

impl Application for FloodApp {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = NoahAgent<GeminiModel>;

    fn new(agent: Self::Flags) -> (Self, Command<Self::Message>) {
        (Self {
            agent,
            selected_tab: "Vitals".to_string(),
            patient_data: DashMap::new(),
        }, Command::none())
    }

    fn title(&self) -> String {
        "Flood EHR".to_string()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::TabSelected(tab) => self.selected_tab = tab,
            Message::UpdateData(key, value) => self.patient_data.insert(key, value),
        }
        Command::none()
    }

    fn view(&self) -> Element<Self::Message> {
        use iced::widget::{button, column, row, text, Container, Scrollable};
        let tabs = row![
            button("Vitals").on_press(Message::TabSelected("Vitals".to_string())),
            button("Labs").on_press(Message::TabSelected("Labs".to_string())),
            button("Meds").on_press(Message::TabSelected("Meds".to_string())),
            button("Orders").on_press(Message::TabSelected("Orders".to_string())),
            button("I/O").on_press(Message::TabSelected("I/O".to_string())),
        ].spacing(10);

        let content = match self.selected_tab.as_str() {
            "Vitals" => Scrollable::new(text(self.patient_data.get("vitals").map_or("No vitals data".to_string(), |v| v.clone()))).width(300),
            "Labs" => Scrollable::new(text(self.patient_data.get("labs").map_or("No lab data".to_string(), |v| v.clone()))).width(300),
            "Meds" => Scrollable::new(text(self.patient_data.get("meds").map_or("No medication data".to_string(), |v| v.clone()))).width(300),
            "Orders" => Scrollable::new(text(self.patient_data.get("orders").map_or("No orders data".to_string(), |v| v.clone()))).width(300),
            "I/O" => Scrollable::new(text(self.patient_data.get("io").map_or("No I/O data".to_string(), |v| v.clone()))).width(300),
            _ => text("Select a tab"),
        };

        Container::new(column![tabs, content].spacing(20))
            .style(iced::theme::Container::Custom(Box::new(|_| iced::widget::container::Style {
                background: Some(iced::Color::from_rgb(0.96, 0.96, 0.98).into()),
                border_radius: 8.0,
                border_width: 1.0,
                border_color: iced::Color::from_rgb(0.8, 0.8, 0.8),
                ..Default::default()
            })))
            .width(320)
            .height(480)
            .into()
    }
}

static PROMETHEUS_HANDLE: OnceCell<PrometheusHandle> = OnceCell::new();

#[derive(Debug, thiserror::Error)]
pub enum MetricsError {
    #[error("Failed to install metrics recorder")]
    InstallRecorderError(BuildError),
}

#[ctor]
fn init_metrics() {
    let handle = setup_metrics_exporter().expect("Failed to setup metrics exporter");
    PROMETHEUS_HANDLE.set(handle).ok();

    counter!("vitals_processed", "Number of vital sign updates processed");
    histogram!("vitals_processing_duration", "Time taken to process vital updates");
    counter!("labs_processed", "Number of lab updates processed");
    histogram!("labs_processing_duration", "Time taken to process lab updates");
    counter!("meds_processed", "Number of medication updates processed");
    histogram!("meds_processing_duration", "Time taken to process medication updates");
    counter!("io_processed", "Number of I/O updates processed");
    histogram!("io_processing_duration", "Time taken to process I/O updates");
    gauge!("active_pipelines", "Number of active data pipelines");
    gauge!("active_tasks", "Number of active tasks in queue");
}

fn setup_metrics_exporter() -> Result<PrometheusHandle, MetricsError> {
    PrometheusBuilder::new()
        .add_global_label("service", "noah-flood")
        .install_recorder()
        .map_err(MetricsError::InstallRecorderError)
}

async fn run_vitals_pipeline(
    patient_id: String,
    worker_count: usize,
    buffer_size: usize,
    agent: NoahAgent<GeminiModel>,
) -> Result<()> {
    gauge!("active_pipelines").increment(1.0);
    let patient_tasks = agent.task_queue.lock().await.len() as f64;
    gauge!("active_tasks").set(patient_tasks);

    let retry_policy = Retry::from_fn(|| tokio::time::sleep(Duration::from_secs(5)), 5);
    let ws_stream = retry_policy.retry(|| async {
        let (stream, _) = connect_async("ws://websocket-server:8080/vitals").await?;
        Ok::<_, Error>(stream)
    }).await?;
    let (_, mut ws_receiver) = ws_stream.split();
    let (tx, rx) = mpsc::channel::<(HashMap<String, f32>, Vec<OrderSet>, Vec<Value>, HashMap<String, f32>, HashMap<String, Value>, HashMap<String, f32>, Option<String>)>(buffer_size);

    let workers: Vec<_> = (0..worker_count)
        .map(|_| {
            let rx = rx.clone();
            let agent = agent.clone();
            tokio::spawn(async move {
                let mut interval = interval(Duration::from_millis(50));
                let mut batch = Vec::with_capacity(BATCH_SIZE);
                let mut last_batch = Instant::now();
                while let Some(data) = rx.recv().await {
                    interval.tick().await;
                    batch.push(data);
                    if batch.len() >= BATCH_SIZE || last_batch.elapsed() >= BATCH_TIMEOUT {
                        let start = Instant::now();
                        for (vitals, ordersets, mar, labs, io_inputs, io_outputs, chest_xray) in batch.drain(..) {
                            let category = if !vitals.is_empty() {"vitals"} else if !labs.is_empty() {"labs"} else if !mar.is_empty() {"meds"} else {"io"};
                            if let Err(e) = agent.store_and_alert((vitals, ordersets, mar, labs, io_inputs, io_outputs, chest_xray)).await {
                                error!("Failed to process {}: {}, skipping...", category, e);
                                continue;
                            }
                            counter!(format!("{}_processed", category)).increment(1.0);
                            histogram!(format!("{}_processing_duration", category)).record(start.elapsed().as_secs_f64());
                        }
                        last_batch = Instant::now();
                    }
                }
                Ok::<(), Error>(())
            })
        })
        .collect();

    let task_executor = tokio::spawn({
        let agent = agent.clone();
        async move {
            let mut pipeline = interval(Duration::from_secs(300));
            loop {
                pipeline.tick().await;
                let mut queue = agent.task_queue.lock().await;
                if let Some(task) = queue.front() {
                    if SystemTime::now() >= task.time_due {
                        let task = queue.pop_front().unwrap();
                        drop(queue);
                        match agent.execute_task(&task.description).await {
                            Ok(result) => {
                                if let Err(e) = agent.blockchain_executor.execute_intervention(&agent.chart.patient_id, &task.description).await {
                                    error!("Blockchain execution failed for task {}: {}", task.description, e);
                                }
                                info!("Executed task: {} - Result: {}", task.description, result);
                                gauge!("active_tasks").decrement(1.0);
                            }
                            Err(e) => error!("Task execution failed: {}", e),
                        }
                    }
                }
            }
        }
    });

    let alert_aggregator = tokio::spawn({
        let agent = agent.clone();
        async move {
            let mut interval = interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let alerts = agent.flush_alerts().await;
                if !alerts.is_empty() {
                    let aggregated = alerts.join("; ");
                    if let Err(e) = agent.hospital_network.send_message(&agent.chart.patient_id, "Doctor", &aggregated).await {
                        error!("Failed to send alert: {}", e);
                    }
                    info!("Alerts for {}: {}", agent.chart.patient_id, aggregated);
                }
            }
        }
    });

    let receiver = tokio::spawn(async move {
        while let Some(message) = ws_receiver.next().await {
            match message {
                Ok(msg) if msg.is_text() => {
                    let data: Value = match serde_json::from_str(msg.to_text().unwrap_or_default()) {
                        Ok(data) => data,
                        Err(e) => {
                            warn!("Failed to parse WebSocket message: {}, skipping...", e);
                            continue;
                        }
                    };
                    if let Some(patients) = data["patients"].as_array() {
                        for patient in patients {
                            if patient["patientId"].as_str() == Some(&patient_id) {
                                let vitals: HashMap<String, f32> = serde_json::from_value(patient["vitals"].clone()).unwrap_or_default();
                                let ordersets: Vec<OrderSet> = serde_json::from_value(patient["ordersets"].clone()).unwrap_or_default();
                                let mar: Vec<Value> = patient["mar"]["records"].as_array().map_or(Vec::new(), |arr| arr.to_vec());
                                let labs: HashMap<String, f32> = serde_json::from_value(patient["labs"].clone()).unwrap_or_default();
                                let io_inputs: HashMap<String, Value> = serde_json::from_value(patient["io"]["inputs"].clone()).unwrap_or_default();
                                let io_outputs: HashMap<String, f32> = serde_json::from_value(patient["io"]["outputs"].clone()).unwrap_or_default();
                                let chest_xray = patient["chestXRay"].as_str().map(String::from);
                                if let Err(e) = tx.send((vitals, ordersets, mar, labs, io_inputs, io_outputs, chest_xray)).await {
                                    error!("Channel send failed: {}, skipping...", e);
                                }
                            }
                        }
                    }
                }
                Err(e) => warn!("WebSocket error: {}, retrying...", e),
            }
        }
        Ok::<(), Error>(())
    });

    let metrics_server = tokio::spawn(async move {
        async fn metrics_handler(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
            let handle = PROMETHEUS_HANDLE.get().expect("Prometheus handle not initialized");
            Ok(Response::new(Body::from(handle.render())))
        }
        let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(metrics_handler)) });
        Server::bind(&([0, 0, 0, 0], 9090).into()).serve(make_svc).await?;
        Ok::<(), Error>(())
    });

    tokio::try_join!(
        receiver,
        futures::future::join_all(workers),
        task_executor,
        alert_aggregator,
        metrics_server
    )?;
    gauge!("active_pipelines").decrement(1.0);
    gauge!("active_tasks").set(agent.task_queue.lock().await.len() as f64);
    Ok(())
}

async fn run_flood_api(agent: NoahAgent<GeminiModel>) -> Result<(), Error> {
    HttpServer::new(move || {
        ActixApp::new()
            .wrap(Logger::default())
            .wrap(actix_cors::Cors::permissive()
                .allowed_methods(vec!["GET", "POST"])
                .allowed_headers(vec![header::CONTENT_TYPE, header::ACCEPT])
                .max_age(3600))
            .route("/vitals/{patient_id}", web::get().to(get_vitals))
            .route("/labs/{patient_id}", web::get().to(get_labs))
            .route("/meds/{patient_id}", web::get().to(get_meds))
            .route("/orders/{patient_id}", web::get().to(get_orders))
            .route("/io/{patient_id}", web::get().to(get_io))
            .route("/metrics", web::get().to(metrics_handler))
            .app_data(web::Data::new(agent.clone()))
    })
    .bind(("0.0.0.0", 8081))?
    .workers(4)
    .run()
    .await?;
    Ok(())
}

async fn get_vitals(path: web::Path<String>, agent: web::Data<NoahAgent<GeminiModel>>) -> impl Responder {
    let patient_id = path.into_inner();
    match agent.monitor_vitals().await {
        Ok(vitals) => web::Json(serde_json::json!({ "patient_id": patient_id, "vitals": vitals })),
        Err(e) => web::Json(serde_json::json!({ "error": e.to_string() })).status(http::StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_labs(path: web::Path<String>, agent: web::Data<NoahAgent<GeminiModel>>) -> impl Responder {
    let patient_id = path.into_inner();
    match agent.update_labs(HashMap::from([("K".to_string(), 3.5), ("LacticAcid".to_string(), 2.5)])).await {
        Ok(labs) => web::Json(serde_json::json!({ "patient_id": patient_id, "labs": labs })),
        Err(e) => web::Json(serde_json::json!({ "error": e.to_string() })).status(http::StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_meds(path: web::Path<String>, agent: web::Data<NoahAgent<GeminiModel>>) -> impl Responder {
    let patient_id = path.into_inner();
    match agent.update_medications(vec![serde_json::json!({
        "orderId": "ORD001",
        "medication": "Levophed 0.1 mcg/kg/min",
        "administeredAt": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
    }])).await {
        Ok(_) => web::Json(serde_json::json!({ "patient_id": patient_id, "meds": "Medication records" })),
        Err(e) => web::Json(serde_json::json!({ "error": e.to_string() })).status(http::StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_orders(path: web::Path<String>, agent: web::Data<NoahAgent<GeminiModel>>) -> impl Responder {
    let patient_id = path.into_inner();
    let ordersets = agent.ordersets.lock().await.clone();
    web::Json(serde_json::json!({ "patient_id": patient_id, "orders": ordersets }))
}

async fn get_io(path: web::Path<String>, agent: web::Data<NoahAgent<GeminiModel>>) -> impl Responder {
    let patient_id = path.into_inner();
    match agent.update_io(
        HashMap::from([("ivPump".to_string(), Value::from("NS 100 mL/hr"))]),
        HashMap::from([("foley".to_string(), 50.0)])
    ).await {
        Ok(_) => web::Json(serde_json::json!({ "patient_id": patient_id, "io": "I/O records" })),
        Err(e) => web::Json(serde_json::json!({ "error": e.to_string() })).status(http::StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn metrics_handler() -> impl Responder {
    let handle = PROMETHEUS_HANDLE.get().expect("Prometheus handle not initialized");
    web::HttpResponse::Ok().content_type("text/plain").body(handle.render())
}

fn profile<F, R>(f: F) -> R where F: FnOnce() -> R {
    let guard = ProfilerGuard::new(100).unwrap();
    let result = f();
    if let Ok(report) = guard.report().build() {
        let mut file = std::fs::File::create("flamegraph.svg").unwrap();
        report.flamegraph(&mut file).unwrap();
    }
    result
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::FULL)
        .init();

    let cli = Cli::parse();
    let patient_id = match &cli.command {
        Commands::MonitorVitals { patient_id } => patient_id,
        Commands::MonitorVitalsStream { patient_id, .. } => patient_id,
        Commands::LaunchFlood => "Seymore Butts",
        Commands::ChartVitals { patient_id, .. } => patient_id,
        Commands::ManageOrders { patient_id } => patient_id,
        Commands::AnalyzeLabs { patient_id } => patient_id,
        Commands::ChartLabs { patient_id } => patient_id,
        Commands::ManageMAR { patient_id } => patient_id,
        Commands::MonitorBlockchain { patient_id } => patient_id,
        #[cfg(feature = "clickhouse")] Commands::TrendVitals { patient_id, .. } => patient_id,
        #[cfg(feature = "clickhouse")] Commands::TrendLabs { patient_id, .. } => patient_id,
        #[cfg(feature = "clickhouse")] Commands::TrendMeds { patient_id, .. } => patient_id,
        #[cfg(feature = "clickhouse")] Commands::TrendIO { patient_id, .. } => patient_id,
    }.to_string();

    let chart = Arc::new(PatientChart {
        patient_id: patient_id.clone(),
        name: patient_id,
        age: 65,
        sex: "M".to_string(),
        diagnosis: "Unknown".to_string(),
        room: "ICU-101".to_string(),
        ventilated: false,
    });

    let agent = NoahAgent::new(
        chart,
        vec!["Default training data for Noah".to_string()],
        GeminiModel,
        &cli.db_type,
        if cli.db_type == "sqlite" { "/app/noah.db" } else { "clickhouse://localhost:8123" },
        match cli.mode.as_str() { "nurse" => AgentMode::Nurse, "patient" => AgentMode::Patient, _ => AgentMode::Nurse },
        cli.voice,
    ).await?;

    let run_command = |command: Commands| async move {
        match command {
            Commands::MonitorVitals { .. } => {
                agent.add_task("Check vitals", 5).await?;
                let task = agent.prioritize_tasks().await?;
                info!("Executing task: {}", agent.execute_task(&task).await?);
                info!("Vitals: {}", agent.monitor_vitals().await?);
                info!("{}", agent.monitor_ehr().await?);
            }
            Commands::MonitorVitalsStream { patient_id, .. } => {
                agent.add_task("Check vitals", 5).await?;
                run_vitals_pipeline(patient_id, cli.worker_count, cli.buffer_size, agent.clone()).await?;
            }
            Commands::LaunchFlood => {
                tokio::try_join!(
                    run_vitals_pipeline("Seymore Butts".to_string(), cli.worker_count, cli.buffer_size, agent.clone()),
                    run_flood_api(agent.clone())
                )?;
            }
            Commands::ChartVitals { patient_id, interval } => {
                agent.add_task("Chart vitals", 5).await?;
                run_vitals_pipeline(patient_id, cli.worker_count, cli.buffer_size, agent.clone()).await?;
            }
            Commands::ManageOrders { patient_id } => {
                let ordersets = agent.ordersets.lock().await.clone();
                info!("Managing orders for {}: {:?}", patient_id, ordersets);
                agent.add_task("Manage orders", 5).await?;
            }
            Commands::AnalyzeLabs { patient_id } => {
                let labs = HashMap::from([("K".to_string(), 3.5), ("LacticAcid".to_string(), 2.5)]);
                info!("Analyzing labs for {}: {:?}", patient_id, labs);
                agent.update_labs(labs).await?;
                agent.add_task("Analyze labs", 5).await?;
            }
            Commands::ChartLabs { patient_id } => {
                let labs = HashMap::from([("K".to_string(), 3.5), ("LacticAcid".to_string(), 2.5)]);
                info!("Charting labs for {}: {:?}", patient_id, labs);
                agent.update_labs(labs).await?;
                agent.add_task("Chart labs", 5).await?;
            }
            Commands::ManageMAR { patient_id } => {
                let mar = vec![serde_json::json!({"orderId": "ORD001", "medication": "Levophed 0.1 mcg/kg/min", "administeredAt": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()})];
                info!("Managing MAR for {}: {:?}", patient_id, mar);
                agent.update_medications(mar).await?;
                agent.add_task("Manage MAR", 5).await?;
            }
            Commands::MonitorBlockchain { patient_id } => {
                info!("Monitoring blockchain for {}: {}", patient_id, agent.monitor_ehr().await?);
                agent.add_task("Monitor blockchain", 5).await?;
            }
            #[cfg(feature = "clickhouse")]
            Commands::TrendVitals { patient_id, key, hours } => {
                let hours = hours.unwrap_or(24);
                let trend = agent.get_vitals_trend(&key, hours).await?;
                info!("Vitals trend for {} ({} over {} hours): {:?}", patient_id, key, hours, trend);
            }
            #[cfg(feature = "clickhouse")]
            Commands::TrendLabs { patient_id, key, hours } => {
                let hours = hours.unwrap_or(24);
                let trend = agent.get_labs_trend(&key, hours).await?;
                info!("Labs trend for {} ({} over {} hours): {:?}", patient_id, key, hours, trend);
            }
            #[cfg(feature = "clickhouse")]
            Commands::TrendMeds { patient_id, medication, hours } => {
                let hours = hours.unwrap_or(24);
                let trend = agent.get_meds_trend(&medication, hours).await?;
                info!("Meds trend for {} ({} over {} hours): {:?}", patient_id, medication, hours, trend);
            }
            #[cfg(feature = "clickhouse")]
            Commands::TrendIO { patient_id, key, hours } => {
                let hours = hours.unwrap_or(24);
                let trend = agent.get_io_trend(&key, hours).await?;
                info!("I/O trend for {} ({} over {} hours): {:?}", patient_id, key, hours, trend);
            }
        }
        Ok::<(), Error>(())
    };

    if cli.profile {
        profile(|| {
            run_command(cli.command).block_on().unwrap_or_else(|e| error!("Command failed: {}", e));
        });
    } else {
        run_command(cli.command).await?;
    }

    Ok(())
}

mod arc_framework {
    use sha2::{Sha256, Digest};
    use anyhow::{Result, Error};
    use tracing::{info, error};
    use tokio::sync::mpsc;

    pub async fn log_to_blockchain(action: &str, patient_id: &str, data: &str) -> Result<String, Error> {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());
        info!("Logged to ARC blockchain: {} for {} with hash {}", action, patient_id, hash);
        Ok(hash)
    }

    pub async fn query_blockchain(patient_id: &str, action: &str) -> Result<Vec<String>, Error> {
        Ok(vec![format!("Query result for {}: {}", patient_id, action)])
    }

    pub async fn batch_log_to_blockchain(events: Vec<(&str, &str, &str)>) -> Result<Vec<String>, Error> {
        let (tx, mut rx) = mpsc::channel(events.len());
        for (action, patient_id, data) in events {
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = tx.send(log_to_blockchain(action, patient_id, data).await).await {
                    error!("Failed to send batch log: {}", e);
                }
            });
        }
        drop(tx);

        let mut hashes = Vec::new();
        while let Some(result) = rx.recv().await {
            hashes.push(result?);
        }
        Ok(hashes)
    }
}

mod tools {
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
            Ok("HR: 72, SpO2: 98, MAP: 93, RR: 18, Temp: 38.2".to_string())
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
            Ok(format!("Labs for {}: K: 3.5, LacticAcid: 2.5".to_string()))
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