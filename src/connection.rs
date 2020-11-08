use crate::parser::PResult;

use std::fmt;
use std::net::ToSocketAddrs;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::prelude::*;

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
pub struct Connection<S: fmt::Debug> {
    inner: S,
    queue: String,
    buf: [u8; 1 << 12],
}

impl Connection<TcpStream> {
    pub async fn new<A: ToSocketAddrs>(addr: A) -> std::io::Result<Self> {
        let addr: Vec<_> = addr.to_socket_addrs()?.collect();
        info!("Connecting to {:?}", addr);
        Ok(Self::from_stream(TcpStream::connect(&*addr).await?))
    }
}

impl<C: fmt::Debug> Connection<C> {
    pub fn from_stream(stream: C) -> Self {
        Self {
            inner: stream,
            queue: String::with_capacity(1 << 12),
            buf: [0; 1 << 12],
        }
    }
}

impl<C: AsyncWrite + Unpin + fmt::Debug> Connection<C> {
    pub async fn send_line<S: AsRef<str>>(&mut self, line: S) -> Result<()> {
        let line = line.as_ref();
        debug!(">>> {}", line);
        self.inner.write(line.as_bytes()).await?;
        if !line.ends_with("\n") {
            self.inner.write_u8(b'\n').await?;
        }
        Ok(())
    }
}

impl<C: AsyncRead + Unpin + fmt::Debug> Connection<C> {
    pub async fn receive(&mut self) -> Result<()> {
        let n = self.inner.read(&mut self.buf).await?;
        if n == 0 {
            return Err(Error::Disconnected);
        }
        let read = String::from_utf8_lossy(&self.buf[..n]);
        if let &std::borrow::Cow::Owned(_) = &read {
            warn!("Non-UTF8 data read from controller: {:?}", read);
        }
        debug!("<<< {}", read);
        self.queue.push_str(&read);
        Ok(())
    }

    /// Waits until the specified response type is found in the stream.
    pub async fn pick<F, R>(&mut self, parse_fn: F) -> Result<R>
    where
        for<'a> F: Fn(&'a str) -> PResult<'a, R>,
    {
        loop {
            // Search for a match on each line beginning.
            // We cannot just iterate over splitlines since a pattern may span multiple lines.
            let mut found = None; // Option<match, offset, remainder_len>
            for offset in
                std::iter::once(0).chain(self.queue.match_indices('\n').map(|(n, _m)| n + 1))
            {
                if offset < self.queue.len() {
                    if let Ok((rem, mtch)) = parse_fn(&self.queue[offset..]) {
                        found = Some((mtch, offset, rem.len()));
                        break;
                    }
                }
            }
            if let Some((mtch, offset, rem)) = found {
                // cut out match from queue buffer
                self.queue.replace_range(offset..self.queue.len() - rem, "");
                return Ok(mtch);
            } else {
                self.receive().await?;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser;

    use assert_matches::assert_matches;
    use std::io::Cursor;

    #[tokio::test]
    async fn wait_on_closed_reader_should_fail() {
        let mut c = Connection::from_stream(Cursor::new(""));
        assert_matches!(c.pick(parser::kal).await, Err(Error::Disconnected));
    }

    #[tokio::test]
    async fn wait_should_return_match() {
        let mut c = Connection::from_stream(Cursor::new("1_KAL|1\n"));
        assert!(c.pick(parser::kal).await.is_ok());
        assert_eq!(c.queue, "");
    }

    #[tokio::test]
    async fn wait_should_cut_out_match() {
        let mut c = Connection::from_stream(Cursor::new(
            "1_KAL|1\n\
             1_DATE|07.11.20\n\
             1_DATAPRINT|1\n",
        ));
        assert_eq!(c.pick(parser::date).await.unwrap(), "07.11.20");
        assert_eq!(
            c.queue,
            "1_KAL|1\n\
             1_DATAPRINT|1\n"
        );
    }
}
