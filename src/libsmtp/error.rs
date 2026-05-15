//! Error and Result types

use std::fmt;
use std::result;

/// Custom error type for the SMTP library.
#[derive(Debug)]
pub enum Error {
    /// Represents an error when parsing a command.
    ParseError(String),
    /// Represents an error when handling a command.
    CommandError(String),
    /// Represents an error when generating a reply.
    ReplyError(String),
    /// Represents an error when managing session state.
    SessionError(String),
    /// Represents an unknown error.
    Unknown(String),
}

// User friendly error messages
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ParseError(msg) => write!(f, "Parse Error: {}", msg),
            Error::CommandError(msg) => write!(f, "Command Error: {}", msg),
            Error::ReplyError(msg) => write!(f, "Reply Error: {}", msg),
            Error::SessionError(msg) => write!(f, "Session Error: {}", msg),
            Error::Unknown(msg) => write!(f, "Unknown Error: {}", msg),
        }
    }
}

/// Alias for Result type using custom Error.
pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create all error variants for testing
    fn collect_all_err_variants() -> Vec<Error> {
        vec![
            Error::ParseError("Invalid command format".to_string()),
            Error::CommandError("Unknown command".to_string()),
            Error::ReplyError("Failed to generate reply".to_string()),
            Error::SessionError("Session state error".to_string()),
            Error::Unknown("An unknown error occurred".to_string()),
        ]
    }

    // GIVEN all error variants WHEN formatted as a string THEN they should display the correct
    // error message
    #[test]
    fn test_all_err_display() {
        let errs = collect_all_err_variants();

        let expected_messages = vec![
            "Parse Error: Invalid command format",
            "Command Error: Unknown command",
            "Reply Error: Failed to generate reply",
            "Session Error: Session state error",
            "Unknown Error: An unknown error occurred",
        ];

        for (err, expected) in errs.iter().zip(expected_messages.iter()) {
            assert_eq!(format!("{}", err), *expected);
        }
    }

    // GIVEN all error variants WHEN wrapped in a Result THEN they should be recognized as errors
    #[test]
    fn test_all_err_result() {
        let errs: Vec<Error> = collect_all_err_variants();

        for err in errs {
            let result: Result<()> = Err(err);
            assert!(result.is_err());
        }
    }
}
