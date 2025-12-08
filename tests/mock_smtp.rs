//! Integration tests using a mock SMTP server.
//!
//! We script server responses upfront and capture client writes for verification.
//! No real network required — just pure protocol testing vibes 🎭

use std::{collections::VecDeque, fmt};

use simple_smtp::{Error, Message, MessageDate, ProtocolError, ReadWrite, Smtp};

// ══════════════════════════════════════════════════════════════════════════════
// Mock Error Type
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct MockError(String);

impl MockError {
    pub fn new(msg: impl Into<String>) -> Self {
        MockError(msg.into())
    }
}

impl fmt::Display for MockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MockError: {}", self.0)
    }
}

impl std::error::Error for MockError {}

// ══════════════════════════════════════════════════════════════════════════════
// MockStream - our fake ReadWrite impl
// ══════════════════════════════════════════════════════════════════════════════

/// A mock stream that simulates server responses and captures client writes.
///
/// Queue up responses with `queue_response()`, then let the SMTP client
/// talk to it. Check what was written with `written()`.
pub struct MockStream {
    /// Queued server responses (each Vec<u8> is returned on a read() call)
    responses: VecDeque<Vec<u8>>,
    /// Everything the client has written
    written: Vec<u8>,
    /// If set, the next read/write will return this error
    inject_error: Option<MockError>,
}

impl MockStream {
    pub fn new() -> Self {
        MockStream {
            responses: VecDeque::new(),
            written: Vec::new(),
            inject_error: None,
        }
    }

    /// Queue a raw response to be returned on the next read() call.
    pub fn queue_response(&mut self, data: impl Into<Vec<u8>>) -> &mut Self {
        self.responses.push_back(data.into());
        self
    }

    /// Queue a single-line SMTP response (adds \r\n automatically).
    pub fn queue_line(&mut self, line: &str) -> &mut Self {
        self.queue_response(format!("{}\r\n", line))
    }

    /// Queue a multi-line SMTP response.
    /// Lines are joined with the continuation format (code-text).
    pub fn queue_multiline(&mut self, code: u16, lines: &[&str]) -> &mut Self {
        let mut response = String::new();
        for (i, line) in lines.iter().enumerate() {
            let is_last = i == lines.len() - 1;
            let separator = if is_last { ' ' } else { '-' };
            response.push_str(&format!("{}{}{}\r\n", code, separator, line));
        }
        self.queue_response(response)
    }

    /// Make the next read() return an error.
    pub fn inject_read_error(&mut self, err: MockError) -> &mut Self {
        self.inject_error = Some(err);
        self
    }

    /// Get everything the client has written so far.
    pub fn written(&self) -> &[u8] {
        &self.written
    }

    /// Get written data as a string (panics if not valid UTF-8).
    pub fn written_str(&self) -> &str {
        std::str::from_utf8(&self.written).expect("written data should be valid UTF-8")
    }

    /// Check if a specific command was sent by the client.
    pub fn contains_command(&self, cmd: &str) -> bool {
        self.written_str().contains(cmd)
    }
}

