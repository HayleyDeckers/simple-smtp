use core::{
    fmt::Display,
    ops::{Deref, Range},
};

use super::{Error, MalformedError, ProtocolError};
use crate::{Buffer, ReadWrite, message::Message};

#[derive(Debug)]
pub struct ReplyLine<'a> {
    code: u16,
    is_last: bool,
    message: &'a str,
}

impl<'a> ReplyLine<'a> {
    pub fn code(&self) -> u16 {
        self.code
    }
    pub fn is_last(&self) -> bool {
        self.is_last
    }
    pub fn message(&self) -> &'a str {
        self.message
    }
}

impl Display for ReplyLine<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}{}{}",
            self.code,
            if self.is_last { ' ' } else { '-' },
            self.message
        )
    }
}

// every line recieved, if vallid starts with 4 bytes [0..3] code and [3] space or dash
// and ends with \r\n.
// that's 4 bytes at the head of the buffer we can use to store data
// and 6 bytes everywhere else.
// if we use the first 16 bits for a u16 code, we can use the next 16 bits to store the size of the line.
// and replace the \r\n with the size of the next line
// that way we don't have to reparse the codes and we can just use the size of the line to iterate instead of
// finding the \r\n. And we could use the last 16 bits to store the total line count
#[derive(Copy, Clone)]
pub struct Reply<'a> {
    code: u16,
    message_len: u16,
    remaining_buffer: &'a [u8],
}

impl<'a> Iterator for Reply<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining_buffer.is_empty() {
            return None;
        }
        let (this, next) = self.remaining_buffer.split_at(self.message_len as usize);
        if next.len() < 6 {
            self.remaining_buffer = &[];
            self.message_len = 0;
        } else {
            // after our message, we first have the required \r\n line terminator
            // then we have 4 bytes for the next code and continuation marker, then the real message starts
            self.remaining_buffer = &next[6..];
            // but we sneakily stored the length of the next message in the two bytes
            // directly preceding it
            self.message_len = u16::from_ne_bytes([next[4], next[5]]);
        }
        Some(core::str::from_utf8(this).expect("should already be validated as utf-8"))
    }
}

impl<'a> Reply<'a> {
    pub fn code(&self) -> u16 {
        self.code
    }

    pub fn lines(&self) -> impl Iterator<Item = &str> {
        *self
    }

    pub fn replies(&self) -> impl Iterator<Item = ReplyLine<'_>> {
        // let n_lines = self.lines().count();
        let end = self.remaining_buffer.as_ptr_range().end as usize;
        self.map(move |line| {
            let my_end = line.as_bytes().as_ptr_range().end as usize;
            ReplyLine {
                code: self.code,
                is_last: end - my_end < 2,
                message: line,
            }
        })
    }

    fn from_buffer(buffer: &[u8]) -> Reply<'_> {
        if buffer.len() < 4 {
            panic!("Buffer too small");
        }
        let code = u16::from_ne_bytes([buffer[0], buffer[1]]);
        let message_len = u16::from_ne_bytes([buffer[2], buffer[3]]);
        let remaining_buffer = &buffer[4..];
        if remaining_buffer.len() < message_len as usize {
            panic!("Buffer too small");
        }
        Reply {
            code,
            message_len,
            remaining_buffer,
        }
    }

    pub fn current_line(self) -> &'a str {
        core::str::from_utf8(&self.remaining_buffer[..self.message_len as usize])
            .expect("should already be validated as utf-8")
    }
}

pub struct Smtp<'a, T: ReadWrite> {
    // the underlying stream, e.g. TcpStream or TlsStream
    stream: T,
    // holds the multi-line reply from the server
    buf: Buffer<'a>,
    // total size of the buffer, including the last \r\n
    // filled: usize,
    // the range of the buffer which has not been processed yet
    buf_unprocessed: Range<usize>,
}

#[cfg(feature = "alloc")]
impl<T: ReadWrite<Error = impl core::error::Error>> Smtp<'static, T> {
    pub fn new(stream: T) -> Self {
        Self::new_with_buffer(stream, vec![0; 1024])
    }
}

impl<'buffer, T: ReadWrite<Error = impl core::error::Error>> Smtp<'buffer, T> {
    async fn fill_buffer(&mut self) -> Result<(), Error<T::Error>> {
        let start_from = self.buf_unprocessed.end;
        let n_bytes = self
            .stream
            .read(&mut self.buf[start_from..])
            .await
            .map_err(Error::IoError)?;

        if n_bytes == 0 {
            return Err(MalformedError::UnexpectedEof.into());
        }
        self.buf_unprocessed.end += n_bytes;
        Ok(())
    }

    async fn fill_to_atleast(&mut self, n: usize) -> Result<(), Error<T::Error>> {
        while self.buf_unprocessed.len() < n {
            self.fill_buffer().await?;
        }
        Ok(())
    }

    async fn consume(&mut self, n: usize) -> Result<&[u8], Error<T::Error>> {
        self.fill_to_atleast(n).await?;
        let old_range = self.buf_unprocessed.clone();
        self.buf_unprocessed.start += n;
        Ok(&self.buf[old_range.start..old_range.start + n])
    }

