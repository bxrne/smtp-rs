//! SMTP protocol library building blocks.
//!
//! This crate exposes foundational types for parsing SMTP commands and
//! managing session state.

#![forbid(unsafe_code)]

pub mod libsmtp;

pub use libsmtp::{Broker, Session};
pub use libsmtp::{Command, Machine, Mail, Reply, State};
pub use libsmtp::{Error, Result};
pub use libsmtp::{MemoryTransport, NullTransport, Transport};
