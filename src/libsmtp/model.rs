//! SMTP protocol state machine, commands, and replies. Transport layer agnostic.

use std::fmt;

/// Session states per RFC 821 §4.1.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Just connected; greeting not yet sent.
    Connecting,
    /// Greeting sent, awaiting `HELO`/`EHLO`.
    Greeted,
    /// `HELO` accepted, awaiting `MAIL`.
    Helo,
    /// `MAIL` accepted, awaiting `RCPT`.
    Mail,
    /// At least one `RCPT` accepted, awaiting more `RCPT` or `DATA`.
    Rcpt,
    /// Inside `DATA` payload, reading body until a lone `.` line.
    Data,
    /// Connection should be closed.
    Closed,
}

/// A parsed SMTP command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Helo(String),
    Ehlo(String),
    Mail(String),
    Rcpt(String),
    Data,
    Rset,
    Noop,
    Quit,
    Vrfy(String),
    Expn(String),
    Help(Option<String>),
    /// Verb that did not match any known command.
    Unknown(String),
}

impl Command {
    /// Parse a single command line (CRLF stripping is handled).
    pub fn parse(line: &str) -> Self {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let verb = parts.next().unwrap_or("").to_ascii_uppercase();
        let arg = parts.next().unwrap_or("").trim();
        match verb.as_str() {
            "HELO" => Command::Helo(arg.to_string()),
            "EHLO" => Command::Ehlo(arg.to_string()),
            "MAIL" => Command::Mail(strip_path_prefix(arg, "FROM:")),
            "RCPT" => Command::Rcpt(strip_path_prefix(arg, "TO:")),
            "DATA" => Command::Data,
            "RSET" => Command::Rset,
            "NOOP" => Command::Noop,
            "QUIT" => Command::Quit,
            "VRFY" => Command::Vrfy(arg.to_string()),
            "EXPN" => Command::Expn(arg.to_string()),
            "HELP" => Command::Help(if arg.is_empty() {
                None
            } else {
                Some(arg.to_string())
            }),
            other => Command::Unknown(other.to_string()),
        }
    }
}

fn strip_path_prefix(arg: &str, prefix: &str) -> String {
    if arg.len() >= prefix.len() && arg[..prefix.len()].eq_ignore_ascii_case(prefix) {
        arg[prefix.len()..].trim().to_string()
    } else {
        arg.to_string()
    }
}

/// An SMTP reply: a 3-digit status code and a textual message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub code: u16,
    pub text: String,
}

impl Reply {
    pub fn new(code: u16, text: impl Into<String>) -> Self {
        Reply {
            code,
            text: text.into(),
        }
    }

    /// On-the-wire format including trailing CRLF.
    pub fn format(&self) -> String {
        format!("{} {}\r\n", self.code, self.text)
    }
}

impl fmt::Display for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code, self.text)
    }
}

/// Accumulated state for one mail transaction.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Mail {
    pub from: String,
    pub to: Vec<String>,
    pub body: String,
}

/// SMTP session finite state machine.
#[derive(Debug)]
pub struct Machine {
    state: State,
    pending: Mail,
    /// Last successfully delivered transaction, if any. Useful for tests
    /// and as an extension hook (e.g. handing the mail off to a queue).
    pub last: Option<Mail>,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    /// Create a new machine
    pub fn new() -> Self {
        Machine {
            state: State::Connecting,
            pending: Mail::default(),
            last: None,
        }
    }

    /// Current protocol state.
    pub fn state(&self) -> State {
        self.state
    }

    /// True once a QUIT has been received and the transport should close.
    pub fn is_closed(&self) -> bool {
        self.state == State::Closed
    }

    /// Produce the initial 220 greeting and advance.
    pub fn greet(&mut self) -> Reply {
        self.state = State::Greeted;
        Reply::new(220, "Service ready")
    }

