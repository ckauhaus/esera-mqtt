use crate::parser::{Parser, Response, ResponseKind};

use futures::stream::FusedStream;
use futures::{Future, SinkExt, Stream, StreamExt};
use std::net::ToSocketAddrs;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::task::{Context, Poll};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LinesCodec, LinesCodecError};

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Transport(#[from] LinesCodecError),
    #[error("While parsing controller response {1:?}: {0}")]
    Parse(#[source] crate::parser::Error, String),
    #[error("Controller connection closed while waiting for response")]
    Disconnected,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

type ConnectionInner = Framed<TcpStream, LinesCodec>;

pub struct Connection {
    inner: ConnectionInner,
    terminated: bool,
    queue: std::collections::VecDeque<String>,
    parser: Parser,
}

impl Connection {
    pub async fn new<A: ToSocketAddrs>(addr: A) -> Result<Self, std::io::Error> {
        let addr: Vec<_> = addr.to_socket_addrs()?.collect();
        info!("Connecting to {:?}", addr);
        let s = TcpStream::connect(&*addr).await?;
        Ok(Self {
            inner: Framed::new(s, LinesCodec::new()),
            terminated: false,
            queue: Default::default(),
            parser: Parser::new(),
        })
    }

    pub fn send<'a, S: AsRef<str> + 'a>(
        &'a mut self,
        line: S,
    ) -> impl Future<Output = Result<(), LinesCodecError>> + 'a {
        debug!("<<< {}", line.as_ref());
        self.inner.send(line)
    }

    fn parse(&mut self, line: &str) -> Result<Response> {
        self.parser
            .parse(line)
            .map_err(|e| Error::Parse(e, line.to_owned()))
    }

    /// Waits until the specified response type is found in the stream.
    pub async fn wait(&mut self, kind: ResponseKind) -> Result<Response> {
        // XXX handle queue
        while let Some(resp) = self.next().await {
            let resp = resp?;
            if ResponseKind::from(&resp) == kind {
                return Ok(resp);
            }
        }
        Err(Error::Disconnected)
    }

    /// Returns the first response which matches the filter criteria.
    pub async fn select<F, R>(&mut self, filter: F) -> Result<R>
    where
        F: Fn(&Response) -> Option<R>,
    {
        // XXX handle queue
        while let Some(resp) = self.next().await {
            let resp = resp?;
            if let Some(r) = filter(&resp) {
                return Ok(r);
            }
        }
        Err(Error::Disconnected)
    }
}

impl Stream for Connection {
    type Item = Result<Response>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(line) = self.queue.pop_front() {
            return Poll::Ready(Some(self.parse(&line)));
        }
        if self.terminated {
            return Poll::Ready(None);
        }
        match self.inner.poll_next_unpin(cx) {
            Poll::Ready(Some(res)) => Poll::Ready(Some(
                res.map_err(|e| Error::Transport(e)).and_then(|line| {
                    debug!(">>> {}", line);
                    self.parse(&line)
                }),
            )),
            Poll::Ready(None) => {
                self.terminated = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl FusedStream for Connection {
    fn is_terminated(&self) -> bool {
        self.terminated
    }
}

impl Deref for Connection {
    type Target = ConnectionInner;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Connection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