    // returns a range instead of a slice because of lifetime issues
    fn buffer_contains_terminator(&mut self) -> Result<Option<Range<usize>>, Error<T::Error>> {
        let buf = &self.buf[self.buf_unprocessed.clone()];
        let mut iter = buf.iter().enumerate();
        while let Some((idx, char)) = iter.next() {
            if *char == b'\r' {
                match iter.next() {
                    Some((_, b'\n')) => {
                        let range = self.buf_unprocessed.start..self.buf_unprocessed.start + idx;
                        self.buf_unprocessed.start += idx + 2;
                        return Ok(Some(range));
                    }
                    Some(_) => {
                        return Err(Error::MalformedError(
                            MalformedError::InvalidLineTermination,
                        ));
                    }
                    None => {
                        return Ok(None);
                    }
                }
            }
            if *char == b'\n' {
                return Err(Error::MalformedError(
                    MalformedError::InvalidLineTermination,
                ));
            }
        }
        Ok(None)
    }

    async fn consume_until_newline_and_write_len_header(
        &mut self,
    ) -> Result<&[u8], Error<T::Error>> {
        loop {
            match self.buffer_contains_terminator()? {
                Some(msg) => {
                    let len = msg.len();
                    // todo: error here
                    // we need to copy the message length into the buffer
                    // we only call this _after_ we have found the code, so we can safely
                    // widen the range and include some extra bytes
                    self.buf[msg.start - 2..msg.start]
                        .copy_from_slice(&u16::to_ne_bytes(len as u16));
                    return Ok(&self.buf[msg]);
                }
                None => self.fill_buffer().await?,
            }
        }
    }

