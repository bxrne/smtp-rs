//! Internal SMTP modules and shared types.

pub mod error;
pub mod net;

/// Re-exported error types for public API consumers.
pub use error::{Error, Result};
pub use net::{Broker, Session};
