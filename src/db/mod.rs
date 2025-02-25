//! Database module for Noah EHR System
//! 
//! This module handles database connections and operations.

use sqlx::PgPool;
use std::sync::Arc;

pub mod migrations;
pub mod queries;

/// Database connection pool
pub struct Database {
    pool: Arc<PgPool>,
}

impl Database {
    /// Create a new database connection
    pub async fn connect(connection_string: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(connection_string).await?;
        Ok(Self {
            pool: Arc::new(pool),
        })
    }
    
    /// Get a reference to the connection pool
    pub fn pool(&self) -> Arc<PgPool> {
        self.pool.clone()
    }
    
    /// Run database migrations
    pub async fn run_migrations(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(self.pool.as_ref()).await
    }
} 