    /// reads a single line from the server.
    pub async fn read_line(&mut self) -> Result<ReplyLine<'_>, Error<T::Error>> {
        let Ok(Ok(code)) = core::str::from_utf8(self.consume(3).await?).map(|s| s.parse::<u16>())
        else {
            return Err(Error::MalformedError(MalformedError::NoCode));
        };
        let is_last = match self.consume(1).await?[0] {
            b' ' => true,
            b'-' => false,
            _ => {
                //todo: wrong error message
                return Err(Error::MalformedError(MalformedError::InvalidEncoding));
            }
        };
        // now we need to find the line terminator
        let message_bytes = self.consume_until_newline_and_write_len_header().await?;
        let message = core::str::from_utf8(message_bytes)
            .map_err(|_| Error::MalformedError(MalformedError::InvalidEncoding))?;
        let reply = ReplyLine {
            code,
            is_last,
            message,
        };
        #[cfg(feature = "log-04")]
        log::debug!("s>{reply}");
        Ok(reply)
    }

    pub async fn read_multiline_reply(&mut self) -> Result<Reply<'_>, Error<T::Error>> {
        self.buf_unprocessed = 0..0;
        let reply = self.read_line().await?;
        let expected_code = reply.code();
        let mut is_last = reply.is_last();
        while !is_last {
            let reply = self.read_line().await?;
            //we double parse here,
            if reply.code() != expected_code {
                return Err(Error::MalformedError(MalformedError::CodeChanged {
                    old_code: expected_code,
                    new_code: reply.code(),
                }));
            }
            is_last = reply.is_last();
        }
        self.buf[0..2].copy_from_slice(&u16::to_ne_bytes(expected_code));
        let all_replies = &self.buf[..self.buf_unprocessed.start];
        Ok(Reply::from_buffer(all_replies))
    }

    pub fn new_with_buffer(stream: T, buffer: impl Into<Buffer<'buffer>>) -> Self {
        Smtp {
            buf: buffer.into(),
            stream,
            buf_unprocessed: 0..0,
        }
    }

    pub async fn send_data<'s>(&'s mut self, data: &[u8]) -> Result<Reply<'s>, Error<T::Error>> {
        #[cfg(feature = "log-04")]
        log::debug!("c>[{} bytes of data]<CR><LF>.<CR><LF>", data.len());
        // send the data
        self.stream
            .write_multi(&[data, b"\r\n.\r\n"])
            .await
            .map_err(Error::IoError)?;
        // read the reply
        self.read_multiline_reply().await
    }

    pub fn into_inner(self) -> (T, Buffer<'buffer>) {
        (self.stream, self.buf)
    }

    pub async fn ready(&mut self) -> Result<Ready<'_>, Error<T::Error>> {
        // wait for the server to be ready
        let reply = self.read_multiline_reply().await?;
        // 220 or 554 are expected
        if reply.code != 220 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[220],
                actual: reply.code(),
            }));
        }
        Ok(Ready::new(reply))
    }

    pub async fn ehlo(&mut self, domain: &str) -> Result<EhloResponse<'_>, Error<T::Error>> {
        #[cfg(feature = "log-04")]
        log::debug!("c>EHLO {}", domain);
        self.stream
            .write_multi(&[b"EHLO ", domain.as_bytes(), b"\r\n"])
            .await
            .map_err(Error::IoError)?;
        let reply = self.read_multiline_reply().await?;
        // or 504, 550, 502
        if reply.code != 250 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[250],
                actual: reply.code(),
            }));
        }
        Ok(EhloResponse::new(reply))
    }

    pub async fn starttls(&mut self) -> Result<Reply<'_>, Error<T::Error>> {
        #[cfg(feature = "log-04")]
        log::debug!("c>STARTTLS");
        self.stream
            .write_single(b"STARTTLS\r\n")
            .await
            .map_err(Error::IoError)?;
        let reply = self.read_multiline_reply().await?;
        // 220 or 554 are expected
        if reply.code != 220 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[220],
                actual: reply.code(),
            }));
        }
        Ok(reply)
    }

    pub async fn auth(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<Reply<'_>, Error<T::Error>> {
        use base64::prelude::*;
        #[cfg(feature = "log-04")]
        log::debug!("c>AUTH PLAIN [censored]");

        // since we have to base64 encode w/o allocating
        // we will use the read buffer to store the base64 encoded data.
        // but there's no api for "encode this slice append x, append y, append z"
        // so we first have to make the data contiguous...
        // let's use the same buffer again for now. Ideally we should write some kind of streaming
        // base64 encoder which we can call with a slice of slices
        let payload = {
            self.buf[0] = 0;
            self.buf[1..1 + username.len()].copy_from_slice(username.as_bytes());
            self.buf[1 + username.len()] = 0;
            self.buf[username.len() + 2..username.len() + 2 + password.len()]
                .copy_from_slice(password.as_bytes());
            let (read, write) = self.buf.split_at_mut(username.len() + 2 + password.len());
            let bytes = BASE64_STANDARD.encode_slice(read, write).unwrap();
            &write[..bytes]
        };
        //if we can allocate, use just do it.
        // let payload = BASE64_STANDARD.encode(format!("\0{}\0{}", username, password));
        self.stream
            .write_multi(&[b"AUTH PLAIN ", payload, b"\r\n"])
            .await
            .map_err(Error::IoError)?;
        let reply = self.read_multiline_reply().await?;
        // 235 or 554 are expected
        if reply.code != 235 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[235],
                actual: reply.code(),
            }));
        }
        Ok(reply)
    }

    pub async fn quit(&mut self) -> Result<Reply<'_>, Error<T::Error>> {
        self.fast_quit().await?;
        let reply = self.read_multiline_reply().await?;
        // 221 or 554 are expected
        if reply.code != 221 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[221],
                actual: reply.code(),
            }));
        }
        Ok(reply)
    }

    pub async fn fast_quit(&mut self) -> Result<(), Error<T::Error>> {
        #[cfg(feature = "log-04")]
        log::debug!("c>QUIT");
        self.stream
            .write_single(b"QUIT\r\n")
            .await
            .map_err(Error::IoError)?;
        Ok(())
    }

    pub async fn send_mail(
        &mut self,
        from: impl AsRef<str>,
        to: impl Iterator<Item = impl AsRef<str>>,
        data: &[u8], //nice to have: streaming data for memory constrained devices
    ) -> Result<(), Error<T::Error>> {
        #[cfg(feature = "log-04")]
        log::debug!("c>MAIL FROM: <{}>", from.as_ref());
        self.stream
            .write_multi(&[b"MAIL FROM:<", from.as_ref().as_bytes(), b">\r\n"])
            .await
            .map_err(Error::IoError)?;
        let reply = self.read_multiline_reply().await?;
        // 250 or 554 are expected
        if reply.code != 250 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[250],
                actual: reply.code(),
            }));
        }

        // now we need to send the recipients
        for recipient in to {
            #[cfg(feature = "log-04")]
            log::debug!("c>RCPT TO: <{}>", recipient.as_ref());
            self.stream
                .write_multi(&[b"RCPT TO:<", recipient.as_ref().as_bytes(), b">\r\n"])
                .await
                .map_err(Error::IoError)?;
            let reply = self.read_multiline_reply().await?;

            // 250 or 554 are expected
            if reply.code != 250 {
                return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                    expected: &[250],
                    actual: reply.code(),
                }));
            }
        }
        #[cfg(feature = "log-04")]
        log::debug!("c>DATA");
        self.stream
            .write_single(b"DATA\r\n")
            .await
            .map_err(Error::IoError)?;
        let reply = self.read_multiline_reply().await?;
        // 354 or 554 are expected
        if reply.code != 354 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[354],
                actual: reply.code(),
            }));
        }
        let reply = self.send_data(data).await?;
        // 250 or 554 are expected
        if reply.code != 250 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[250],
                actual: reply.code(),
            }));
        }
        Ok(())
    }

    /// Send a Message by streaming directly to the connection.
    ///
    /// The envelope addresses (from, to) are separate from the message headers.
    /// This is how SMTP works - the envelope is for routing, headers are for display.
    /// While they're usually the same, they don't have to be (e.g., mailing lists).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let msg = Message::new(
    ///         MessageDate::now(),
    ///         "Me <me@example.com>",
    ///         "unique-id@example.com",
    ///     )
    ///     .with_to("you@example.com")
    ///     .with_subject("Hello!")
    ///     .with_body("Hi there!");
    ///
    /// smtp.send_message(
    ///     &msg,
    ///     "me@example.com",           // envelope from (bare address)
    ///     ["you@example.com"].iter(), // envelope to
    /// ).await?;
    /// ```
    pub async fn send_message(
        &mut self,
        message: &Message<'_>,
        envelope_from: &str,
        envelope_to: impl Iterator<Item = impl AsRef<str>>,
    ) -> Result<(), Error<T::Error>> {
        // Validate headers don't contain bare \r\n (injection prevention)
        fn check_header(value: &str, name: &'static str) -> Result<(), ProtocolError> {
            if value.contains("\r\n") {
                return Err(ProtocolError::InvalidHeader(name));
            }
            Ok(())
        }

        check_header(message.from(), "From")?;
        check_header(message.message_id(), "Message-ID")?;
        if let Some(to) = message.to() {
            check_header(to, "To")?;
        }
        if let Some(cc) = message.cc() {
            check_header(cc, "Cc")?;
        }
        if let Some(bcc) = message.bcc() {
            check_header(bcc, "Bcc")?;
        }
        if let Some(reply_to) = message.reply_to() {
            check_header(reply_to, "Reply-To")?;
        }
        if let Some(subject) = message.subject() {
            check_header(subject, "Subject")?;
        }
        if let Some(irt) = message.in_reply_to() {
            check_header(irt, "In-Reply-To")?;
        }
        if let Some(refs) = message.references() {
            check_header(refs, "References")?;
        }

        // Send envelope
        #[cfg(feature = "log-04")]
        log::debug!("c>MAIL FROM: <{}>", envelope_from);
        self.stream
            .write_multi(&[b"MAIL FROM:<", envelope_from.as_bytes(), b">\r\n"])
            .await
            .map_err(Error::IoError)?;
        let reply = self.read_multiline_reply().await?;
        if reply.code != 250 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[250],
                actual: reply.code(),
            }));
        }

        for recipient in envelope_to {
            #[cfg(feature = "log-04")]
            log::debug!("c>RCPT TO: <{}>", recipient.as_ref());
            self.stream
                .write_multi(&[b"RCPT TO:<", recipient.as_ref().as_bytes(), b">\r\n"])
                .await
                .map_err(Error::IoError)?;
            let reply = self.read_multiline_reply().await?;
            if reply.code != 250 {
                return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                    expected: &[250],
                    actual: reply.code(),
                }));
            }
        }

        #[cfg(feature = "log-04")]
        log::debug!("c>DATA");
        self.stream
            .write_single(b"DATA\r\n")
            .await
            .map_err(Error::IoError)?;
        let reply = self.read_multiline_reply().await?;
        if reply.code != 354 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[354],
                actual: reply.code(),
            }));
        }

        // Stream message headers directly - no buffer needed except for date
        // RFC 2822 dates are max ~31 bytes, e.g. "Wed, 01 Jan 2025 12:00:00 +0000"
        let mut date_buf = [0u8; 40];
        let date_len = {
            use core::fmt::Write;
            // Tiny inline writer - just tracks position while writing to a slice
            struct SliceWriter<'a>(&'a mut [u8], usize);
            impl core::fmt::Write for SliceWriter<'_> {
                fn write_str(&mut self, s: &str) -> core::fmt::Result {
                    let b = s.as_bytes();
                    let end = self.1 + b.len();
                    if end > self.0.len() {
                        return Err(core::fmt::Error);
                    }
                    self.0[self.1..end].copy_from_slice(b);
                    self.1 = end;
                    Ok(())
                }
            }
            let mut w = SliceWriter(&mut date_buf, 0);
            let _ = write!(w, "{}", message.date());
            w.1
        };
        self.stream
            .write_multi(&[b"Date: ", &date_buf[..date_len], b"\r\n"])
            .await
            .map_err(Error::IoError)?;

        self.stream
            .write_multi(&[b"From: ", message.from().as_bytes(), b"\r\n"])
            .await
            .map_err(Error::IoError)?;

        self.stream
            .write_multi(&[b"Message-ID: <", message.message_id().as_bytes(), b">\r\n"])
            .await
            .map_err(Error::IoError)?;

        if let Some(to) = message.to() {
            self.stream
                .write_multi(&[b"To: ", to.as_bytes(), b"\r\n"])
                .await
                .map_err(Error::IoError)?;
        }

        if let Some(cc) = message.cc() {
            self.stream
                .write_multi(&[b"Cc: ", cc.as_bytes(), b"\r\n"])
                .await
                .map_err(Error::IoError)?;
        }

        if let Some(bcc) = message.bcc() {
            self.stream
                .write_multi(&[b"Bcc: ", bcc.as_bytes(), b"\r\n"])
                .await
                .map_err(Error::IoError)?;
        }

        if let Some(reply_to) = message.reply_to() {
            self.stream
                .write_multi(&[b"Reply-To: ", reply_to.as_bytes(), b"\r\n"])
                .await
                .map_err(Error::IoError)?;
        }

        if let Some(subject) = message.subject() {
            self.stream
                .write_multi(&[b"Subject: ", subject.as_bytes(), b"\r\n"])
                .await
                .map_err(Error::IoError)?;
        }

        if let Some(irt) = message.in_reply_to() {
            self.stream
                .write_multi(&[b"In-Reply-To: ", irt.as_bytes(), b"\r\n"])
                .await
                .map_err(Error::IoError)?;
        }

        if let Some(refs) = message.references() {
            self.stream
                .write_multi(&[b"References: ", refs.as_bytes(), b"\r\n"])
                .await
                .map_err(Error::IoError)?;
        }

        // Blank line before body
        self.stream
            .write_single(b"\r\n")
            .await
            .map_err(Error::IoError)?;

        // Write body with dot-stuffing (RFC 5321 §4.5.2)
        // Any line starting with '.' gets an extra '.' prepended
        // We do this without allocation by writing chunks
        if let Some(body) = message.body() {
            let body = body.as_bytes();

            // Handle body starting with a dot
            if body.starts_with(b".") {
                self.stream
                    .write_single(b".")
                    .await
                    .map_err(Error::IoError)?;
            }

            let mut pos = 0;
            while pos < body.len() {
                // Find next \r\n. sequence (line starting with dot)
                if let Some(rel_idx) = find_crlf_dot(&body[pos..]) {
                    // Write up to and including the \r\n
                    let crlf_end = pos + rel_idx + 2;
                    self.stream
                        .write_single(&body[pos..crlf_end])
                        .await
                        .map_err(Error::IoError)?;
                    // Write extra dot (the stuffing)
                    self.stream
                        .write_single(b".")
                        .await
                        .map_err(Error::IoError)?;
                    // Continue from the original dot
                    pos = crlf_end;
                } else {
                    // No more \r\n. sequences, write the rest
                    self.stream
                        .write_single(&body[pos..])
                        .await
                        .map_err(Error::IoError)?;
                    break;
                }
            }
        }

        // End with \r\n.\r\n
        self.stream
            .write_single(b"\r\n.\r\n")
            .await
            .map_err(Error::IoError)?;

        let reply = self.read_multiline_reply().await?;
        if reply.code != 250 {
            return Err(Error::MalformedError(MalformedError::UnexpectedCode {
                expected: &[250],
                actual: reply.code(),
            }));
        }
        Ok(())
    }
}

