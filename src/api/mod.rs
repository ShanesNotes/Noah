//! API module for Noah EHR System
//! 
//! This module contains all API-related functionality.

pub mod routes;
pub mod handlers;
pub mod middleware;

pub use routes::configure; 