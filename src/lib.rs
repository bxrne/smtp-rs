//! SMTP protocol library building blocks.
//!
//! This crate exposes foundational types for parsing SMTP commands and
//! managing session state.

#![forbid(unsafe_code)]

pub mod libsmtp;

pub use libsmtp::{Error, Result};