/// Find the position of \r\n. in a byte slice (returns position of \r)
fn find_crlf_dot(data: &[u8]) -> Option<usize> {
    // We need at least 3 bytes for \r\n.
    if data.len() < 3 {
        return None;
    }
    data.windows(3).position(|w| w == b"\r\n.")
}

pub struct Ready<'a> {
    hostname: &'a str,
    reply: Reply<'a>,
}
impl<'a> Ready<'a> {
    pub fn new(reply: Reply<'a>) -> Self {
        let first_line = reply.current_line();
        let hostname = if let Some((hostname, _)) = first_line.split_once(' ') {
            hostname
        } else {
            first_line
        };
        // todo checks on hostname format
        // todo check protocol (ESMTP, SMTP)
        Ready { hostname, reply }
    }

    pub fn hostname(&self) -> &'a str {
        self.hostname
    }
    // pub fn message(&self) -> Option<&'a str> {
    //     (!self.message.is_empty()).then_some(self.message)
    // }
}

impl<'a> Deref for Ready<'a> {
    type Target = Reply<'a>;
    fn deref(&self) -> &Self::Target {
        &self.reply
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extensions<'a> {
    StartTls,
    /// AUTH extension with supported mechanisms (e.g., "PLAIN LOGIN")
    Auth(&'a str),
    Other(&'a str, &'a str),
}

impl Display for Extensions<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Extensions::StartTls => write!(f, "STARTTLS"),
            Extensions::Auth(mechanisms) => {
                if mechanisms.is_empty() {
                    write!(f, "AUTH")
                } else {
                    write!(f, "AUTH {mechanisms}")
                }
            }
            Extensions::Other(s, arg) => {
                if arg.is_empty() {
                    write!(f, "{s}")
                } else {
                    write!(f, "{s} {arg}")
                }
            }
        }
    }
}
impl Extensions<'_> {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Extensions<'_> {
        // Split to get keyword and args separately
        let (keyword, args) = s.split_once(' ').unwrap_or((s, ""));

