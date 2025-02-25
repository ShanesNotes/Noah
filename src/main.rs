//! Noah EHR System
//! 
//! Main entry point for the Noah EHR System.

use std::sync::Arc;
use log::info;

#[cfg(feature = "web-ui")]
use actix_web::{App, HttpServer, middleware};
#[cfg(feature = "web-ui")]
use actix_files as fs;

#[cfg(feature = "desktop-ui")]
use iced::{Settings, Application};

use noah::core::ai::AiService;
use noah::api;
use noah::db::Database;
use noah::config;

#[cfg(feature = "web-ui")]
async fn run_web_ui(config: &config::Config, db: Arc<Database>, ai_service: Arc<AiService>) -> std::io::Result<()> {
    info!("Starting Noah web UI on {}:{}", config.server.host, config.server.port);
    
    // Create app state
    let app_state = actix_web::web::Data::new(AppState {
        db,
        ai_service,
        config: config.clone(),
    });
    
    // Start HTTP server
    HttpServer::new(move || {
        App::new()
            // Add app state
            .app_data(app_state.clone())
            // Enable logger
            .wrap(middleware::Logger::default())
            // API routes
            .configure(api::configure)
            // Serve static files from the web directory
            .service(fs::Files::new("/", "./web").index_file("index.html"))
    })
    .bind(format!("{}:{}", config.server.host, config.server.port))?
    .run()
    .await
}

#[cfg(feature = "desktop-ui")]
fn run_desktop_ui(ai_service: Arc<AiService>) -> Result<(), iced::Error> {
    info!("Starting Noah desktop UI");
    
    noah::ui::desktop::FloodApp::run(Settings::with_flags(ai_service))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    
    // Load configuration
    let config = config::load_config().expect("Failed to load configuration");
    
    // Connect to database
    let database = Database::connect(&config.database.url)
        .await
        .expect("Failed to connect to database");
    
    // Run migrations
    database.run_migrations()
        .await
        .expect("Failed to run database migrations");
        
    // Initialize AI service
    let arc_api_key = std::env::var("ARC_API_KEY").unwrap_or_default();
    let rig_api_key = std::env::var("RIG_API_KEY").unwrap_or_default();
    let ai_service = Arc::new(AiService::new(arc_api_key, rig_api_key));
    
    // Run the appropriate UI
    #[cfg(feature = "web-ui")]
    run_web_ui(&config, Arc::new(database), ai_service.clone()).await?;
    
    #[cfg(feature = "desktop-ui")]
    run_desktop_ui(ai_service.clone())?;
    
    Ok(())
}

/// Application state
struct AppState {
    db: Arc<Database>,
    ai_service: Arc<AiService>,
    config: config::Config,
}