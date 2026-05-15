//! Transport abstractions for accepted SMTP messages.

use crate::{Mail, Result};
use std::sync::{Arc, Mutex};

/// Transport for delivering accepted mail to a queue, spool, or downstream service.
pub trait Transport: Send + Sync {
    fn deliver(&self, mail: Mail) -> Result<()>;
}

/// A transport that discards all mail (default).
#[derive(Debug, Default)]
pub struct NullTransport;

impl Transport for NullTransport {
    fn deliver(&self, _mail: Mail) -> Result<()> {
        Ok(())
    }
}

/// In-memory transport useful for tests or demos.
#[derive(Clone, Debug, Default)]
pub struct MemoryTransport {
    inner: Arc<Mutex<Vec<Mail>>>,
}

impl MemoryTransport {
    /// Take all stored mails.
    pub fn take(&self) -> Vec<Mail> {
        std::mem::take(&mut self.inner.lock().expect("memory transport lock"))
    }
}

impl Transport for MemoryTransport {
    fn deliver(&self, mail: Mail) -> Result<()> {
        self.inner.lock().expect("memory transport lock").push(mail);
        Ok(())
    }
}
