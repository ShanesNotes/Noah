//! Noah EHR System
//! 
//! Main entry point for the Noah EHR System.

use actix_files as fs;
use actix_web::{App, HttpServer, middleware};
use noah::{config, api, websocket, db};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize logger
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    
    // Load configuration
    let config = config::load_config().expect("Failed to load configuration");
    
    // Connect to database
    let database = db::Database::connect(&config.database.url)
        .await
        .expect("Failed to connect to database");
    
    // Run migrations
    database.run_migrations()
        .await
        .expect("Failed to run database migrations");
    
    // Create app state
    let app_state = web::Data::new(AppState {
        db: database,
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
            // WebSocket route
            .service(websocket::server::websocket_route())
            // Serve static files from the web directory
            .service(fs::Files::new("/", "./web").index_file("index.html"))
    })
    .bind(format!("{}:{}", config.server.host, config.server.port))?
    .run()
    .await
}

/// Application state
struct AppState {
    db: db::Database,
    config: config::Config,
}