    /// Drive the machine with one line from the client.
    ///
    /// Returns None for intermediate DATA body lines (which receive no
    /// reply per RFC 821) and Some(reply) otherwise.
    pub fn step(&mut self, line: &str) -> Option<Reply> {
        if self.state == State::Data {
            return self.handle_data_line(line);
        }
        Some(self.handle_command(Command::parse(line)))
    }

    fn handle_data_line(&mut self, line: &str) -> Option<Reply> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "." {
            let mail = std::mem::take(&mut self.pending);
            self.last = Some(mail);
            self.state = State::Helo;
            return Some(Reply::new(250, "OK message accepted"));
        }
        // RFC 821 §4.5.2 transparency: leading "." is stripped.
        let payload = trimmed.strip_prefix('.').unwrap_or(trimmed);
        self.pending.body.push_str(payload);
        self.pending.body.push_str("\r\n");
        None
    }

    fn handle_command(&mut self, cmd: Command) -> Reply {
        use Command::*;
        match cmd {
            Helo(domain) | Ehlo(domain) => {
                if domain.is_empty() {
                    return Reply::new(501, "Syntax: HELO <domain>");
                }
                self.pending = Default::default();
                self.state = State::Helo;
                Reply::new(250, format!("Hello {}", domain))
            }
            Mail(path) => match self.state {
                State::Helo => {
                    self.pending.from = path;
                    self.state = State::Mail;
                    Reply::new(250, "OK")
                }
                _ => Reply::new(503, "Bad sequence of commands"),
            },
            Rcpt(path) => match self.state {
                State::Mail | State::Rcpt => {
                    self.pending.to.push(path);
                    self.state = State::Rcpt;
                    Reply::new(250, "OK")
                }
                _ => Reply::new(503, "Bad sequence of commands"),
            },
            Data => match self.state {
                State::Rcpt => {
                    self.state = State::Data;
                    Reply::new(354, "Start mail input; end with <CRLF>.<CRLF>")
                }
                _ => Reply::new(503, "Bad sequence of commands"),
            },
            Rset => {
                self.pending = Default::default();
                if self.state != State::Greeted {
                    self.state = State::Helo;
                }
                Reply::new(250, "OK")
            }
            Noop => Reply::new(250, "OK"),
            Quit => {
                self.state = State::Closed;
                Reply::new(221, "Service closing transmission channel")
            }
            Vrfy(_) | Expn(_) => Reply::new(252, "Cannot VRFY user, but will accept message"),
            Help(_) => Reply::new(214, "RFC 821 SMTP"),
            Unknown(verb) => Reply::new(500, format!("Unrecognized command: {}", verb)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // GIVEN known SMTP verbs WHEN parsed THEN they map to the right Command variant
    #[test]
    fn test_command_parse_known_verbs() {
        assert_eq!(
            Command::parse("HELO example.com\r\n"),
            Command::Helo("example.com".to_string())
        );
        assert_eq!(
            Command::parse("ehlo example.com"),
            Command::Ehlo("example.com".to_string())
        );
        assert_eq!(
            Command::parse("MAIL FROM:<a@b>"),
            Command::Mail("<a@b>".to_string())
        );
        assert_eq!(
            Command::parse("RCPT TO:<c@d>"),
            Command::Rcpt("<c@d>".to_string())
        );
        assert_eq!(Command::parse("DATA"), Command::Data);
        assert_eq!(Command::parse("RSET"), Command::Rset);
        assert_eq!(Command::parse("NOOP"), Command::Noop);
        assert_eq!(Command::parse("QUIT"), Command::Quit);
        assert_eq!(Command::parse("HELP"), Command::Help(None));
        assert_eq!(
            Command::parse("HELP MAIL"),
            Command::Help(Some("MAIL".to_string()))
        );
    }

    // GIVEN an unknown verb WHEN parsed THEN it returns Command::Unknown
    #[test]
    fn test_command_parse_unknown_verb() {
        assert_eq!(
            Command::parse("FOOBAR baz"),
            Command::Unknown("FOOBAR".to_string())
        );
    }

    // GIVEN a Reply WHEN formatted THEN it includes the code, text, and CRLF
    #[test]
    fn test_reply_format() {
        let reply = Reply::new(250, "OK");
        assert_eq!(reply.format(), "250 OK\r\n");
    }

    // GIVEN a fresh machine WHEN greeted THEN it advances to Greeted and replies 220
    #[test]
    fn test_machine_greet_transitions_to_greeted() {
        let mut m = Machine::new();
        assert_eq!(m.state(), State::Connecting);
        let reply = m.greet();
        assert_eq!(reply, Reply::new(220, "Service ready"));
        assert_eq!(m.state(), State::Greeted);
    }

    // GIVEN a greeted machine WHEN MAIL arrives before HELO THEN it returns 503
    #[test]
    fn test_machine_mail_before_helo_is_bad_sequence() {
        let mut m = Machine::new();
        m.greet();
        let reply = m.step("MAIL FROM:<a@b>").expect("reply");
        assert_eq!(reply.code, 503);
    }

    // GIVEN a HELO with no domain WHEN parsed THEN the machine returns 501
    #[test]
    fn test_machine_helo_requires_domain() {
        let mut m = Machine::new();
        m.greet();
        let reply = m.step("HELO").expect("reply");
        assert_eq!(reply.code, 501);
    }

    // GIVEN a full HELO/MAIL/RCPT/DATA/QUIT exchange WHEN driven through the machine
    // THEN state transitions are correct and the mail is captured
    #[test]
    fn test_machine_full_transaction() {
        let mut m = Machine::new();
        assert_eq!(m.greet().code, 220);

        assert_eq!(m.step("HELO client.example").unwrap().code, 250);
        assert_eq!(m.state(), State::Helo);

        assert_eq!(m.step("MAIL FROM:<from@example>").unwrap().code, 250);
        assert_eq!(m.state(), State::Mail);

        assert_eq!(m.step("RCPT TO:<a@example>").unwrap().code, 250);
        assert_eq!(m.step("RCPT TO:<b@example>").unwrap().code, 250);
        assert_eq!(m.state(), State::Rcpt);

        assert_eq!(m.step("DATA").unwrap().code, 354);
        assert_eq!(m.state(), State::Data);

        assert!(m.step("Subject: hi").is_none());
        assert!(m.step("").is_none());
        assert!(m.step("hello world").is_none());
        // Transparency: a leading "." is stripped.
        assert!(m.step("..dotted").is_none());

        let end = m.step(".").expect("end-of-data reply");
        assert_eq!(end.code, 250);
        assert_eq!(m.state(), State::Helo);

        let mail = m.last.as_ref().expect("mail captured");
        assert_eq!(mail.from, "<from@example>");
        assert_eq!(mail.to, vec!["<a@example>", "<b@example>"]);
        assert_eq!(mail.body, "Subject: hi\r\n\r\nhello world\r\n.dotted\r\n");

        let bye = m.step("QUIT").expect("quit reply");
        assert_eq!(bye.code, 221);
        assert!(m.is_closed());
    }

    // GIVEN a transaction in progress WHEN RSET is issued THEN pending mail is cleared
    #[test]
    fn test_machine_rset_clears_pending_mail() {
        let mut m = Machine::new();
        m.greet();
        m.step("HELO x").unwrap();
        m.step("MAIL FROM:<a@b>").unwrap();
        m.step("RCPT TO:<c@d>").unwrap();
        assert_eq!(m.state(), State::Rcpt);

        let reply = m.step("RSET").expect("reply");
        assert_eq!(reply.code, 250);
        assert_eq!(m.state(), State::Helo);

        // A fresh MAIL must succeed and not carry over old recipients.
        m.step("MAIL FROM:<e@f>").unwrap();
        assert_eq!(m.state(), State::Mail);
    }

    // GIVEN any state WHEN an unknown verb arrives THEN the machine returns 500
    #[test]
    fn test_machine_unknown_command() {
        let mut m = Machine::new();
        m.greet();
        let reply = m.step("FOOBAR").expect("reply");
        assert_eq!(reply.code, 500);
    }
}