        if keyword.eq_ignore_ascii_case("STARTTLS") {
            // RFC 3207 defines STARTTLS with no parameters.
            // We accept args gracefully (Postel's law) but warn about it.
            // <https://datatracker.ietf.org/doc/html/rfc3207#section-4>
            #[cfg(feature = "log-04")]
            if !args.is_empty() {
                log::warn!("STARTTLS with unexpected arguments: {args:?}");
            }
            Extensions::StartTls
        } else if keyword.eq_ignore_ascii_case("AUTH") {
            // RFC 4954 Section 3: AUTH should have a space-separated list of
            // supported SASL mechanisms. Empty mechanisms is technically weird.
            // <https://datatracker.ietf.org/doc/html/rfc4954#section-3>
            #[cfg(feature = "log-04")]
            if args.is_empty() {
                log::warn!("AUTH extension with no mechanisms advertised");
            }
            Extensions::Auth(args)
        } else {
            Extensions::Other(keyword, args)
        }
    }
}

pub struct EhloResponse<'a> {
    // todo: bitfield of known extensions
    reply: Reply<'a>,
}
impl<'a> Deref for EhloResponse<'a> {
    type Target = Reply<'a>;
    fn deref(&self) -> &Self::Target {
        &self.reply
    }
}

impl<'a> EhloResponse<'a> {
    pub fn new(reply: Reply<'a>) -> Self {
        EhloResponse { reply }
    }

