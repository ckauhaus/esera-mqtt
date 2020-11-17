use crate::parser;

use async_trait::async_trait;
use std::fmt;
use std::net::ToSocketAddrs;
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::prelude::*;

const POLLTIME: u64 = 25;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Transport(#[from] std::io::Error),
    #[error("While parsing controller response {1:?}: {0}")]
    Parse(#[source] crate::parser::Error, String),
    #[error("Controller connection closed while waiting for response")]
    Disconnected,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct Connection<S: fmt::Debug + Send> {
    inner: S,
    queue: String,
}

impl Connection<TcpStream> {
    pub async fn new<A: ToSocketAddrs>(addr: A) -> std::io::Result<Self> {
        let addr: Vec<_> = addr.to_socket_addrs()?.collect();
        info!("Connecting to {:?}", addr);
        Ok(Self::from_stream(TcpStream::connect(&*addr).await?))
    }
}

impl<C: fmt::Debug + AsyncWrite + AsyncRead + Unpin + Send> Connection<C> {
    pub fn from_stream(stream: C) -> Self {
        Self {
            inner: stream,
            queue: String::with_capacity(1 << 12),
        }
    }

    pub async fn next(&mut self) -> Result<parser::Response> {
        loop {
            if !self.buf().is_empty() {
                match parser::parse(self.buf()) {
                    Ok((rem, mtch)) => {
                        let rem = rem.len();
                        // cut out match from queue buffer
                        self.cut_range(0, self.buf().len() - rem);
                        return Ok(mtch);
                    }
                    Err(nom::Err::Error(e)) => {
                        warn!("Cannot parse {:?}: {}", self.buf(), e);
                    }
                    Err(nom::Err::Failure(e)) => {
                        error!("Failed to parse {:?}: {}", self.buf(), e);
                        let line_ending =
                            self.buf().find('\n').map(|pos| pos + 1).unwrap_or_default();
                        self.cut_range(0, line_ending);
                    }
                    Err(nom::Err::Incomplete(_)) => (),
                }
            }
            tokio::time::delay_for(Duration::from_millis(POLLTIME)).await;
            self.fill_buf().await?;
        }
    }
}

#[async_trait]
pub trait Controller {
    async fn send_line(&mut self, line: &str) -> Result<()>;

    fn buf(&self) -> &str;

    async fn fill_buf(&mut self) -> Result<()>;

    fn cut_range(&mut self, from: usize, to: usize);

    fn line_beginnings<'a>(&'a self) -> Box<dyn Iterator<Item = usize> + 'a>;
}

#[async_trait]
impl<S> Controller for Connection<S>
where
    S: AsyncWrite + AsyncRead + Unpin + fmt::Debug + Send,
{
    async fn send_line(&mut self, line: &str) -> Result<()> {
        debug!("<<< {}", line);
        self.inner.write(line.as_bytes()).await?;
        if !line.ends_with('\n') {
            self.inner.write_u8(b'\n').await?;
        }
        Ok(())
    }

    #[inline]
    fn buf(&self) -> &str {
        &self.queue
    }

    // XXX replace self.queue
    async fn fill_buf(&mut self) -> Result<()> {
        let mut receive = [0; 1 << 12];
        let n = self.inner.read(&mut receive).await?;
        if n == 0 {
            return Err(Error::Disconnected);
        }
        let read = String::from_utf8_lossy(&receive[..n]);
        if let std::borrow::Cow::Owned(_) = read {
            warn!("Non-UTF8 data read from controller: {:?}", read);
        }
        debug!(">>> {}", read.trim());
        self.queue.push_str(&read);
        Ok(())
    }

    fn cut_range(&mut self, start: usize, end: usize) {
        self.queue.replace_range(std::ops::Range { start, end }, "");
    }

    fn line_beginnings<'a>(&'a self) -> Box<dyn Iterator<Item = usize> + 'a> {
        Box::new(std::iter::once(0).chain(self.queue.match_indices('\n').map(|(n, _)| n + 1)))
    }
}

/// Waits until the specified response type is found in the stream.
pub async fn pick<C, F, R>(ctrl: &mut C, parse_fn: F) -> Result<R>
where
    C: Controller + Send + ?Sized,
    R: std::fmt::Debug + Send,
    for<'a> F: Fn(&'a str) -> parser::PResult<'a, R> + Send,
{
    loop {
        // Search for a match on each line beginning.
        // We cannot just iterate over splitlines since a pattern may span multiple lines.
        let mut found = None;
        for offset in ctrl.line_beginnings() {
            if offset < ctrl.buf().len() {
                if let Ok((rem, mtch)) = parse_fn(&ctrl.buf()[offset..]) {
                    found = Some((mtch, offset, rem.len()));
                    break;
                }
            }
        }
        if let Some((mtch, offset, rem)) = found {
            ctrl.cut_range(offset, ctrl.buf().len() - rem);
            return Ok(mtch);
        } else {
            tokio::time::delay_for(Duration::from_millis(POLLTIME)).await;
            ctrl.fill_buf().await?;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser;

    use assert_matches::assert_matches;
    use bstr::B;
    use std::io::Cursor;

    #[tokio::test]
    async fn wait_on_closed_reader_should_fail() {
        let mut c = Connection::from_stream(Cursor::new(B("").to_vec()));
        assert_matches!(pick(&mut c, parser::kal).await, Err(Error::Disconnected));
    }

    #[tokio::test]
    async fn wait_should_return_match() {
        let mut c = Connection::from_stream(Cursor::new(B("1_KAL|1\n").to_vec()));
        assert!(pick(&mut c, parser::kal).await.is_ok());
        assert_eq!(c.queue, "");
    }

    #[tokio::test]
    async fn wait_should_cut_out_match() {
        let mut c = Connection::from_stream(Cursor::new(
            B("1_KAL|1\n\
               1_DATE|07.11.20\n\
               1_DATAPRINT|1\n")
            .to_vec(),
        ));
        assert_eq!(pick(&mut c, parser::date).await.unwrap(), "07.11.20");
        assert_eq!(
            c.queue,
            "1_KAL|1\n\
             1_DATAPRINT|1\n"
        );
    }
}
