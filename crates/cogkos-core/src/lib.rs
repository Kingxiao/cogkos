//! CogKOS Core - Data models and domain logic

pub mod audit;
pub mod authority;
pub mod benchmark;
pub mod config_reload;
pub mod encryption;
pub mod errors;
pub mod evolution;
pub mod health;
pub mod models;
pub mod monitoring;
pub mod rbac;
pub mod retry;
pub mod security;
pub mod transactional_memory;

// Explicitly handle Result to avoid ambiguity
pub use authority::*;
pub use benchmark::*;
pub use config_reload::*;
pub use encryption::*;
pub use errors::{CogKosError, Result};
pub use models::*;
pub use rbac::*;
pub use retry::*;
pub use security::SecurityMode;
pub use transactional_memory::*;