    /// Check if the server supports an extension.
    ///
    /// For `Auth`, you can pass:
    /// - `Auth("")` to check if AUTH is supported at all
    /// - `Auth("PLAIN")` to check if a specific mechanism is supported
    pub fn supports(&self, ext: Extensions) -> bool {
        self.extensions().any(|e| match (&e, &ext) {
            // For AUTH, special handling: check if the requested mechanism
            // is in the server's list (or if we're just checking for any AUTH)
            (Extensions::Auth(server_mechs), Extensions::Auth(wanted)) => {
                if wanted.is_empty() {
                    // Auth("") means "does the server support AUTH at all?"
                    true
                } else {
                    // Check if the wanted mechanism is in the server's list
                    server_mechs
                        .split_whitespace()
                        .any(|m| m.eq_ignore_ascii_case(wanted))
                }
            }
            // Everything else uses structural equality
            _ => e == ext,
        })
    }

    pub fn extensions<'b: 'a>(&'b self) -> impl Iterator<Item = Extensions<'a>> {
        // Pass the full line to from_str - it handles keyword/args splitting
        self.reply.lines().skip(1).map(Extensions::from_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to build a buffer in the format Reply::from_buffer expects.
    // Format: [code: u16][msg_len: u16][message bytes...]
    fn build_single_line_buffer(code: u16, message: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&code.to_ne_bytes());
        buf.extend_from_slice(&(message.len() as u16).to_ne_bytes());
        buf.extend_from_slice(message.as_bytes());
        buf
    }

    // Helper to build a multi-line reply buffer.
    // Each line after the first is prefixed with \r\n + 4 bytes (code+marker),
    // but bytes 4-5 of that prefix store the next message length.
    fn build_multiline_buffer(code: u16, messages: &[&str]) -> Vec<u8> {
        let mut buf = Vec::new();
        // First line header
        buf.extend_from_slice(&code.to_ne_bytes());
        buf.extend_from_slice(&(messages[0].len() as u16).to_ne_bytes());
        buf.extend_from_slice(messages[0].as_bytes());

        for msg in &messages[1..] {
            // \r\n terminator for previous line
            buf.extend_from_slice(b"\r\n");
            // 4 bytes: first 2 are code (we don't care in from_buffer),
            // next 2 are the length of *this* message
            buf.extend_from_slice(&code.to_ne_bytes()); // placeholder for code bytes
            buf.extend_from_slice(&(msg.len() as u16).to_ne_bytes());
            buf.extend_from_slice(msg.as_bytes());
        }
        buf
    }

    // ══════════════════════════════════════════════════════════════════════════
    // Reply::from_buffer tests
    // ══════════════════════════════════════════════════════════════════════════

    #[test]
    fn reply_from_buffer_single_line() {
        let buf = build_single_line_buffer(250, "OK");
        let reply = Reply::from_buffer(&buf);

        assert_eq!(reply.code(), 250);
        assert_eq!(reply.current_line(), "OK");
    }

    #[test]
    fn reply_from_buffer_empty_message() {
        let buf = build_single_line_buffer(220, "");
        let reply = Reply::from_buffer(&buf);

        assert_eq!(reply.code(), 220);
        assert_eq!(reply.current_line(), "");
    }

    #[test]
    fn reply_from_buffer_long_message() {
        let long_msg = "a".repeat(200);
        let buf = build_single_line_buffer(354, &long_msg);
        let reply = Reply::from_buffer(&buf);

        assert_eq!(reply.code(), 354);
        assert_eq!(reply.current_line(), long_msg);
    }

    #[test]
    #[should_panic(expected = "Buffer too small")]
    fn reply_from_buffer_too_small_header() {
        let buf = vec![0, 0, 0];
        let _ = Reply::from_buffer(&buf);
    }

    #[test]
    #[should_panic(expected = "Buffer too small")]
    fn reply_from_buffer_message_len_exceeds_buffer() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&250u16.to_ne_bytes());
        buf.extend_from_slice(&10u16.to_ne_bytes()); // claims 10 bytes
        buf.extend_from_slice(b"hi"); // only 2 bytes
        let _ = Reply::from_buffer(&buf);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // Reply iterator tests
    // ══════════════════════════════════════════════════════════════════════════

    #[test]
    fn reply_iterator_single_line() {
        let buf = build_single_line_buffer(250, "mail.example.com");
        let reply = Reply::from_buffer(&buf);

        let lines: Vec<_> = reply.lines().collect();
        assert_eq!(lines, vec!["mail.example.com"]);
    }

    #[test]
    fn reply_iterator_multiline() {
        let buf =
            build_multiline_buffer(250, &["mail.example.com", "STARTTLS", "AUTH PLAIN LOGIN"]);
        let reply = Reply::from_buffer(&buf);

        let lines: Vec<_> = reply.lines().collect();
        assert_eq!(
            lines,
            vec!["mail.example.com", "STARTTLS", "AUTH PLAIN LOGIN"]
        );
    }

    /// RFC 5321 Section 4.2 doesn't mandate non-empty text after the code,
    /// so some servers send empty extension lines. We handle this gracefully.
    /// <https://datatracker.ietf.org/doc/html/rfc5321#section-4.2>
    #[test]
    fn reply_iterator_empty_lines() {
        let buf = build_multiline_buffer(250, &["host", "", "SIZE 1000"]);
        let reply = Reply::from_buffer(&buf);

        let lines: Vec<_> = reply.lines().collect();
        assert_eq!(lines, vec!["host", "", "SIZE 1000"]);
    }

    #[test]
    fn reply_code_accessor() {
        let buf = build_single_line_buffer(421, "Service not available");
        let reply = Reply::from_buffer(&buf);
        assert_eq!(reply.code(), 421);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // Extensions::from_str tests
    // ══════════════════════════════════════════════════════════════════════════

    /// RFC 5321 Section 2.4: "Verbs and argument values (e.g., 'TO:' or 'to:'
    /// in the RCPT command and extension name keywords) are not case sensitive"
    /// <https://datatracker.ietf.org/doc/html/rfc5321#section-2.4>
    #[test]
    fn extensions_case_insensitive() {
        // STARTTLS in various cases
        assert_eq!(Extensions::from_str("STARTTLS"), Extensions::StartTls);
        assert_eq!(Extensions::from_str("starttls"), Extensions::StartTls);
        assert_eq!(Extensions::from_str("StartTls"), Extensions::StartTls);

        // AUTH in various cases (with empty mechanisms)
        assert_eq!(Extensions::from_str("AUTH"), Extensions::Auth(""));
        assert_eq!(Extensions::from_str("auth"), Extensions::Auth(""));
        assert_eq!(Extensions::from_str("AuTh"), Extensions::Auth(""));
    }

    #[test]
    fn extensions_auth_with_mechanisms() {
        // Server advertises AUTH with PLAIN and LOGIN mechanisms
        assert_eq!(
            Extensions::from_str("AUTH PLAIN LOGIN"),
            Extensions::Auth("PLAIN LOGIN")
        );

        // Case insensitive keyword, mechanisms preserved as-is
        assert_eq!(
            Extensions::from_str("auth PLAIN CRAM-MD5"),
            Extensions::Auth("PLAIN CRAM-MD5")
        );
    }

    #[test]
    fn extensions_other_no_args() {
        assert_eq!(
            Extensions::from_str("PIPELINING"),
            Extensions::Other("PIPELINING", "")
        );
    }

    #[test]
    fn extensions_other_with_args() {
        assert_eq!(
            Extensions::from_str("SIZE 10485760"),
            Extensions::Other("SIZE", "10485760")
        );
    }

    #[test]
    fn extensions_8bitmime() {
        assert_eq!(
            Extensions::from_str("8BITMIME"),
            Extensions::Other("8BITMIME", "")
        );
    }

    #[test]
    fn extensions_empty_string() {
        assert_eq!(Extensions::from_str(""), Extensions::Other("", ""));
    }

    // ══════════════════════════════════════════════════════════════════════════
    // ReplyLine tests
    // ══════════════════════════════════════════════════════════════════════════

    #[test]
    fn replyline_accessors() {
        let line = ReplyLine {
            code: 250,
            is_last: false,
            message: "STARTTLS",
        };

        assert_eq!(line.code(), 250);
        assert!(!line.is_last());
        assert_eq!(line.message(), "STARTTLS");
    }

    #[test]
    fn replyline_display_continuation() {
        let line = ReplyLine {
            code: 250,
            is_last: false,
            message: "mail.example.com",
        };
        assert_eq!(format!("{}", line), "250-mail.example.com");
    }

    #[test]
    fn replyline_display_final() {
        let line = ReplyLine {
            code: 250,
            is_last: true,
            message: "OK",
        };
        assert_eq!(format!("{}", line), "250 OK");
    }

    #[test]
    fn replyline_display_empty_message() {
        let line = ReplyLine {
            code: 220,
            is_last: true,
            message: "",
        };
        assert_eq!(format!("{}", line), "220 ");
    }

    #[test]
    fn replyline_display_various_codes() {
        let cases = [
            (220, "Service ready"),
            (250, "OK"),
            (354, "Start mail input"),
            (421, "Service not available"),
            (550, "Mailbox unavailable"),
        ];

        for (code, msg) in cases {
            let line = ReplyLine {
                code,
                is_last: true,
                message: msg,
            };
            assert_eq!(format!("{}", line), format!("{} {}", code, msg));
        }
    }

    // ══════════════════════════════════════════════════════════════════════════
    // Extensions Display tests
    // ══════════════════════════════════════════════════════════════════════════

    #[test]
    fn extensions_display_starttls() {
        assert_eq!(format!("{}", Extensions::StartTls), "STARTTLS");
    }

    #[test]
    fn extensions_display_auth_no_mechanisms() {
        assert_eq!(format!("{}", Extensions::Auth("")), "AUTH");
    }

    #[test]
    fn extensions_display_auth_with_mechanisms() {
        assert_eq!(
            format!("{}", Extensions::Auth("PLAIN LOGIN")),
            "AUTH PLAIN LOGIN"
        );
    }

    #[test]
    fn extensions_display_other_no_arg() {
        let ext = Extensions::Other("PIPELINING", "");
        assert_eq!(format!("{}", ext), "PIPELINING");
    }

    #[test]
    fn extensions_display_other_with_arg() {
        let ext = Extensions::Other("SIZE", "10485760");
        assert_eq!(format!("{}", ext), "SIZE 10485760");
    }

    // ══════════════════════════════════════════════════════════════════════════
    // MalformedError Display tests
    // ══════════════════════════════════════════════════════════════════════════

    #[test]
    fn malformed_error_display_invalid_line_termination() {
        let err = MalformedError::InvalidLineTermination;
        assert_eq!(format!("{}", err), "Invalid line termination");
    }

    #[test]
    fn malformed_error_display_invalid_encoding() {
        let err = MalformedError::InvalidEncoding;
        assert_eq!(format!("{}", err), "Invalid encoding");
    }

    #[test]
    fn malformed_error_display_no_code() {
        let err = MalformedError::NoCode;
        assert_eq!(format!("{}", err), "No code");
    }

    #[test]
    fn malformed_error_display_unexpected_code() {
        let err = MalformedError::UnexpectedCode {
            expected: &[250, 251],
            actual: 550,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("550"));
        assert!(msg.contains("250"));
        assert!(msg.contains("251"));
    }

    #[test]
    fn malformed_error_display_code_changed() {
        let err = MalformedError::CodeChanged {
            old_code: 250,
            new_code: 354,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("250"));
        assert!(msg.contains("354"));
    }

    #[test]
    fn malformed_error_display_unexpected_eof() {
        let err = MalformedError::UnexpectedEof;
        assert_eq!(format!("{}", err), "Unexpected EOF reached");
    }

    // ══════════════════════════════════════════════════════════════════════════
    // Reply::replies() tests (ReplyLine iterator)
    // ══════════════════════════════════════════════════════════════════════════

    #[test]
    fn reply_replies_single_line() {
        let buf = build_single_line_buffer(250, "OK");
        let reply = Reply::from_buffer(&buf);

        let lines: Vec<_> = reply.replies().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].code(), 250);
        assert_eq!(lines[0].message(), "OK");
        assert!(lines[0].is_last());
    }

