use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use anyhow::{Result, anyhow, Context};
use bb8_redis::bb8::Pool as RedisPool;
use bb8_redis::RedisConnectionManager;
use sqlx::{SqlitePool, Pool, Sqlite, Transaction};
use uuid::Uuid;
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc, NaiveDateTime};
use tracing::{info, error, warn, debug, instrument};
use dashmap::DashMap;

// Constants
const REDIS_TTL: u64 = 300; // 5 minutes cache TTL

// ===== Data Models =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patient {
    pub mrn: String,
    pub first_name: String,
    pub last_name: String,
    pub date_of_birth: NaiveDateTime,
    pub sex: String,
    pub height_cm: Option<f32>,
    pub weight_kg: Option<f32>,
    pub allergies: Vec<Allergy>,
    pub code_status: String, // e.g., "Full Code", "DNR", "DNI"
    pub isolation_status: Option<String>,
    pub admission_date: DateTime<Utc>,
    pub attending_provider: String,
    pub primary_diagnosis: String,
    pub secondary_diagnoses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Allergy {
    pub substance: String,
    pub reaction: String,
    pub severity: String, // Mild, Moderate, Severe
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VitalSigns {
    pub patient_id: String,
    pub timestamp: DateTime<Utc>,
    pub heart_rate: Option<f32>,
    pub respiratory_rate: Option<f32>,
    pub blood_pressure_systolic: Option<f32>,
    pub blood_pressure_diastolic: Option<f32>,
    pub mean_arterial_pressure: Option<f32>,
    pub temperature_celsius: Option<f32>,
    pub oxygen_saturation: Option<f32>,
    pub end_tidal_co2: Option<f32>,
    pub pain_score: Option<i32>,
    pub glasgow_coma_scale: Option<i32>,
    pub recorded_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabResult {
    pub patient_id: String,
    pub order_id: String,
    pub test_name: String,
    pub value: f64,
    pub unit: String,
    pub reference_range_low: Option<f64>,
    pub reference_range_high: Option<f64>,
    pub critical: bool,
    pub status: String, // Preliminary, Final
    pub collected_at: DateTime<Utc>,
    pub resulted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Medication {
    pub patient_id: String,
    pub order_id: String,
    pub name: String,
    pub dose: String,
    pub route: String,
    pub frequency: String,
    pub start_date: DateTime<Utc>,
    pub end_date: Option<DateTime<Utc>>,
    pub ordering_provider: String,
    pub status: String, // Active, Discontinued, Completed
    pub pharmacy_status: String, // Verified, Dispensed
    pub last_administered: Option<DateTime<Utc>>,
    pub medication_class: String,
    pub is_high_alert: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicationAdministration {
    pub patient_id: String,
    pub medication_id: String,
    pub administered_at: DateTime<Utc>,
    pub administered_by: String,
    pub dose_given: String,
    pub site: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FluidIntakeOutput {
    pub patient_id: String,
    pub timestamp: DateTime<Utc>,
    pub recorded_by: String,
    pub intake: HashMap<String, f64>, // e.g., "IV NS" -> 100.0
    pub output: HashMap<String, f64>, // e.g., "Urine" -> 50.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VentilatorSettings {
    pub patient_id: String,
    pub timestamp: DateTime<Utc>,
    pub mode: String, // e.g., "AC/VC", "SIMV", "PRVC"
    pub fio2: f32,
    pub peep: f32,
    pub respiratory_rate: f32,
    pub tidal_volume: Option<f32>,
    pub pressure_support: Option<f32>,
    pub inspiratory_pressure: Option<f32>,
    pub i_time: Option<f32>,
    pub recorded_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assessment {
    pub patient_id: String,
    pub timestamp: DateTime<Utc>,
    pub assessment_type: String, // e.g., "Neurological", "Respiratory", "Cardiovascular"
    pub findings: HashMap<String, String>,
    pub notes: Option<String>,
    pub performed_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClinicalNote {
    pub patient_id: String,
    pub note_id: String,
    pub note_type: String, // e.g., "Progress Note", "Nursing Note", "Procedure Note"
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub cosigned_by: Option<String>,
    pub cosigned_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub patient_id: String,
    pub order_id: String,
    pub order_type: String, // e.g., "Medication", "Lab", "Imaging", "Nursing"
    pub details: String,
    pub ordered_at: DateTime<Utc>,
    pub ordered_by: String,
    pub status: String, // e.g., "Active", "Completed", "Discontinued"
    pub start_date: DateTime<Utc>,
    pub end_date: Option<DateTime<Utc>>,
    pub frequency: Option<String>,
    pub priority: String, // e.g., "Routine", "Urgent", "STAT"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarePlan {
    pub patient_id: String,
    pub problems: Vec<Problem>,
    pub goals: Vec<Goal>,
    pub interventions: Vec<Intervention>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Problem {
    pub id: String,
    pub description: String,
    pub status: String, // Active, Resolved
    pub onset_date: Option<DateTime<Utc>>,
    pub resolution_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub description: String,
    pub target_date: Option<DateTime<Utc>>,
    pub status: String, // Not Met, Partially Met, Met
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intervention {
    pub id: String,
    pub description: String,
    pub frequency: String,
    pub responsible_role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub patient_id: String,
    pub alert_id: String,
    pub alert_type: String, // e.g., "Clinical", "Medication", "Lab"
    pub severity: String,   // e.g., "Critical", "High", "Medium", "Low"
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub acknowledged: bool,
    pub acknowledged_by: Option<String>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

// Blockchain integration
struct BlockchainExecutor {
    // In a real implementation, this would contain connection details
    // and authentication for the blockchain network
}

impl BlockchainExecutor {
    fn new() -> Self {
        Self {}
    }
    
    async fn log_to_blockchain(&self, action: &str, patient_id: &str, data: &str) -> Result<String> {
        // This is a mock implementation
        // In a real system, this would interact with a blockchain network
        let hash = format!("hash_{}_{}_{}", patient_id, action, Uuid::new_v4());
        info!("Blockchain log: {} for patient {}", action, patient_id);
        Ok(hash)
    }
    
    async fn query_blockchain(&self, patient_id: &str, query_type: &str) -> Result<Vec<String>> {
        // Mock implementation
        info!("Blockchain query: {} for patient {}", query_type, patient_id);
        Ok(vec![format!("Event for patient {}", patient_id)])
    }
}

// Database backend enum
#[derive(Clone)]
pub enum DatabaseBackend {
    SQLite(Pool<Sqlite>),
    #[cfg(feature = "clickhouse")]
    ClickHouse(clickhouse::Client),
}

// Main EHR Database struct
pub struct EHRDatabase {
    db: Arc<DatabaseBackend>,
    redis_pool: RedisPool<RedisConnectionManager>,
    blockchain_executor: BlockchainExecutor,
    cache: Arc<DashMap<String, serde_json::Value>>,
}

impl EHRDatabase {
    pub async fn new(db_type: &str, db_url: &str, redis_url: &str) -> Result<Self> {
        // Initialize database connection
        let db = match db_type {
            "sqlite" => {
                let pool = SqlitePool::connect(db_url).await?;
                
                // Initialize tables if they don't exist
                Self::initialize_sqlite_schema(&pool).await?;
                
                Arc::new(DatabaseBackend::SQLite(pool))
            }
            #[cfg(feature = "clickhouse")]
            "clickhouse" => {
                let client = clickhouse::Client::default()
                    .with_url(db_url)
                    .with_database("ehr")
                    .with_user("default")
                    .with_password("");
                
                // Initialize ClickHouse tables
                Self::initialize_clickhouse_schema(&client).await?;
                
                Arc::new(DatabaseBackend::ClickHouse(client))
            }
            _ => return Err(anyhow!("Unsupported database type: {}", db_type)),
        };
        
        // Initialize Redis connection
        let manager = RedisConnectionManager::new(redis_url)?;
        let redis_pool = RedisPool::builder()
            .max_size(15)
            .build(manager)
            .await?;
        
        // Create blockchain executor
        let blockchain_executor = BlockchainExecutor::new();
        
        // Create in-memory cache
        let cache = Arc::new(DashMap::new());
        
        Ok(Self {
            db,
            redis_pool,
            blockchain_executor,
            cache,
        })
    }
    
    // Initialize SQLite schema
    async fn initialize_sqlite_schema(pool: &SqlitePool) -> Result<()> {
        // Patients table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS patients (
                mrn TEXT PRIMARY KEY,
                first_name TEXT NOT NULL,
                last_name TEXT NOT NULL,
                date_of_birth INTEGER NOT NULL,
                sex TEXT NOT NULL,
                height_cm REAL,
                weight_kg REAL,
                code_status TEXT NOT NULL,
                isolation_status TEXT,
                admission_date INTEGER NOT NULL,
                attending_provider TEXT NOT NULL,
                primary_diagnosis TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )"
        )
        .execute(pool)
        .await?;
        
        // Allergies table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS allergies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                substance TEXT NOT NULL,
                reaction TEXT NOT NULL,
                severity TEXT NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Secondary diagnoses table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS secondary_diagnoses (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                diagnosis TEXT NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Vital signs table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS vital_signs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                heart_rate REAL,
                respiratory_rate REAL,
                blood_pressure_systolic REAL,
                blood_pressure_diastolic REAL,
                mean_arterial_pressure REAL,
                temperature_celsius REAL,
                oxygen_saturation REAL,
                end_tidal_co2 REAL,
                pain_score INTEGER,
                glasgow_coma_scale INTEGER,
                recorded_by TEXT NOT NULL,
                event_id TEXT NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Lab results table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS lab_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                order_id TEXT NOT NULL,
                test_name TEXT NOT NULL,
                value REAL NOT NULL,
                unit TEXT NOT NULL,
                reference_range_low REAL,
                reference_range_high REAL,
                critical BOOLEAN NOT NULL,
                status TEXT NOT NULL,
                collected_at INTEGER NOT NULL,
                resulted_at INTEGER NOT NULL,
                event_id TEXT NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Medications table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS medications (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                order_id TEXT NOT NULL,
                name TEXT NOT NULL,
                dose TEXT NOT NULL,
                route TEXT NOT NULL,
                frequency TEXT NOT NULL,
                start_date INTEGER NOT NULL,
                end_date INTEGER,
                ordering_provider TEXT NOT NULL,
                status TEXT NOT NULL,
                pharmacy_status TEXT NOT NULL,
                last_administered INTEGER,
                medication_class TEXT NOT NULL,
                is_high_alert BOOLEAN NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Medication administrations table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS medication_administrations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                medication_id TEXT NOT NULL,
                administered_at INTEGER NOT NULL,
                administered_by TEXT NOT NULL,
                dose_given TEXT NOT NULL,
                site TEXT,
                notes TEXT,
                event_id TEXT NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Fluid intake/output table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS fluid_io (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                recorded_by TEXT NOT NULL,
                io_type TEXT NOT NULL,
                category TEXT NOT NULL,
                amount REAL NOT NULL,
                event_id TEXT NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Ventilator settings table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS ventilator_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                mode TEXT NOT NULL,
                fio2 REAL NOT NULL,
                peep REAL NOT NULL,
                respiratory_rate REAL NOT NULL,
                tidal_volume REAL,
                pressure_support REAL,
                inspiratory_pressure REAL,
                i_time REAL,
                recorded_by TEXT NOT NULL,
                event_id TEXT NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Assessments table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS assessments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                assessment_type TEXT NOT NULL,
                notes TEXT,
                performed_by TEXT NOT NULL,
                event_id TEXT NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Assessment findings table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS assessment_findings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                assessment_id INTEGER NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                FOREIGN KEY (assessment_id) REFERENCES assessments(id)
            )"
        )
        .execute(pool)
        .await?;
        
        // Clinical notes table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS clinical_notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                note_id TEXT NOT NULL,
                note_type TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                created_by TEXT NOT NULL,
                cosigned_by TEXT,
                cosigned_at INTEGER,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Orders table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS orders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                order_id TEXT NOT NULL,
                order_type TEXT NOT NULL,
                details TEXT NOT NULL,
                ordered_at INTEGER NOT NULL,
                ordered_by TEXT NOT NULL,
                status TEXT NOT NULL,
                start_date INTEGER NOT NULL,
                end_date INTEGER,
                frequency TEXT,
                priority TEXT NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Care plans table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS care_plans (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                updated_by TEXT NOT NULL,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        // Problems table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS problems (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                care_plan_id INTEGER NOT NULL,
                problem_id TEXT NOT NULL,
                description TEXT NOT NULL,
                status TEXT NOT NULL,
                onset_date INTEGER,
                resolution_date INTEGER,
                FOREIGN KEY (care_plan_id) REFERENCES care_plans(id)
            )"
        )
        .execute(pool)
        .await?;
        
        // Goals table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS goals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                care_plan_id INTEGER NOT NULL,
                goal_id TEXT NOT NULL,
                description TEXT NOT NULL,
                target_date INTEGER,
                status TEXT NOT NULL,
                FOREIGN KEY (care_plan_id) REFERENCES care_plans(id)
            )"
        )
        .execute(pool)
        .await?;
        
        // Interventions table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS interventions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                care_plan_id INTEGER NOT NULL,
                intervention_id TEXT NOT NULL,
                description TEXT NOT NULL,
                frequency TEXT NOT NULL,
                responsible_role TEXT NOT NULL,
                FOREIGN KEY (care_plan_id) REFERENCES care_plans(id)
            )"
        )
        .execute(pool)
        .await?;
        
        // Alerts table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS alerts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                patient_id TEXT NOT NULL,
                alert_id TEXT NOT NULL,
                alert_type TEXT NOT NULL,
                severity TEXT NOT NULL,
                message TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                acknowledged BOOLEAN NOT NULL,
                acknowledged_by TEXT,
                acknowledged_at INTEGER,
                FOREIGN KEY (patient_id) REFERENCES patients(mrn)
            )"
        )
        .execute(pool)
        .await?;
        
        Ok(())
    }
    
    #[cfg(feature = "clickhouse")]
    async fn initialize_clickhouse_schema(client: &clickhouse::Client) -> Result<()> {
        // Implementation for ClickHouse would go here
        // This is a placeholder for future implementation
        Ok(())
    }
    
    // ===== Patient Management =====
    
    #[instrument(skip(self, patient), fields(mrn = %patient.mrn))]
    pub async fn create_patient(&self, patient: Patient) -> Result<()> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
                
                let mut tx = pool.begin().await?;
                
                // Insert patient
                sqlx::query(
                    "INSERT INTO patients (
                        mrn, first_name, last_name, date_of_birth, sex,
                        height_cm, weight_kg, code_status, isolation_status,
                        admission_date, attending_provider, primary_diagnosis,
                        created_at, updated_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&patient.mrn)
                .bind(&patient.first_name)
                .bind(&patient.last_name)
                .bind(patient.date_of_birth.timestamp())
                .bind(&patient.sex)
                .bind(patient.height_cm)
                .bind(patient.weight_kg)
                .bind(&patient.code_status)
                .bind(&patient.isolation_status)
                .bind(patient.admission_date.timestamp())
                .bind(&patient.attending_provider)
                .bind(&patient.primary_diagnosis)
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                
                // Insert allergies
                for allergy in &patient.allergies {
                    sqlx::query(
                        "INSERT INTO allergies (patient_id, substance, reaction, severity) 
                         VALUES (?, ?, ?, ?)"
                    )
                    .bind(&patient.mrn)
                    .bind(&allergy.substance)
                    .bind(&allergy.reaction)
                    .bind(&allergy.severity)
                    .execute(&mut *tx)
                    .await?;
                }
                
                // Insert secondary diagnoses
                for diagnosis in &patient.secondary_diagnoses {
                    sqlx::query(
                        "INSERT INTO secondary_diagnoses (patient_id, diagnosis) 
                         VALUES (?, ?)"
                    )
                    .bind(&patient.mrn)
                    .bind(diagnosis)
                    .execute(&mut *tx)
                    .await?;
                }
                
                tx.commit().await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&patient)?;
                self.blockchain_executor.log_to_blockchain("Create Patient", &patient.mrn, &data).await?;
                
                info!("Patient created: {}", patient.mrn);
                Ok(())
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                // Implementation for ClickHouse would go here
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }

    #[instrument(skip(self), fields(mrn = %mrn))]
    pub async fn get_patient(&self, mrn: &str) -> Result<Option<Patient>> {
        // Try cache first
        let cache_key = format!("patient_{}", mrn);
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(Some(serde_json::from_value(cached.value().clone())?));
        }
        
        // Try Redis next
        let mut conn = self.redis_pool.get().await?;
        if let Ok(Some(data)) = redis::cmd("GET").arg(&cache_key).query_async::<_, Option<String>>(&mut *conn).await {
            let patient: Patient = serde_json::from_str(&data)?;
            self.cache.insert(cache_key, serde_json::to_value(&patient)?);
            return Ok(Some(patient));
        }
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                // Get patient basic info
                let patient_row = sqlx::query!(
                    "SELECT * FROM patients WHERE mrn = ?",
                    mrn
                )
                .fetch_optional(pool)
                .await?;
                
                if let Some(row) = patient_row {
                    // Get allergies
                    let allergies = sqlx::query!(
                        "SELECT substance, reaction, severity FROM allergies WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| Allergy {
                        substance: r.substance,
                        reaction: r.reaction,
                        severity: r.severity,
                    })
                    .collect();
                    
                    // Get secondary diagnoses
                    let secondary_diagnoses = sqlx::query!(
                        "SELECT diagnosis FROM secondary_diagnoses WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| r.diagnosis)
                    .collect();
                    
                    // Get vital signs
                    let vital_signs = sqlx::query!(
                        "SELECT * FROM vital_signs WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| VitalSigns {
                        patient_id: r.patient_id,
                        timestamp: r.timestamp.into(),
                        heart_rate: r.heart_rate,
                        respiratory_rate: r.respiratory_rate,
                        blood_pressure_systolic: r.blood_pressure_systolic,
                        blood_pressure_diastolic: r.blood_pressure_diastolic,
                        mean_arterial_pressure: r.mean_arterial_pressure,
                        temperature_celsius: r.temperature_celsius,
                        oxygen_saturation: r.oxygen_saturation,
                        end_tidal_co2: r.end_tidal_co2,
                        pain_score: r.pain_score,
                        glasgow_coma_scale: r.glasgow_coma_scale,
                        recorded_by: r.recorded_by,
                    })
                    .collect();
                    
                    // Get lab results
                    let lab_results = sqlx::query!(
                        "SELECT * FROM lab_results WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| LabResult {
                        patient_id: r.patient_id,
                        order_id: r.order_id,
                        test_name: r.test_name,
                        value: r.value,
                        unit: r.unit,
                        reference_range_low: r.reference_range_low,
                        reference_range_high: r.reference_range_high,
                        critical: r.critical,
                        status: r.status,
                        collected_at: r.collected_at.into(),
                        resulted_at: r.resulted_at.into(),
                    })
                    .collect();
                    
                    // Get medications
                    let medications = sqlx::query!(
                        "SELECT * FROM medications WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| Medication {
                        patient_id: r.patient_id,
                        order_id: r.order_id,
                        name: r.name,
                        dose: r.dose,
                        route: r.route,
                        frequency: r.frequency,
                        start_date: r.start_date.into(),
                        end_date: r.end_date.map(|d| d.into()),
                        ordering_provider: r.ordering_provider,
                        status: r.status,
                        pharmacy_status: r.pharmacy_status,
                        last_administered: r.last_administered.map(|d| d.into()),
                        medication_class: r.medication_class,
                        is_high_alert: r.is_high_alert,
                    })
                    .collect();
                    
                    // Get medication administrations
                    let medication_administrations = sqlx::query!(
                        "SELECT * FROM medication_administrations WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| MedicationAdministration {
                        patient_id: r.patient_id,
                        medication_id: r.medication_id,
                        administered_at: r.administered_at.into(),
                        administered_by: r.administered_by,
                        dose_given: r.dose_given,
                        site: r.site,
                        notes: r.notes,
                    })
                    .collect();
                    
                    // Get fluid intake/output
                    let fluid_io = sqlx::query!(
                        "SELECT * FROM fluid_io WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| FluidIntakeOutput {
                        patient_id: r.patient_id,
                        timestamp: r.timestamp.into(),
                        recorded_by: r.recorded_by,
                        intake: serde_json::from_str(&r.intake)?,
                        output: serde_json::from_str(&r.output)?,
                    })
                    .collect();
                    
                    // Get ventilator settings
                    let ventilator_settings = sqlx::query!(
                        "SELECT * FROM ventilator_settings WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| VentilatorSettings {
                        patient_id: r.patient_id,
                        timestamp: r.timestamp.into(),
                        mode: r.mode,
                        fio2: r.fio2,
                        peep: r.peep,
                        respiratory_rate: r.respiratory_rate,
                        tidal_volume: r.tidal_volume,
                        pressure_support: r.pressure_support,
                        inspiratory_pressure: r.inspiratory_pressure,
                        i_time: r.i_time,
                        recorded_by: r.recorded_by,
                    })
                    .collect();
                    
                    // Get assessments
                    let assessments = sqlx::query!(
                        "SELECT * FROM assessments WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| Assessment {
                        patient_id: r.patient_id,
                        timestamp: r.timestamp.into(),
                        assessment_type: r.assessment_type,
                        findings: serde_json::from_str(&r.findings)?,
                        notes: r.notes,
                        performed_by: r.performed_by,
                    })
                    .collect();
                    
                    // Get clinical notes
                    let clinical_notes = sqlx::query!(
                        "SELECT * FROM clinical_notes WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| ClinicalNote {
                        patient_id: r.patient_id,
                        note_id: r.note_id,
                        note_type: r.note_type,
                        content: r.content,
                        created_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.created_at, 0)
                                .ok_or_else(|| anyhow!("Invalid created_at timestamp"))?,
                            Utc,
                        ),
                        created_by: r.created_by,
                        cosigned_by: r.cosigned_by,
                        cosigned_at: r.cosigned_at.map(|d| d.into()),
                    })
                    .collect();
                    
                    // Get orders
                    let orders = sqlx::query!(
                        "SELECT * FROM orders WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| Order {
                        patient_id: r.patient_id,
                        order_id: r.order_id,
                        order_type: r.order_type,
                        details: r.details,
                        ordered_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.ordered_at, 0)
                                .ok_or_else(|| anyhow!("Invalid ordered_at timestamp"))?,
                            Utc,
                        ),
                        ordered_by: r.ordered_by,
                        status: r.status,
                        start_date: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.start_date, 0)
                                .ok_or_else(|| anyhow!("Invalid start_date timestamp"))?,
                            Utc,
                        ),
                        end_date: if let Some(end_date) = row.end_date {
                            Some(DateTime::<Utc>::from_utc(
                                NaiveDateTime::from_timestamp_opt(end_date, 0)
                                    .ok_or_else(|| anyhow!("Invalid end_date timestamp"))?,
                                Utc,
                            ))
                        } else {
                            None
                        },
                        frequency: r.frequency,
                        priority: r.priority,
                    })
                    .collect();
                    
                    // Get care plans
                    let care_plans = sqlx::query!(
                        "SELECT * FROM care_plans WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| CarePlan {
                        patient_id: r.patient_id,
                        problems: serde_json::from_str(&r.problems)?,
                        goals: serde_json::from_str(&r.goals)?,
                        interventions: serde_json::from_str(&r.interventions)?,
                        updated_at: r.updated_at.into(),
                        updated_by: r.updated_by,
                    })
                    .collect();
                    
                    // Get problems
                    let problems = sqlx::query!(
                        "SELECT * FROM problems WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| Problem {
                        id: r.problem_id,
                        description: r.description,
                        status: r.status,
                        onset_date: r.onset_date.map(|d| d.into()),
                        resolution_date: r.resolution_date.map(|d| d.into()),
                    })
                    .collect();
                    
                    // Get goals
                    let goals = sqlx::query!(
                        "SELECT * FROM goals WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| Goal {
                        id: r.goal_id,
                        description: r.description,
                        target_date: r.target_date.map(|d| d.into()),
                        status: r.status,
                    })
                    .collect();
                    
                    // Get interventions
                    let interventions = sqlx::query!(
                        "SELECT * FROM interventions WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| Intervention {
                        id: r.intervention_id,
                        description: r.description,
                        frequency: r.frequency,
                        responsible_role: r.responsible_role,
                    })
                    .collect();
                    
                    // Get alerts
                    let alerts = sqlx::query!(
                        "SELECT * FROM alerts WHERE patient_id = ?",
                        mrn
                    )
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(|r| Alert {
                        patient_id: r.patient_id,
                        alert_id: r.alert_id,
                        alert_type: r.alert_type,
                        severity: r.severity,
                        message: r.message,
                        created_at: r.created_at.into(),
                        acknowledged: r.acknowledged,
                        acknowledged_by: r.acknowledged_by,
                        acknowledged_at: r.acknowledged_at.map(|d| d.into()),
                    })
                    .collect();
                    
                    let patient = Patient {
                        mrn: row.mrn,
                        first_name: row.first_name,
                        last_name: row.last_name,
                        date_of_birth: NaiveDateTime::from_timestamp_opt(row.date_of_birth, 0)
                            .ok_or_else(|| anyhow!("Invalid date of birth timestamp"))?,
                        sex: row.sex,
                        height_cm: row.height_cm,
                        weight_kg: row.weight_kg,
                        allergies,
                        code_status: row.code_status,
                        isolation_status: row.isolation_status,
                        admission_date: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.admission_date, 0)
                                .ok_or_else(|| anyhow!("Invalid admission date timestamp"))?,
                            Utc,
                        ),
                        attending_provider: row.attending_provider,
                        primary_diagnosis: row.primary_diagnosis,
                        secondary_diagnoses,
                    };
                    
                    // Cache in Redis
                    let json = serde_json::to_string(&patient)?;
                    redis::cmd("SETEX")
                        .arg(&cache_key)
                        .arg(REDIS_TTL)
                        .arg(&json)
                        .execute_async(&mut *conn)
                        .await?;
                    
                    // Cache in memory
                    self.cache.insert(cache_key, serde_json::to_value(&patient)?);
                    
                    Ok(Some(patient))
                } else {
                    Ok(None)
                }
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                // Implementation for ClickHouse would go here
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }

    // ===== Vital Signs =====

    #[instrument(skip(self, vitals), fields(patient_id = %vitals.patient_id))]
    pub async fn record_vital_signs(&self, vitals: VitalSigns) -> Result<String> {
        let event_id = Uuid::new_v4().to_string();
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                sqlx::query(
                    "INSERT INTO vital_signs (
                        patient_id, timestamp, heart_rate, respiratory_rate,
                        blood_pressure_systolic, blood_pressure_diastolic,
                        mean_arterial_pressure, temperature_celsius,
                        oxygen_saturation, end_tidal_co2, pain_score,
                        glasgow_coma_scale, recorded_by, event_id
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&vitals.patient_id)
                .bind(vitals.timestamp.timestamp())
                .bind(vitals.heart_rate)
                .bind(vitals.respiratory_rate)
                .bind(vitals.blood_pressure_systolic)
                .bind(vitals.blood_pressure_diastolic)
                .bind(vitals.mean_arterial_pressure)
                .bind(vitals.temperature_celsius)
                .bind(vitals.oxygen_saturation)
                .bind(vitals.end_tidal_co2)
                .bind(vitals.pain_score)
                .bind(vitals.glasgow_coma_scale)
                .bind(&vitals.recorded_by)
                .bind(&event_id)
                .execute(pool)
                .await?;
                
                // Cache the vitals
                let cache_key = format!("vitals_{}_{}", vitals.patient_id, vitals.timestamp.timestamp());
                let mut conn = self.redis_pool.get().await?;
                redis::cmd("SETEX")
                    .arg(&cache_key)
                    .arg(REDIS_TTL)
                    .arg(serde_json::to_string(&vitals)?)
                    .execute_async(&mut *conn)
                    .await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&vitals)?;
                self.blockchain_executor.log_to_blockchain("Record Vitals", &vitals.patient_id, &data).await?;
                
                // Check for critical values and create alerts if needed
                self.check_vital_signs_alerts(&vitals).await?;
                
                Ok(event_id)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    async fn check_vital_signs_alerts(&self, vitals: &VitalSigns) -> Result<()> {
        // Check for critical vital signs and create alerts
        
        // Check heart rate
        if let Some(hr) = vitals.heart_rate {
            if hr > 150.0 {
                self.create_alert(Alert {
                    patient_id: vitals.patient_id.clone(),
                    alert_id: Uuid::new_v4().to_string(),
                    alert_type: "Clinical".to_string(),
                    severity: "Critical".to_string(),
                    message: format!("Critical high heart rate: {} bpm", hr),
                    created_at: Utc::now(),
                    acknowledged: false,
                    acknowledged_by: None,
                    acknowledged_at: None,
                }).await?;
            } else if hr < 40.0 {
                self.create_alert(Alert {
                    patient_id: vitals.patient_id.clone(),
                    alert_id: Uuid::new_v4().to_string(),
                    alert_type: "Clinical".to_string(),
                    severity: "Critical".to_string(),
                    message: format!("Critical low heart rate: {} bpm", hr),
                    created_at: Utc::now(),
                    acknowledged: false,
                    acknowledged_by: None,
                    acknowledged_at: None,
                }).await?;
            }
        }
        
        // Check blood pressure
        if let (Some(sbp), Some(dbp)) = (vitals.blood_pressure_systolic, vitals.blood_pressure_diastolic) {
            if sbp > 180.0 || dbp > 120.0 {
                self.create_alert(Alert {
                    patient_id: vitals.patient_id.clone(),
                    alert_id: Uuid::new_v4().to_string(),
                    alert_type: "Clinical".to_string(),
                    severity: "Critical".to_string(),
                    message: format!("Critical high blood pressure: {}/{} mmHg", sbp, dbp),
                    created_at: Utc::now(),
                    acknowledged: false,
                    acknowledged_by: None,
                    acknowledged_at: None,
                }).await?;
            } else if sbp < 90.0 {
                self.create_alert(Alert {
                    patient_id: vitals.patient_id.clone(),
                    alert_id: Uuid::new_v4().to_string(),
                    alert_type: "Clinical".to_string(),
                    severity: "Critical".to_string(),
                    message: format!("Critical low blood pressure: {}/{} mmHg", sbp, dbp),
                    created_at: Utc::now(),
                    acknowledged: false,
                    acknowledged_by: None,
                    acknowledged_at: None,
                }).await?;
            }
        }
        
        // Check oxygen saturation
        if let Some(spo2) = vitals.oxygen_saturation {
            if spo2 < 90.0 {
                self.create_alert(Alert {
                    patient_id: vitals.patient_id.clone(),
                    alert_id: Uuid::new_v4().to_string(),
                    alert_type: "Clinical".to_string(),
                    severity: "Critical".to_string(),
                    message: format!("Critical low oxygen saturation: {}%", spo2),
                    created_at: Utc::now(),
                    acknowledged: false,
                    acknowledged_by: None,
                    acknowledged_at: None,
                }).await?;
            }
        }
        
        // Check respiratory rate
        if let Some(rr) = vitals.respiratory_rate {
            if rr > 30.0 {
                self.create_alert(Alert {
                    patient_id: vitals.patient_id.clone(),
                    alert_id: Uuid::new_v4().to_string(),
                    alert_type: "Clinical".to_string(),
                    severity: "High".to_string(),
                    message: format!("High respiratory rate: {} breaths/min", rr),
                    created_at: Utc::now(),
                    acknowledged: false,
                    acknowledged_by: None,
                    acknowledged_at: None,
                }).await?;
            } else if rr < 8.0 {
                self.create_alert(Alert {
                    patient_id: vitals.patient_id.clone(),
                    alert_id: Uuid::new_v4().to_string(),
                    alert_type: "Clinical".to_string(),
                    severity: "Critical".to_string(),
                    message: format!("Critical low respiratory rate: {} breaths/min", rr),
                    created_at: Utc::now(),
                    acknowledged: false,
                    acknowledged_by: None,
                    acknowledged_at: None,
                }).await?;
            }
        }
        
        // Check temperature
        if let Some(temp) = vitals.temperature_celsius {
            if temp > 39.0 {
                self.create_alert(Alert {
                    patient_id: vitals.patient_id.clone(),
                    alert_id: Uuid::new_v4().to_string(),
                    alert_type: "Clinical".to_string(),
                    severity: "High".to_string(),
                    message: format!("High temperature: {}°C", temp),
                    created_at: Utc::now(),
                    acknowledged: false,
                    acknowledged_by: None,
                    acknowledged_at: None,
                }).await?;
            } else if temp < 35.0 {
                self.create_alert(Alert {
                    patient_id: vitals.patient_id.clone(),
                    alert_id: Uuid::new_v4().to_string(),
                    alert_type: "Clinical".to_string(),
                    severity: "High".to_string(),
                    message: format!("Low temperature: {}°C", temp),
                    created_at: Utc::now(),
                    acknowledged: false,
                    acknowledged_by: None,
                    acknowledged_at: None,
                }).await?;
            }
        }
        
        Ok(())
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_latest_vital_signs(&self, patient_id: &str) -> Result<Option<VitalSigns>> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let row = sqlx::query!(
                    "SELECT * FROM vital_signs 
                     WHERE patient_id = ? 
                     ORDER BY timestamp DESC 
                     LIMIT 1",
                    patient_id
                )
                .fetch_optional(pool)
                .await?;
                
                if let Some(row) = row {
                    let vitals = VitalSigns {
                        patient_id: row.patient_id,
                        timestamp: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.timestamp, 0)
                                .ok_or_else(|| anyhow!("Invalid timestamp"))?,
                            Utc,
                        ),
                        heart_rate: row.heart_rate,
                        respiratory_rate: row.respiratory_rate,
                        blood_pressure_systolic: row.blood_pressure_systolic,
                        blood_pressure_diastolic: row.blood_pressure_diastolic,
                        mean_arterial_pressure: row.mean_arterial_pressure,
                        temperature_celsius: row.temperature_celsius,
                        oxygen_saturation: row.oxygen_saturation,
                        end_tidal_co2: row.end_tidal_co2,
                        pain_score: row.pain_score,
                        glasgow_coma_scale: row.glasgow_coma_scale,
                        recorded_by: row.recorded_by,
                    };
                    
                    Ok(Some(vitals))
                } else {
                    Ok(None)
                }
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_vital_signs_history(&self, patient_id: &str, hours: i64) -> Result<Vec<VitalSigns>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let rows = sqlx::query!(
                    "SELECT * FROM vital_signs 
                     WHERE patient_id = ? AND timestamp >= ? 
                     ORDER BY timestamp DESC",
                    patient_id,
                    since.timestamp()
                )
                .fetch_all(pool)
                .await?;
                
                let mut vitals_history = Vec::with_capacity(rows.len());
                
                for row in rows {
                    let vitals = VitalSigns {
                        patient_id: row.patient_id,
                        timestamp: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.timestamp, 0)
                                .ok_or_else(|| anyhow!("Invalid timestamp"))?,
                            Utc,
                        ),
                        heart_rate: row.heart_rate,
                        respiratory_rate: row.respiratory_rate,
                        blood_pressure_systolic: row.blood_pressure_systolic,
                        blood_pressure_diastolic: row.blood_pressure_diastolic,
                        mean_arterial_pressure: row.mean_arterial_pressure,
                        temperature_celsius: row.temperature_celsius,
                        oxygen_saturation: row.oxygen_saturation,
                        end_tidal_co2: row.end_tidal_co2,
                        pain_score: row.pain_score,
                        glasgow_coma_scale: row.glasgow_coma_scale,
                        recorded_by: row.recorded_by,
                    };
                    
                    vitals_history.push(vitals);
                }
                
                Ok(vitals_history)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }

    // ===== Lab Results =====
    
    #[instrument(skip(self, lab_result), fields(patient_id = %lab_result.patient_id, test = %lab_result.test_name))]
    pub async fn record_lab_result(&self, lab_result: LabResult) -> Result<String> {
        let event_id = Uuid::new_v4().to_string();
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                sqlx::query(
                    "INSERT INTO lab_results (
                        patient_id, order_id, test_name, value, unit,
                        reference_range_low, reference_range_high, critical,
                        status, collected_at, resulted_at, event_id
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&lab_result.patient_id)
                .bind(&lab_result.order_id)
                .bind(&lab_result.test_name)
                .bind(lab_result.value)
                .bind(&lab_result.unit)
                .bind(lab_result.reference_range_low)
                .bind(lab_result.reference_range_high)
                .bind(lab_result.critical)
                .bind(&lab_result.status)
                .bind(lab_result.collected_at.timestamp())
                .bind(lab_result.resulted_at.timestamp())
                .bind(&event_id)
                .execute(pool)
                .await?;
                
                // Cache the lab result
                let cache_key = format!("lab_{}_{}", lab_result.patient_id, lab_result.test_name);
                let mut conn = self.redis_pool.get().await?;
                redis::cmd("SETEX")
                    .arg(&cache_key)
                    .arg(REDIS_TTL)
                    .arg(serde_json::to_string(&lab_result)?)
                    .execute_async(&mut *conn)
                    .await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&lab_result)?;
                self.blockchain_executor.log_to_blockchain("Record Lab Result", &lab_result.patient_id, &data).await?;
                
                // Check for critical values and create alerts
                if lab_result.critical {
                    self.create_alert(Alert {
                        patient_id: lab_result.patient_id.clone(),
                        alert_id: Uuid::new_v4().to_string(),
                        alert_type: "Lab".to_string(),
                        severity: "Critical".to_string(),
                        message: format!(
                            "Critical lab value: {} = {} {} (Reference range: {}-{})",
                            lab_result.test_name,
                            lab_result.value,
                            lab_result.unit,
                            lab_result.reference_range_low.unwrap_or(0.0),
                            lab_result.reference_range_high.unwrap_or(0.0)
                        ),
                        created_at: Utc::now(),
                        acknowledged: false,
                        acknowledged_by: None,
                        acknowledged_at: None,
                    }).await?;
                } else {
                    // Check if outside reference range
                    let outside_range = match (lab_result.reference_range_low, lab_result.reference_range_high) {
                        (Some(low), Some(high)) => lab_result.value < low || lab_result.value > high,
                        (Some(low), None) => lab_result.value < low,
                        (None, Some(high)) => lab_result.value > high,
                        (None, None) => false,
                    };
                    
                    if outside_range {
                        self.create_alert(Alert {
                            patient_id: lab_result.patient_id.clone(),
                            alert_id: Uuid::new_v4().to_string(),
                            alert_type: "Lab".to_string(),
                            severity: "High".to_string(),
                            message: format!(
                                "Abnormal lab value: {} = {} {} (Reference range: {}-{})",
                                lab_result.test_name,
                                lab_result.value,
                                lab_result.unit,
                                lab_result.reference_range_low.unwrap_or(0.0),
                                lab_result.reference_range_high.unwrap_or(0.0)
                            ),
                            created_at: Utc::now(),
                            acknowledged: false,
                            acknowledged_by: None,
                            acknowledged_at: None,
                        }).await?;
                    }
                }
                
                Ok(event_id)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id, test_name = %test_name))]
    pub async fn get_latest_lab_result(&self, patient_id: &str, test_name: &str) -> Result<Option<LabResult>> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let row = sqlx::query!(
                    "SELECT * FROM lab_results 
                     WHERE patient_id = ? AND test_name = ? 
                     ORDER BY resulted_at DESC 
                     LIMIT 1",
                    patient_id,
                    test_name
                )
                .fetch_optional(pool)
                .await?;
                
                if let Some(row) = row {
                    let lab_result = LabResult {
                        patient_id: row.patient_id,
                        order_id: row.order_id,
                        test_name: row.test_name,
                        value: row.value,
                        unit: row.unit,
                        reference_range_low: row.reference_range_low,
                        reference_range_high: row.reference_range_high,
                        critical: row.critical != 0,
                        status: row.status,
                        collected_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.collected_at, 0)
                                .ok_or_else(|| anyhow!("Invalid collected_at timestamp"))?,
                            Utc,
                        ),
                        resulted_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.resulted_at, 0)
                                .ok_or_else(|| anyhow!("Invalid resulted_at timestamp"))?,
                            Utc,
                        ),
                    };
                    
                    Ok(Some(lab_result))
                } else {
                    Ok(None)
                }
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }

    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_lab_results_history(&self, patient_id: &str, hours: i64) -> Result<Vec<LabResult>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let rows = sqlx::query!(
                    "SELECT * FROM lab_results 
                     WHERE patient_id = ? AND resulted_at >= ? 
                     ORDER BY resulted_at DESC",
                    patient_id,
                    since.timestamp()
                )
                .fetch_all(pool)
                .await?;
                
                let mut lab_history = Vec::with_capacity(rows.len());
                
                for row in rows {
                    let lab_result = LabResult {
                        patient_id: row.patient_id,
                        order_id: row.order_id,
                        test_name: row.test_name,
                        value: row.value,
                        unit: row.unit,
                        reference_range_low: row.reference_range_low,
                        reference_range_high: row.reference_range_high,
                        critical: row.critical != 0,
                        status: row.status,
                        collected_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.collected_at, 0)
                                .ok_or_else(|| anyhow!("Invalid collected_at timestamp"))?,
                            Utc,
                        ),
                        resulted_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.resulted_at, 0)
                                .ok_or_else(|| anyhow!("Invalid resulted_at timestamp"))?,
                            Utc,
                        ),
                    };
                    
                    lab_history.push(lab_result);
                }
                
                Ok(lab_history)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }

    // ===== Medications =====
    
    #[instrument(skip(self, medication), fields(patient_id = %medication.patient_id, med = %medication.name))]
    pub async fn add_medication(&self, medication: Medication) -> Result<()> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                sqlx::query(
                    "INSERT INTO medications (
                        patient_id, order_id, name, dose, route,
                        frequency, start_date, end_date, ordering_provider,
                        status, pharmacy_status, last_administered,
                        medication_class, is_high_alert
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&medication.patient_id)
                .bind(&medication.order_id)
                .bind(&medication.name)
                .bind(&medication.dose)
                .bind(&medication.route)
                .bind(&medication.frequency)
                .bind(medication.start_date.timestamp())
                .bind(medication.end_date.map(|d| d.timestamp()))
                .bind(&medication.ordering_provider)
                .bind(&medication.status)
                .bind(&medication.pharmacy_status)
                .bind(medication.last_administered.map(|d| d.timestamp()))
                .bind(&medication.medication_class)
                .bind(medication.is_high_alert)
                .execute(pool)
                .await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&medication)?;
                self.blockchain_executor.log_to_blockchain("Add Medication", &medication.patient_id, &data).await?;
                
                // Create alert for high-alert medications
                if medication.is_high_alert {
                    self.create_alert(Alert {
                        patient_id: medication.patient_id.clone(),
                        alert_id: Uuid::new_v4().to_string(),
                        alert_type: "Medication".to_string(),
                        severity: "High".to_string(),
                        message: format!(
                            "High-alert medication added: {} {} {}",
                            medication.name,
                            medication.dose,
                            medication.route
                        ),
                        created_at: Utc::now(),
                        acknowledged: false,
                        acknowledged_by: None,
                        acknowledged_at: None,
                    }).await?;
                }
                
                Ok(())
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self, admin), fields(patient_id = %admin.patient_id, med_id = %admin.medication_id))]
    pub async fn record_medication_administration(&self, admin: MedicationAdministration) -> Result<String> {
        let event_id = Uuid::new_v4().to_string();
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                // Record the administration
                sqlx::query(
                    "INSERT INTO medication_administrations (
                        patient_id, medication_id, administered_at,
                        administered_by, dose_given, site, notes, event_id
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&admin.patient_id)
                .bind(&admin.medication_id)
                .bind(admin.administered_at.timestamp())
                .bind(&admin.administered_by)
                .bind(&admin.dose_given)
                .bind(&admin.site)
                .bind(&admin.notes)
                .bind(&event_id)
                .execute(pool)
                .await?;
                
                // Update the last_administered time in the medications table
                sqlx::query(
                    "UPDATE medications 
                     SET last_administered = ? 
                     WHERE patient_id = ? AND order_id = ?"
                )
                .bind(admin.administered_at.timestamp())
                .bind(&admin.patient_id)
                .bind(&admin.medication_id)
                .execute(pool)
                .await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&admin)?;
                self.blockchain_executor.log_to_blockchain("Administer Medication", &admin.patient_id, &data).await?;
                
                Ok(event_id)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_active_medications(&self, patient_id: &str) -> Result<Vec<Medication>> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let rows = sqlx::query!(
                    "SELECT * FROM medications 
                     WHERE patient_id = ? AND status = 'Active' 
                     ORDER BY start_date DESC",
                    patient_id
                )
                .fetch_all(pool)
                .await?;
                
                let mut medications = Vec::with_capacity(rows.len());
                
                for row in rows {
                    let medication = Medication {
                        patient_id: row.patient_id,
                        order_id: row.order_id,
                        name: row.name,
                        dose: row.dose,
                        route: row.route,
                        frequency: row.frequency,
                        start_date: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.start_date, 0)
                                .ok_or_else(|| anyhow!("Invalid start_date timestamp"))?,
                            Utc,
                        ),
                        end_date: if let Some(end_date) = row.end_date {
                            Some(DateTime::<Utc>::from_utc(
                                NaiveDateTime::from_timestamp_opt(end_date, 0)
                                    .ok_or_else(|| anyhow!("Invalid end_date timestamp"))?,
                                Utc,
                            ))
                        } else {
                            None
                        },
                        ordering_provider: row.ordering_provider,
                        status: row.status,
                        pharmacy_status: row.pharmacy_status,
                        last_administered: if let Some(last_admin) = row.last_administered {
                            Some(DateTime::<Utc>::from_utc(
                                NaiveDateTime::from_timestamp_opt(last_admin, 0)
                                    .ok_or_else(|| anyhow!("Invalid last_administered timestamp"))?,
                                Utc,
                            ))
                        } else {
                            None
                        },
                        medication_class: row.medication_class,
                        is_high_alert: row.is_high_alert != 0,
                    };
                    
                    medications.push(medication);
                }
                
                Ok(medications)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }

    #[instrument(skip(self), fields(patient_id = %patient_id, medication_id = %medication_id))]
    pub async fn get_medication_administrations(&self, patient_id: &str, medication_id: &str, hours: i64) -> Result<Vec<MedicationAdministration>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let rows = sqlx::query!(
                    "SELECT * FROM medication_administrations 
                     WHERE patient_id = ? AND medication_id = ? AND administered_at >= ? 
                     ORDER BY administered_at DESC",
                    patient_id,
                    medication_id,
                    since.timestamp()
                )
                .fetch_all(pool)
                .await?;
                
                let mut administrations = Vec::with_capacity(rows.len());
                
                for row in rows {
                    let administration = MedicationAdministration {
                        patient_id: row.patient_id,
                        medication_id: row.medication_id,
                        administered_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.administered_at, 0)
                                .ok_or_else(|| anyhow!("Invalid administered_at timestamp"))?,
                            Utc,
                        ),
                        administered_by: row.administered_by,
                        dose_given: row.dose_given,
                        site: row.site,
                        notes: row.notes,
                    };
                    
                    administrations.push(administration);
                }
                
                Ok(administrations)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    // ===== Fluid Intake/Output =====
    
    #[instrument(skip(self, io), fields(patient_id = %io.patient_id))]
    pub async fn record_fluid_io(&self, io: FluidIntakeOutput) -> Result<String> {
        let event_id = Uuid::new_v4().to_string();
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let mut tx = pool.begin().await?;
                
                // Record intake
                for (category, amount) in &io.intake {
                    sqlx::query(
                        "INSERT INTO fluid_io (
                            patient_id, timestamp, recorded_by, io_type,
                            category, amount, event_id
                        ) VALUES (?, ?, ?, 'intake', ?, ?, ?)"
                    )
                    .bind(&io.patient_id)
                    .bind(io.timestamp.timestamp())
                    .bind(&io.recorded_by)
                    .bind(category)
                    .bind(amount)
                    .bind(&event_id)
                    .execute(&mut *tx)
                    .await?;
                }
                
                // Record output
                for (category, amount) in &io.output {
                    sqlx::query(
                        "INSERT INTO fluid_io (
                            patient_id, timestamp, recorded_by, io_type,
                            category, amount, event_id
                        ) VALUES (?, ?, ?, 'output', ?, ?, ?)"
                    )
                    .bind(&io.patient_id)
                    .bind(io.timestamp.timestamp())
                    .bind(&io.recorded_by)
                    .bind(category)
                    .bind(amount)
                    .bind(&event_id)
                    .execute(&mut *tx)
                    .await?;
                }
                
                tx.commit().await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&io)?;
                self.blockchain_executor.log_to_blockchain("Record Fluid I/O", &io.patient_id, &data).await?;
                
                Ok(event_id)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_fluid_balance(&self, patient_id: &str, hours: i64) -> Result<(f64, f64, f64)> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                // Get total intake
                let intake_row = sqlx::query!(
                    "SELECT SUM(amount) as total FROM fluid_io 
                     WHERE patient_id = ? AND io_type = 'intake' AND timestamp >= ?",
                    patient_id,
                    since.timestamp()
                )
                .fetch_one(pool)
                .await?;
                
                let total_intake = intake_row.total.unwrap_or(0.0);
                
                // Get total output
                let output_row = sqlx::query!(
                    "SELECT SUM(amount) as total FROM fluid_io 
                     WHERE patient_id = ? AND io_type = 'output' AND timestamp >= ?",
                    patient_id,
                    since.timestamp()
                )
                .fetch_one(pool)
                .await?;
                
                let total_output = output_row.total.unwrap_or(0.0);
                
                // Calculate balance
                let balance = total_intake - total_output;
                
                Ok((total_intake, total_output, balance))
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_fluid_io_history(&self, patient_id: &str, hours: i64) -> Result<Vec<(DateTime<Utc>, String, String, f64)>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let rows = sqlx::query!(
                    "SELECT timestamp, io_type, category, amount FROM fluid_io 
                     WHERE patient_id = ? AND timestamp >= ? 
                     ORDER BY timestamp DESC",
                    patient_id,
                    since.timestamp()
                )
                .fetch_all(pool)
                .await?;
                
                let mut io_history = Vec::with_capacity(rows.len());
                
                for row in rows {
                    let timestamp = DateTime::<Utc>::from_utc(
                        NaiveDateTime::from_timestamp_opt(row.timestamp, 0)
                            .ok_or_else(|| anyhow!("Invalid timestamp"))?,
                        Utc,
                    );
                    
                    io_history.push((timestamp, row.io_type, row.category, row.amount));
                }
                
                Ok(io_history)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    // ===== Ventilator Settings =====
    
    #[instrument(skip(self, settings), fields(patient_id = %settings.patient_id))]
    pub async fn record_ventilator_settings(&self, settings: VentilatorSettings) -> Result<String> {
        let event_id = Uuid::new_v4().to_string();
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                sqlx::query(
                    "INSERT INTO ventilator_settings (
                        patient_id, timestamp, mode, fio2, peep,
                        respiratory_rate, tidal_volume, pressure_support,
                        inspiratory_pressure, i_time, recorded_by, event_id
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&settings.patient_id)
                .bind(settings.timestamp.timestamp())
                .bind(&settings.mode)
                .bind(settings.fio2)
                .bind(settings.peep)
                .bind(settings.respiratory_rate)
                .bind(settings.tidal_volume)
                .bind(settings.pressure_support)
                .bind(settings.inspiratory_pressure)
                .bind(settings.i_time)
                .bind(&settings.recorded_by)
                .bind(&event_id)
                .execute(pool)
                .await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&settings)?;
                self.blockchain_executor.log_to_blockchain("Record Ventilator Settings", &settings.patient_id, &data).await?;
                
                Ok(event_id)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }

    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_latest_ventilator_settings(&self, patient_id: &str) -> Result<Option<VentilatorSettings>> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let row = sqlx::query!(
                    "SELECT * FROM ventilator_settings 
                     WHERE patient_id = ? 
                     ORDER BY timestamp DESC 
                     LIMIT 1",
                    patient_id
                )
                .fetch_optional(pool)
                .await?;
                
                if let Some(row) = row {
                    let settings = VentilatorSettings {
                        patient_id: row.patient_id,
                        timestamp: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.timestamp, 0)
                                .ok_or_else(|| anyhow!("Invalid timestamp"))?,
                            Utc,
                        ),
                        mode: row.mode,
                        fio2: row.fio2,
                        peep: row.peep,
                        respiratory_rate: row.respiratory_rate,
                        tidal_volume: row.tidal_volume,
                        pressure_support: row.pressure_support,
                        inspiratory_pressure: row.inspiratory_pressure,
                        i_time: row.i_time,
                        recorded_by: row.recorded_by,
                    };
                    
                    Ok(Some(settings))
                } else {
                    Ok(None)
                }
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }

    // ===== Assessments =====
    
    #[instrument(skip(self, assessment), fields(patient_id = %assessment.patient_id, type = %assessment.assessment_type))]
    pub async fn record_assessment(&self, assessment: Assessment) -> Result<String> {
        let event_id = Uuid::new_v4().to_string();
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let mut tx = pool.begin().await?;
                
                // Insert assessment
                let assessment_id = sqlx::query!(
                    "INSERT INTO assessments (
                        patient_id, timestamp, assessment_type, notes,
                        performed_by, event_id
                    ) VALUES (?, ?, ?, ?, ?, ?)
                    RETURNING id",
                    assessment.patient_id,
                    assessment.timestamp.timestamp(),
                    assessment.assessment_type,
                    assessment.notes,
                    assessment.performed_by,
                    event_id
                )
                .fetch_one(&mut *tx)
                .await?
                .id;
                
                // Insert findings
                for (key, value) in &assessment.findings {
                    sqlx::query(
                        "INSERT INTO assessment_findings (
                            assessment_id, key, value
                        ) VALUES (?, ?, ?)"
                    )
                    .bind(assessment_id)
                    .bind(key)
                    .bind(value)
                    .execute(&mut *tx)
                    .await?;
                }
                
                tx.commit().await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&assessment)?;
                self.blockchain_executor.log_to_blockchain("Record Assessment", &assessment.patient_id, &data).await?;
                
                Ok(event_id)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id, assessment_type = %assessment_type))]
    pub async fn get_latest_assessment(&self, patient_id: &str, assessment_type: &str) -> Result<Option<Assessment>> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                // Get the latest assessment
                let assessment_row = sqlx::query!(
                    "SELECT * FROM assessments 
                     WHERE patient_id = ? AND assessment_type = ? 
                     ORDER BY timestamp DESC 
                     LIMIT 1",
                    patient_id,
                    assessment_type
                )
                .fetch_optional(pool)
                .await?;
                
                if let Some(row) = assessment_row {
                    // Get findings for this assessment
                    let findings_rows = sqlx::query!(
                        "SELECT key, value FROM assessment_findings 
                         WHERE assessment_id = ?",
                        row.id
                    )
                    .fetch_all(pool)
                    .await?;
                    
                    let findings = findings_rows.into_iter()
                        .map(|r| (r.key, r.value))
                        .collect();
                    
                    let assessment = Assessment {
                        patient_id: row.patient_id,
                        timestamp: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.timestamp, 0)
                                .ok_or_else(|| anyhow!("Invalid timestamp"))?,
                            Utc,
                        ),
                        assessment_type: row.assessment_type,
                        findings,
                        notes: row.notes,
                        performed_by: row.performed_by,
                    };
                    
                    Ok(Some(assessment))
                } else {
                    Ok(None)
                }
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    // ===== Clinical Notes =====
    
    #[instrument(skip(self, note), fields(patient_id = %note.patient_id, type = %note.note_type))]
    pub async fn add_clinical_note(&self, note: ClinicalNote) -> Result<()> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                sqlx::query(
                    "INSERT INTO clinical_notes (
                        patient_id, note_id, note_type, content,
                        created_at, created_by, cosigned_by, cosigned_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&note.patient_id)
                .bind(&note.note_id)
                .bind(&note.note_type)
                .bind(&note.content)
                .bind(note.created_at.timestamp())
                .bind(&note.created_by)
                .bind(&note.cosigned_by)
                .bind(note.cosigned_at.map(|d| d.timestamp()))
                .execute(pool)
                .await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&note)?;
                self.blockchain_executor.log_to_blockchain("Add Clinical Note", &note.patient_id, &data).await?;
                
                Ok(())
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_clinical_notes(&self, patient_id: &str, note_type: Option<&str>, limit: i64) -> Result<Vec<ClinicalNote>> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let rows = if let Some(note_type) = note_type {
                    sqlx::query!(
                        "SELECT * FROM clinical_notes 
                         WHERE patient_id = ? AND note_type = ? 
                         ORDER BY created_at DESC 
                         LIMIT ?",
                        patient_id,
                        note_type,
                        limit
                    )
                    .fetch_all(pool)
                    .await?
                } else {
                    sqlx::query!(
                        "SELECT * FROM clinical_notes 
                         WHERE patient_id = ? 
                         ORDER BY created_at DESC 
                         LIMIT ?",
                        patient_id,
                        limit
                    )
                    .fetch_all(pool)
                    .await?
                };
                
                let mut notes = Vec::with_capacity(rows.len());
                
                for row in rows {
                    let note = ClinicalNote {
                        patient_id: row.patient_id,
                        note_id: row.note_id,
                        note_type: row.note_type,
                        content: row.content,
                        created_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.created_at, 0)
                                .ok_or_else(|| anyhow!("Invalid created_at timestamp"))?,
                            Utc,
                        ),
                        created_by: row.created_by,
                        cosigned_by: row.cosigned_by,
                        cosigned_at: if let Some(cosigned_at) = row.cosigned_at {
                            Some(DateTime::<Utc>::from_utc(
                                NaiveDateTime::from_timestamp_opt(cosigned_at, 0)
                                    .ok_or_else(|| anyhow!("Invalid cosigned_at timestamp"))?,
                                Utc,
                            ))
                        } else {
                            None
                        },
                    };
                    
                    notes.push(note);
                }
                
                Ok(notes)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }

    // ===== Orders =====
    
    #[instrument(skip(self, order), fields(patient_id = %order.patient_id, type = %order.order_type))]
    pub async fn create_order(&self, order: Order) -> Result<()> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                sqlx::query(
                    "INSERT INTO orders (
                        patient_id, order_id, order_type, details,
                        ordered_at, ordered_by, status, start_date,
                        end_date, frequency, priority
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&order.patient_id)
                .bind(&order.order_id)
                .bind(&order.order_type)
                .bind(&order.details)
                .bind(order.ordered_at.timestamp())
                .bind(&order.ordered_by)
                .bind(&order.status)
                .bind(order.start_date.timestamp())
                .bind(order.end_date.map(|d| d.timestamp()))
                .bind(&order.frequency)
                .bind(&order.priority)
                .execute(pool)
                .await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&order)?;
                self.blockchain_executor.log_to_blockchain("Create Order", &order.patient_id, &data).await?;
                
                // Create alert for STAT orders
                if order.priority == "STAT" {
                    self.create_alert(Alert {
                        patient_id: order.patient_id.clone(),
                        alert_id: Uuid::new_v4().to_string(),
                        alert_type: "Order".to_string(),
                        severity: "High".to_string(),
                        message: format!(
                            "STAT order placed: {} - {}",
                            order.order_type,
                            order.details
                        ),
                        created_at: Utc::now(),
                        acknowledged: false,
                        acknowledged_by: None,
                        acknowledged_at: None,
                    }).await?;
                }
                
                Ok(())
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_active_orders(&self, patient_id: &str, order_type: Option<&str>) -> Result<Vec<Order>> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let rows = if let Some(order_type) = order_type {
                    sqlx::query!(
                        "SELECT * FROM orders 
                         WHERE patient_id = ? AND order_type = ? AND status = 'Active' 
                         ORDER BY ordered_at DESC",
                        patient_id,
                        order_type
                    )
                    .fetch_all(pool)
                    .await?
                } else {
                    sqlx::query!(
                        "SELECT * FROM orders 
                         WHERE patient_id = ? AND status = 'Active' 
                         ORDER BY ordered_at DESC",
                        patient_id
                    )
                    .fetch_all(pool)
                    .await?
                };
                
                let mut orders = Vec::with_capacity(rows.len());
                
                for row in rows {
                    let order = Order {
                        patient_id: row.patient_id,
                        order_id: row.order_id,
                        order_type: row.order_type,
                        details: row.details,
                        ordered_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.ordered_at, 0)
                                .ok_or_else(|| anyhow!("Invalid ordered_at timestamp"))?,
                            Utc,
                        ),
                        ordered_by: row.ordered_by,
                        status: row.status,
                        start_date: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.start_date, 0)
                                .ok_or_else(|| anyhow!("Invalid start_date timestamp"))?,
                            Utc,
                        ),
                        end_date: if let Some(end_date) = row.end_date {
                            Some(DateTime::<Utc>::from_utc(
                                NaiveDateTime::from_timestamp_opt(end_date, 0)
                                    .ok_or_else(|| anyhow!("Invalid end_date timestamp"))?,
                                Utc,
                            ))
                        } else {
                            None
                        },
                        frequency: row.frequency,
                        priority: row.priority,
                    };
                    
                    orders.push(order);
                }
                
                Ok(orders)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id, order_id = %order_id))]
    pub async fn update_order_status(&self, patient_id: &str, order_id: &str, status: &str) -> Result<()> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                sqlx::query(
                    "UPDATE orders 
                     SET status = ? 
                     WHERE patient_id = ? AND order_id = ?"
                )
                .bind(status)
                .bind(patient_id)
                .bind(order_id)
                .execute(pool)
                .await?;
                
                // Log to blockchain
                let data = format!("{{\"status\": \"{}\"}}", status);
                self.blockchain_executor.log_to_blockchain("Update Order Status", patient_id, &data).await?;
                
                Ok(())
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    // ===== Care Plans =====
    
    #[instrument(skip(self, care_plan), fields(patient_id = %care_plan.patient_id))]
    pub async fn update_care_plan(&self, care_plan: CarePlan) -> Result<()> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let mut tx = pool.begin().await?;
                
                // Check if a care plan already exists for this patient
                let existing_care_plan = sqlx::query!(
                    "SELECT id FROM care_plans WHERE patient_id = ?",
                    care_plan.patient_id
                )
                .fetch_optional(&mut *tx)
                .await?;
                
                let care_plan_id = if let Some(row) = existing_care_plan {
                    // Update existing care plan
                    sqlx::query!(
                        "UPDATE care_plans 
                         SET updated_at = ?, updated_by = ? 
                         WHERE id = ?",
                        care_plan.updated_at.timestamp(),
                        care_plan.updated_by,
                        row.id
                    )
                    .execute(&mut *tx)
                    .await?;
                    
                    // Delete existing problems, goals, and interventions
                    sqlx::query!("DELETE FROM problems WHERE care_plan_id = ?", row.id)
                        .execute(&mut *tx)
                        .await?;
                    
                    sqlx::query!("DELETE FROM goals WHERE care_plan_id = ?", row.id)
                        .execute(&mut *tx)
                        .await?;
                    
                    sqlx::query!("DELETE FROM interventions WHERE care_plan_id = ?", row.id)
                        .execute(&mut *tx)
                        .await?;
                    
                    row.id
                } else {
                    // Create new care plan
                    sqlx::query!(
                        "INSERT INTO care_plans (patient_id, updated_at, updated_by) 
                         VALUES (?, ?, ?)
                         RETURNING id",
                        care_plan.patient_id,
                        care_plan.updated_at.timestamp(),
                        care_plan.updated_by
                    )
                    .fetch_one(&mut *tx)
                    .await?
                    .id
                };
                
                // Insert problems
                for problem in &care_plan.problems {
                    sqlx::query!(
                        "INSERT INTO problems (
                            care_plan_id, problem_id, description, status,
                            onset_date, resolution_date
                        ) VALUES (?, ?, ?, ?, ?, ?)",
                        care_plan_id,
                        problem.id,
                        problem.description,
                        problem.status,
                        problem.onset_date.map(|d| d.timestamp()),
                        problem.resolution_date.map(|d| d.timestamp())
                    )
                    .execute(&mut *tx)
                    .await?;
                }
                
                // Insert goals
                for goal in &care_plan.goals {
                    sqlx::query!(
                        "INSERT INTO goals (
                            care_plan_id, goal_id, description, target_date, status
                        ) VALUES (?, ?, ?, ?, ?)",
                        care_plan_id,
                        goal.id,
                        goal.description,
                        goal.target_date.map(|d| d.timestamp()),
                        goal.status
                    )
                    .execute(&mut *tx)
                    .await?;
                }
                
                // Insert interventions
                for intervention in &care_plan.interventions {
                    sqlx::query!(
                        "INSERT INTO interventions (
                            care_plan_id, intervention_id, description, 
                            frequency, responsible_role
                        ) VALUES (?, ?, ?, ?, ?)",
                        care_plan_id,
                        intervention.id,
                        intervention.description,
                        intervention.frequency,
                        intervention.responsible_role
                    )
                    .execute(&mut *tx)
                    .await?;
                }
                
                tx.commit().await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&care_plan)?;
                self.blockchain_executor.log_to_blockchain("Update Care Plan", &care_plan.patient_id, &data).await?;
                
                Ok(())
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_care_plan(&self, patient_id: &str) -> Result<Option<CarePlan>> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                // Get care plan
                let care_plan_row = sqlx::query!(
                    "SELECT * FROM care_plans WHERE patient_id = ?",
                    patient_id
                )
                .fetch_optional(pool)
                .await?;
                
                if let Some(row) = care_plan_row {
                    // Get problems
                    let problem_rows = sqlx::query!(
                        "SELECT * FROM problems WHERE care_plan_id = ?",
                        row.id
                    )
                    .fetch_all(pool)
                    .await?;
                    
                    let mut problems = Vec::with_capacity(problem_rows.len());
                    for p_row in problem_rows {
                        let problem = Problem {
                            id: p_row.problem_id,
                            description: p_row.description,
                            status: p_row.status,
                            onset_date: if let Some(onset) = p_row.onset_date {
                                Some(DateTime::<Utc>::from_utc(
                                    NaiveDateTime::from_timestamp_opt(onset, 0)
                                        .ok_or_else(|| anyhow!("Invalid onset_date timestamp"))?,
                                    Utc,
                                ))
                            } else {
                                None
                            },
                            resolution_date: if let Some(resolution) = p_row.resolution_date {
                                Some(DateTime::<Utc>::from_utc(
                                    NaiveDateTime::from_timestamp_opt(resolution, 0)
                                        .ok_or_else(|| anyhow!("Invalid resolution_date timestamp"))?,
                                    Utc,
                                ))
                            } else {
                                None
                            },
                        };
                        problems.push(problem);
                    }
                    
                    // Get goals
                    let goal_rows = sqlx::query!(
                        "SELECT * FROM goals WHERE care_plan_id = ?",
                        row.id
                    )
                    .fetch_all(pool)
                    .await?;
                    
                    let mut goals = Vec::with_capacity(goal_rows.len());
                    for g_row in goal_rows {
                        let goal = Goal {
                            id: g_row.goal_id,
                            description: g_row.description,
                            target_date: if let Some(target) = g_row.target_date {
                                Some(DateTime::<Utc>::from_utc(
                                    NaiveDateTime::from_timestamp_opt(target, 0)
                                        .ok_or_else(|| anyhow!("Invalid target_date timestamp"))?,
                                    Utc,
                                ))
                            } else {
                                None
                            },
                            status: g_row.status,
                        };
                        goals.push(goal);
                    }
                    
                    // Get interventions
                    let intervention_rows = sqlx::query!(
                        "SELECT * FROM interventions WHERE care_plan_id = ?",
                        row.id
                    )
                    .fetch_all(pool)
                    .await?;
                    
                    let mut interventions = Vec::with_capacity(intervention_rows.len());
                    for i_row in intervention_rows {
                        let intervention = Intervention {
                            id: i_row.intervention_id,
                            description: i_row.description,
                            frequency: i_row.frequency,
                            responsible_role: i_row.responsible_role,
                        };
                        interventions.push(intervention);
                    }
                    
                    let care_plan = CarePlan {
                        patient_id: row.patient_id,
                        problems,
                        goals,
                        interventions,
                        updated_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.updated_at, 0)
                                .ok_or_else(|| anyhow!("Invalid updated_at timestamp"))?,
                            Utc,
                        ),
                        updated_by: row.updated_by,
                    };
                    
                    Ok(Some(care_plan))
                } else {
                    Ok(None)
                }
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    // ===== Alerts =====
    
    #[instrument(skip(self, alert), fields(patient_id = %alert.patient_id, type = %alert.alert_type))]
    pub async fn create_alert(&self, alert: Alert) -> Result<()> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                sqlx::query(
                    "INSERT INTO alerts (
                        patient_id, alert_id, alert_type, severity,
                        message, created_at, acknowledged,
                        acknowledged_by, acknowledged_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&alert.patient_id)
                .bind(&alert.alert_id)
                .bind(&alert.alert_type)
                .bind(&alert.severity)
                .bind(&alert.message)
                .bind(alert.created_at.timestamp())
                .bind(alert.acknowledged)
                .bind(&alert.acknowledged_by)
                .bind(alert.acknowledged_at.map(|d| d.timestamp()))
                .execute(pool)
                .await?;
                
                // Log to blockchain
                let data = serde_json::to_string(&alert)?;
                self.blockchain_executor.log_to_blockchain("Create Alert", &alert.patient_id, &data).await?;
                
                Ok(())
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id, alert_id = %alert_id))]
    pub async fn acknowledge_alert(&self, patient_id: &str, alert_id: &str, acknowledged_by: &str) -> Result<()> {
        let now = Utc::now();
        
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                sqlx::query(
                    "UPDATE alerts 
                     SET acknowledged = 1, acknowledged_by = ?, acknowledged_at = ? 
                     WHERE patient_id = ? AND alert_id = ?"
                )
                .bind(acknowledged_by)
                .bind(now.timestamp())
                .bind(patient_id)
                .bind(alert_id)
                .execute(pool)
                .await?;
                
                // Log to blockchain
                let data = format!(
                    "{{\"acknowledged_by\": \"{}\", \"acknowledged_at\": \"{}\"}}",
                    acknowledged_by,
                    now.to_rfc3339()
                );
                self.blockchain_executor.log_to_blockchain("Acknowledge Alert", patient_id, &data).await?;
                
                Ok(())
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_active_alerts(&self, patient_id: &str) -> Result<Vec<Alert>> {
        match &*self.db {
            DatabaseBackend::SQLite(pool) => {
                let rows = sqlx::query!(
                    "SELECT * FROM alerts 
                     WHERE patient_id = ? AND acknowledged = 0 
                     ORDER BY created_at DESC",
                    patient_id
                )
                .fetch_all(pool)
                .await?;
                
                let mut alerts = Vec::with_capacity(rows.len());
                
                for row in rows {
                    let alert = Alert {
                        patient_id: row.patient_id,
                        alert_id: row.alert_id,
                        alert_type: row.alert_type,
                        severity: row.severity,
                        message: row.message,
                        created_at: DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp_opt(row.created_at, 0)
                                .ok_or_else(|| anyhow!("Invalid created_at timestamp"))?,
                            Utc,
                        ),
                        acknowledged: row.acknowledged != 0,
                        acknowledged_by: row.acknowledged_by,
                        acknowledged_at: if let Some(ack_at) = row.acknowledged_at {
                            Some(DateTime::<Utc>::from_utc(
                                NaiveDateTime::from_timestamp_opt(ack_at, 0)
                                    .ok_or_else(|| anyhow!("Invalid acknowledged_at timestamp"))?,
                                Utc,
                            ))
                        } else {
                            None
                        },
                    };
                    
                    alerts.push(alert);
                }
                
                Ok(alerts)
            }
            #[cfg(feature = "clickhouse")]
            DatabaseBackend::ClickHouse(_) => {
                Err(anyhow!("ClickHouse implementation not yet available"))
            }
        }
    }
    
    // ===== Blockchain Audit Trail =====
    
    #[instrument(skip(self), fields(patient_id = %patient_id))]
    pub async fn get_blockchain_audit_trail(&self, patient_id: &str) -> Result<Vec<String>> {
        self.blockchain_executor.query_blockchain(patient_id, "audit_trail").await
    }
}
