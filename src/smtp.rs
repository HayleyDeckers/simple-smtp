use core::{
    fmt::Display,
    ops::{Deref, Range},
};

use super::{Error, MalformedError};
use crate::{Buffer, ReadWrite};

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
    Auth,
    Other(&'a str, &'a str),
}

impl Display for Extensions<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Extensions::StartTls => write!(f, "STARTTLS"),
            Extensions::Auth => write!(f, "AUTH"),
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
        if s.eq_ignore_ascii_case("STARTTLS") {
            Extensions::StartTls
        } else if s.eq_ignore_ascii_case("AUTH") {
            Extensions::Auth
        } else if let Some((s, arg)) = s.split_once(' ') {
            Extensions::Other(s, arg)
        } else {
            Extensions::Other(s, "")
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

    pub fn supports(&self, ext: Extensions) -> bool {
        self.extensions().any(|e| e == ext)
    }

    pub fn extensions<'b: 'a>(&'b self) -> impl Iterator<Item = Extensions<'a>> {
        self.reply.lines().skip(1).map(|line| {
            let line = if let Some((line, _)) = line.split_once(' ') {
                line
            } else {
                line
            };
            Extensions::from_str(line)
        })
    }
}