    #[test]
    fn reply_replies_multiline_is_last_flags() {
        let buf = build_multiline_buffer(250, &["host.example.com", "STARTTLS", "SIZE 1000"]);
        let reply = Reply::from_buffer(&buf);

        let lines: Vec<_> = reply.replies().collect();
        assert_eq!(lines.len(), 3);

        // First two should NOT be last
        assert!(!lines[0].is_last());
        assert!(!lines[1].is_last());
        // Last one should be last
        assert!(lines[2].is_last());

        // All should have same code
        for line in &lines {
            assert_eq!(line.code(), 250);
        }
    }

    // ══════════════════════════════════════════════════════════════════════════
    // EhloResponse::supports() tests
    // ══════════════════════════════════════════════════════════════════════════

    #[test]
    fn ehlo_supports_starttls() {
        let buf = build_multiline_buffer(250, &["mail.example.com", "STARTTLS", "SIZE 1000"]);
        let reply = Reply::from_buffer(&buf);
        let ehlo = EhloResponse::new(reply);

        assert!(ehlo.supports(Extensions::StartTls));
        assert!(!ehlo.supports(Extensions::Auth("")));
    }

    #[test]
    fn ehlo_supports_auth_any() {
        // When checking Auth(""), we're asking "does the server support AUTH at all?"
        let buf = build_multiline_buffer(250, &["mail.example.com", "AUTH PLAIN LOGIN"]);
        let reply = Reply::from_buffer(&buf);
        let ehlo = EhloResponse::new(reply);

        // Should return true for Auth("") meaning "any AUTH"
        assert!(ehlo.supports(Extensions::Auth("")));
    }

    #[test]
    fn ehlo_supports_auth_specific_mechanism() {
        // Server advertises AUTH PLAIN LOGIN
        let buf = build_multiline_buffer(250, &["mail.example.com", "AUTH PLAIN LOGIN"]);
        let reply = Reply::from_buffer(&buf);
        let ehlo = EhloResponse::new(reply);

        // Should be able to check for specific mechanisms
        assert!(ehlo.supports(Extensions::Auth("PLAIN")));
        assert!(ehlo.supports(Extensions::Auth("LOGIN")));
        assert!(!ehlo.supports(Extensions::Auth("CRAM-MD5")));
    }
}
