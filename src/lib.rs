//! Noah EHR System core library
//! 
//! This module exports the core functionality of the Noah EHR system.

pub mod api;
pub mod ehr;
pub mod models;
pub mod utils;
pub mod websocket;

/// Application configuration
pub mod config {
    use serde::Deserialize;
    
    #[derive(Debug, Deserialize)]
    pub struct Config {
        pub server: ServerConfig,
        pub database: DatabaseConfig,
        pub websocket: WebSocketConfig,
    }
    
    #[derive(Debug, Deserialize)]
    pub struct ServerConfig {
        pub host: String,
        pub port: u16,
    }
    
    #[derive(Debug, Deserialize)]
    pub struct DatabaseConfig {
        pub url: String,
    }
    
    #[derive(Debug, Deserialize)]
    pub struct WebSocketConfig {
        pub ping_interval: u64,
    }
    
    /// Load configuration from file
    pub fn load_config() -> Result<Config, config::ConfigError> {
        let mut settings = config::Config::default();
        
        // Start with default settings
        settings.merge(config::File::with_name("config/default"))?;
        
        // Override with environment-specific settings
        let env = std::env::var("NOAH_ENV").unwrap_or_else(|_| "development".into());
        settings.merge(config::File::with_name(&format!("config/{}", env)).required(false))?;
        
        // Override with environment variables
        settings.merge(config::Environment::with_prefix("NOAH"))?;
        
        settings.try_into()
    }
} 