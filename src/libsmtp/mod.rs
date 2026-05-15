//! Internal SMTP modules and shared types.

pub mod error;
pub mod model;
pub mod session_broker;
pub mod transport;

/// Re-exported error types for public API consumers.
pub use error::{Error, Result};
pub use model::{Command, Machine, Mail, Reply, State};
pub use session_broker::{Broker, Session};
pub use transport::{MemoryTransport, NullTransport, Transport};
