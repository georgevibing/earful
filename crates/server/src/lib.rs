//! feedback-harvest: tiny self-hosted feedback and bug report collector.

pub mod auth;
pub mod config;
pub mod crypto;
pub mod db;
pub mod embed;
pub mod error;
pub mod models;
pub mod pow;
pub mod ratelimit;
pub mod routes;
pub mod search;
pub mod spam;
pub mod state;
pub mod worker;

pub use routes::build_router;