impl Default for MockStream {
    fn default() -> Self {
        Self::new()
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// ReadWrite implementation for MockStream
// ══════════════════════════════════════════════════════════════════════════════

impl ReadWrite for MockStream {
    type Error = MockError;

    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // Check for injected error first
        if let Some(err) = self.inject_error.take() {
            return Err(err);
        }

        // Pop the next queued response
        match self.responses.pop_front() {
            Some(data) => {
                let len = data.len().min(buf.len());
                buf[..len].copy_from_slice(&data[..len]);

                // If we didn't consume all the data, push the rest back
                if len < data.len() {
                    self.responses.push_front(data[len..].to_vec());
                }

                Ok(len)
            }
            None => {
                // No more responses = EOF (0 bytes read)
                Ok(0)
            }
        }
    }

    async fn write_single(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        // Check for injected error
        if let Some(err) = self.inject_error.take() {
            return Err(err);
        }

        self.written.extend_from_slice(buf);
        Ok(())
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Helper functions for common SMTP response patterns
// ══════════════════════════════════════════════════════════════════════════════

/// Create a mock with a standard 220 greeting.
fn mock_with_greeting() -> MockStream {
    let mut mock = MockStream::new();
    mock.queue_line("220 mail.example.com ESMTP ready");
    mock
}

/// Create a mock set up for a basic EHLO exchange.
fn mock_with_ehlo() -> MockStream {
    let mut mock = mock_with_greeting();
    mock.queue_multiline(
        250,
        &[
            "mail.example.com",
            "STARTTLS",
            "AUTH PLAIN LOGIN",
            "SIZE 10485760",
        ],
    );
    mock
}

// ══════════════════════════════════════════════════════════════════════════════
// Tests: Basic Flows
// ══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_ready_greeting() {
    let mock = mock_with_greeting();
    let mut smtp = Smtp::new(mock);

    let ready = smtp.ready().await.expect("ready() should succeed");
    assert_eq!(ready.hostname(), "mail.example.com");
}

#[tokio::test]
async fn test_ehlo_parses_extensions() {
    let mock = mock_with_ehlo();
    let mut smtp = Smtp::new(mock);

    let _ = smtp.ready().await.unwrap();
    let ehlo = smtp
        .ehlo("client.example.com")
        .await
        .expect("ehlo() should succeed");

    // Check we got the extensions
    assert!(ehlo.supports(simple_smtp::smtp::Extensions::StartTls));
    assert!(ehlo.supports(simple_smtp::smtp::Extensions::Auth("PLAIN")));
    assert!(ehlo.supports(simple_smtp::smtp::Extensions::Auth("LOGIN")));
    assert!(!ehlo.supports(simple_smtp::smtp::Extensions::Auth("CRAM-MD5")));

    // Check the client sent the right command
    let (stream, _) = smtp.into_inner();
    assert!(stream.contains_command("EHLO client.example.com\r\n"));
}

#[tokio::test]
async fn test_starttls_command() {
    let mut mock = mock_with_ehlo();
    // Queue STARTTLS response
    mock.queue_line("220 Ready to start TLS");

    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client.example.com").await.unwrap();

    let reply = smtp.starttls().await.expect("starttls() should succeed");
    assert_eq!(reply.code(), 220);

    let (stream, _) = smtp.into_inner();
    assert!(stream.contains_command("STARTTLS\r\n"));
}

#[tokio::test]
async fn test_auth_plain() {
    let mut mock = mock_with_ehlo();
    // Queue successful AUTH response
    mock.queue_line("235 Authentication successful");

    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client.example.com").await.unwrap();

    let reply = smtp
        .auth("user@example.com", "hunter2")
        .await
        .expect("auth() should succeed");
    assert_eq!(reply.code(), 235);

    let (stream, _) = smtp.into_inner();
    // Should have sent AUTH PLAIN with base64 payload
    assert!(stream.contains_command("AUTH PLAIN "));
}

#[tokio::test]
async fn test_send_mail_flow() {
    let mut mock = mock_with_ehlo();
    // MAIL FROM response
    mock.queue_line("250 OK");
    // RCPT TO response
    mock.queue_line("250 OK");
    // DATA response
    mock.queue_line("354 Start mail input");
    // End of data response
    mock.queue_line("250 OK: queued as 12345");

    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client.example.com").await.unwrap();

    smtp.send_mail(
        "sender@example.com",
        ["recipient@example.com"].iter(),
        b"Subject: Test\r\n\r\nHello!",
    )
    .await
    .expect("send_mail() should succeed");

    let (stream, _) = smtp.into_inner();
    let written = stream.written_str();

    assert!(written.contains("MAIL FROM:<sender@example.com>\r\n"));
    assert!(written.contains("RCPT TO:<recipient@example.com>\r\n"));
    assert!(written.contains("DATA\r\n"));
    assert!(written.contains("Subject: Test\r\n\r\nHello!"));
    assert!(written.contains("\r\n.\r\n")); // End of data marker
}

#[tokio::test]
async fn test_quit() {
    let mut mock = mock_with_ehlo();
    mock.queue_line("221 Bye");

    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client.example.com").await.unwrap();

    let reply = smtp.quit().await.expect("quit() should succeed");
    assert_eq!(reply.code(), 221);

    let (stream, _) = smtp.into_inner();
    assert!(stream.contains_command("QUIT\r\n"));
}

#[tokio::test]
async fn test_full_happy_path() {
    // The whole enchilada: greeting -> EHLO -> AUTH -> MAIL -> QUIT
    let mut mock = MockStream::new();
    mock.queue_line("220 mail.example.com ESMTP");
    mock.queue_multiline(250, &["mail.example.com", "AUTH PLAIN"]);
    mock.queue_line("235 Authenticated");
    mock.queue_line("250 OK"); // MAIL FROM
    mock.queue_line("250 OK"); // RCPT TO
    mock.queue_line("354 Go ahead");
    mock.queue_line("250 Queued");
    mock.queue_line("221 Bye");

    let mut smtp = Smtp::new(mock);

    // Full flow
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client.local").await.unwrap();
    let _ = smtp.auth("me", "secret").await.unwrap();
    smtp.send_mail("me@local", ["you@remote"].iter(), b"hi")
        .await
        .unwrap();
    let _ = smtp.quit().await.unwrap();

    let (stream, _) = smtp.into_inner();
    let written = stream.written_str();

    // Verify the conversation order
    let ehlo_pos = written.find("EHLO").unwrap();
    let auth_pos = written.find("AUTH").unwrap();
    let mail_pos = written.find("MAIL FROM").unwrap();
    let quit_pos = written.find("QUIT").unwrap();

    assert!(ehlo_pos < auth_pos, "EHLO should come before AUTH");
    assert!(auth_pos < mail_pos, "AUTH should come before MAIL FROM");
    assert!(mail_pos < quit_pos, "MAIL FROM should come before QUIT");
}

// ══════════════════════════════════════════════════════════════════════════════
// Tests: Error Recovery
// ══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_auth_rejected() {
    let mut mock = mock_with_ehlo();
    // Server says no 😤
    mock.queue_line("535 Authentication failed");

    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client.example.com").await.unwrap();

    let result = smtp.auth("baduser", "wrongpass").await;
    assert!(result.is_err(), "auth() should fail on 535");
}

#[tokio::test]
async fn test_mail_from_rejected() {
    let mut mock = mock_with_ehlo();
    // Sender not allowed
    mock.queue_line("550 Sender rejected");

    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client.example.com").await.unwrap();

    let result = smtp
        .send_mail("spam@bad.com", ["victim@example.com"].iter(), b"spam")
        .await;
    assert!(result.is_err(), "send_mail() should fail on 550");
}

#[tokio::test]
async fn test_rcpt_to_rejected() {
    let mut mock = mock_with_ehlo();
    mock.queue_line("250 OK"); // MAIL FROM succeeds
    mock.queue_line("550 User unknown"); // RCPT TO fails

    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client.example.com").await.unwrap();

    let result = smtp
        .send_mail("sender@ok.com", ["nonexistent@example.com"].iter(), b"hi")
        .await;
    assert!(
        result.is_err(),
        "send_mail() should fail if RCPT TO rejected"
    );
}

#[tokio::test]
async fn test_unexpected_eof() {
    // Server just... disappears 💀
    let mut mock = MockStream::new();
    mock.queue_line("220 mail.example.com ESMTP");
    // No EHLO response queued — EOF will happen

    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();

    let result = smtp.ehlo("client.local").await;
    assert!(result.is_err(), "should error on unexpected EOF");
}

#[tokio::test]
async fn test_malformed_no_code() {
    // Server sends garbage without a status code
    let mut mock = MockStream::new();
    mock.queue_response(b"Hello I am a broken server\r\n".to_vec());

    let mut smtp = Smtp::new(mock);
    let result = smtp.ready().await;
    assert!(result.is_err(), "should error on malformed response");
}

#[tokio::test]
async fn test_malformed_bad_line_terminator() {
    // Server uses bare LF instead of CRLF 🙄
    let mut mock = MockStream::new();
    mock.queue_response(b"220 mail.example.com ESMTP\n".to_vec());

    let mut smtp = Smtp::new(mock);
    let result = smtp.ready().await;
    assert!(result.is_err(), "should error on bare LF");
}

#[tokio::test]
async fn test_io_error_propagates() {
    let mut mock = MockStream::new();
    mock.inject_read_error(MockError::new("connection reset by peer"));

    let mut smtp = Smtp::new(mock);
    let result = smtp.ready().await;

    assert!(result.is_err(), "IO error should propagate");
}

#[tokio::test]
async fn test_wrong_greeting_code() {
    // Server is temporarily unavailable
    let mut mock = MockStream::new();
    mock.queue_line("421 Service not available");

    let mut smtp = Smtp::new(mock);
    let result = smtp.ready().await;

    assert!(result.is_err(), "ready() should fail on non-220 code");
}

// ══════════════════════════════════════════════════════════════════════════════
// Tests: RFC 5322 Message + Dot-Stuffing
// ══════════════════════════════════════════════════════════════════════════════

/// Helper to create a mock set up for a full send_message flow.
fn mock_for_send_message() -> MockStream {
    let mut mock = mock_with_ehlo();
    mock.queue_line("250 OK"); // MAIL FROM
    mock.queue_line("250 OK"); // RCPT TO
    mock.queue_line("354 Go ahead"); // DATA
    mock.queue_line("250 Queued"); // End of data
    mock
}

#[tokio::test]
async fn test_send_message_dot_stuffs_body_starting_with_dot() {
    let date = MessageDate::utc(2025, 1, 1, 12, 0, 0).unwrap();
    let msg = Message::new(date, "from@test.com", "id@test.com")
        .with_body(".This line starts with a dot");

    let mock = mock_for_send_message();
    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client").await.unwrap();

    smtp.send_message(&msg, "from@test.com", ["to@test.com"].iter())
        .await
        .expect("send_message should succeed");

    let (stream, _) = smtp.into_inner();
    let written = stream.written_str();
    // Body should have extra dot: "..This line starts with a dot"
    assert!(
        written.contains("..This line starts with a dot"),
        "Body not properly dot-stuffed. Written:\n{}",
        written
    );
}

#[tokio::test]
async fn test_send_message_dot_stuffs_line_starting_with_dot() {
    let date = MessageDate::utc(2025, 1, 1, 12, 0, 0).unwrap();
    let msg =
        Message::new(date, "from@test.com", "id@test.com").with_body("Hello\r\n.Hidden\r\nWorld");

    let mock = mock_for_send_message();
    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client").await.unwrap();

    smtp.send_message(&msg, "from@test.com", ["to@test.com"].iter())
        .await
        .expect("send_message should succeed");

    let (stream, _) = smtp.into_inner();
    let written = stream.written_str();
    // The line ".Hidden" should become "..Hidden"
    assert!(
        written.contains("\r\n..Hidden\r\n"),
        "Line not properly dot-stuffed. Written:\n{}",
        written
    );
}

#[tokio::test]
async fn test_send_message_dot_stuffs_lone_dot_line() {
    // This is the critical case - a line with just "." would end the message early 💀
    let date = MessageDate::utc(2025, 1, 1, 12, 0, 0).unwrap();
    let msg = Message::new(date, "from@test.com", "id@test.com").with_body("Before\r\n.\r\nAfter");

    let mock = mock_for_send_message();
    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client").await.unwrap();

    smtp.send_message(&msg, "from@test.com", ["to@test.com"].iter())
        .await
        .expect("send_message should succeed");

    let (stream, _) = smtp.into_inner();
    let written = stream.written_str();
    // The lone "." line should become ".."
    assert!(
        written.contains("\r\n..\r\n"),
        "Lone dot not properly stuffed. Written:\n{}",
        written
    );
    // And "After" should still be in the message
    assert!(
        written.contains("After"),
        "Text after dot was lost. Written:\n{}",
        written
    );
}

#[tokio::test]
async fn test_send_message_no_stuffing_needed() {
    let date = MessageDate::utc(2025, 1, 1, 12, 0, 0).unwrap();
    let msg = Message::new(date, "from@test.com", "id@test.com")
        .with_body("Just a normal message.\r\nNo dots at line starts.");

    let mock = mock_for_send_message();
    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client").await.unwrap();

    smtp.send_message(&msg, "from@test.com", ["to@test.com"].iter())
        .await
        .expect("send_message should succeed");

    let (stream, _) = smtp.into_inner();
    let written = stream.written_str();
    // Body should be unchanged
    assert!(written.contains("Just a normal message.\r\nNo dots at line starts."));
    // No double dots should appear
    assert!(!written.contains(".."));
}

#[tokio::test]
async fn test_send_message_rejects_header_with_crlf() {
    // Subject contains \r\n which could allow header injection 🦹
    let date = MessageDate::utc(2025, 1, 1, 12, 0, 0).unwrap();
    let msg = Message::new(date, "from@test.com", "id@test.com")
        .with_subject("Hello\r\nX-Injected: evil");

    let mock = mock_for_send_message();
    let mut smtp = Smtp::new(mock);
    let _ = smtp.ready().await.unwrap();
    let _ = smtp.ehlo("client").await.unwrap();

    let result = smtp
        .send_message(&msg, "from@test.com", ["to@test.com"].iter())
        .await;

    // Should fail with InvalidHeader error
    assert!(result.is_err());
    let err = result.unwrap_err();
    match err {
        Error::ProtocolError(ProtocolError::InvalidHeader(name)) => {
            assert_eq!(name, "Subject");
        }
        _ => panic!("Expected InvalidHeader error, got {:?}", err),
    }
}

#[tokio::test]
async fn test_send_message_rejects_from_with_crlf() {
    let date = MessageDate::utc(2025, 1, 1, 12, 0, 0).unwrap();
    let msg = Message::new(date, "from@test.com\r\nX-Bad: header", "id@test.com");

    let mock = MockStream::new(); // No responses needed, we fail before sending
    let mut smtp = Smtp::new(mock);

    let result = smtp
        .send_message(&msg, "from@test.com", ["to@test.com"].iter())
        .await;

    assert!(matches!(
        result,
        Err(Error::ProtocolError(ProtocolError::InvalidHeader("From")))
    ));
}
