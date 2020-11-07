use crate::parser::{PResult, Response, ResponseKind};

use bytes::{Bytes, BytesMut};
use futures::{Future, FutureExt, SinkExt, Stream, StreamExt};
use std::fmt;
use std::net::ToSocketAddrs;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::task::{Context, Poll};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::prelude::*;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Transport(#[from] std::io::Error),
    #[error("Trying to send unterminated line")]
    Unterminated,
    #[error("While parsing controller response {1:?}: {0}")]
    Parse(#[source] crate::parser::Error, String),
    #[error("Controller connection closed while waiting for response")]
    Disconnected,
    #[error("Read non-UTF8 data from controller: {0:?}")]
    Utf8(Vec<u8>, #[source] std::str::Utf8Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct Connection<S: fmt::Debug> {
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

impl<C: fmt::Debug> Connection<C> {
    pub fn from_stream(stream: C) -> Self {
        Self {
            inner: stream,
            queue: String::with_capacity(4096),
        }
    }
}

impl<C: AsyncWrite + Unpin + fmt::Debug> Connection<C> {
    pub async fn send_line<S: AsRef<str>>(&mut self, line: S) -> Result<()> {
        let line = line.as_ref();
        debug!("<<< {}", line.trim_end());
        self.inner.write_all(line.as_bytes()).await?;
        if !line.ends_with("\n") {
            self.inner.write(b"\n").await?;
        }
        Ok(())
    }
}

fn try_parse<'a, R, F>(input: &'a str, parse_fn: &F) -> Option<(usize, R, usize)>
where
    R: 'static + fmt::Debug,
    F: Fn(&'a str) -> PResult<'a, R>,
{
    use nom::bytes::complete::take_while1;
    use nom::character::complete::newline;
    use nom::combinator::{map, opt};
    use nom::sequence::{terminated, tuple};
    tuple((
        opt(terminated(take_while1(|_| true), newline)), // pre
        parse_fn,                                        // mtch
    ))(input)
    .map(|(rem, (pre, mtch))| {
        (
            pre.map(|s| s.len() + 1).unwrap_or_default(),
            mtch,
            rem.len(),
        )
    })
    .map_err(|e| e.map(|e| eprintln!("{}", nom::error::convert_error(input, e))))
    .ok()
}

impl<C: AsyncRead + Unpin + fmt::Debug> Connection<C> {
    /// Waits until the specified response type is found in the stream.
    pub async fn wait<F, R>(&mut self, parse_fn: F) -> Result<R>
    where
        R: 'static + fmt::Debug,
        for<'a> F: Fn(&'a str) -> PResult<'a, R>,
    {
        let mut buf = Vec::with_capacity(1 << 16);
        loop {
            // memory layout while looking for TIME statement:
            // 1_KAL|1\n1_TIME|15:05:31\n1_EVT|15:05:32\n
            // `--pre--'`--mtch---------'`--remainder---'
            if let Some((pre, mtch, rem)) = try_parse(&self.queue, &parse_fn) {
                debug!("got: [{}]{:?}[{}]", pre, mtch, rem);
                let rem_cursor = self.queue.len() - rem;
                self.queue.replace_range(pre..rem_cursor, "");
                return Ok(mtch);
            }
            // fill queue from the stream
            buf.clear();
            if self.inner.read(&mut buf).await? == 0 {
                return Err(Error::Disconnected);
            }
            debug!(">>> {}", String::from_utf8_lossy(&buf).trim_end());
            self.queue
                .push_str(std::str::from_utf8(&buf).map_err(|e| Error::Utf8(buf.clone(), e))?);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser;

    use assert_matches::assert_matches;
    use std::io::Cursor;

    #[test]
    fn try_parse_pre_mtch_rem() {
        assert_eq!(
            try_parse("1_KAL|1\n1_DATE|07.11.20\n1_DATAPRINT|1\n", &parser::date).unwrap(),
            (8, "07.11.20".into(), 14)
        )
    }

    #[tokio::test]
    async fn wait_on_closed_reader_should_fail() {
        let mut c = Connection::from_stream(Cursor::new(""));
        assert_matches!(c.wait(parser::kal).await, Err(Error::Disconnected));
    }

    #[tokio::test]
    async fn wait_should_return_match() {
        let mut c = Connection::from_stream(Cursor::new("1_KAL|1\n"));
        assert!(c.wait(parser::kal).await.is_ok());
        assert_eq!(c.queue, "");
    }

    #[tokio::test]
    async fn wait_should_cut_out_match() {
        let mut c = Connection::from_stream(Cursor::new(
            "\
            1_KAL|1\n\
            1_DATE|07.11.20\n\
            1_DATAPRINT|1\n",
        ));
        assert_eq!(c.wait(parser::date).await.unwrap(), "07.11.20");
        assert_eq!(
            c.queue,
            "\
            1_KAL|1\n\
            1_DATAPRINT|1\n"
        );
    }
}
