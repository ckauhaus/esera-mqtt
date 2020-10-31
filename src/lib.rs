#![allow(unused)]

#[macro_use]
extern crate log;
use anyhow::{anyhow, Result};
use futures::sink::SinkExt;
use futures::stream::{FusedStream, Stream, StreamExt};
use futures::task;
use std::fmt;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::task::Poll;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LinesCodec};

pub type Str = smol_str::SmolStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Online = 0,
    Err1,
    Err2,
    Err3,
    Offline = 5,
    Unconfigured = 10,
}

impl Default for Status {
    fn default() -> Self {
        Status::Unconfigured
    }
}

#[derive(Debug)]
pub struct Device {
    serial: Str,
    status: Status,
    artno: Str,
    name: Str,
    model: Box<dyn Model>,
}

impl Default for Device {
    fn default() -> Self {
        Self {
            model: Box::new(Unconfigured {}),
            ..Default::default()
        }
    }
}

pub struct Bus {
    contno: u16,
    devices: [Device; 31],
}

pub trait Model: fmt::Debug {}

#[derive(Debug, Default, Clone)]
pub struct Controller2 {}

impl Model for Controller2 {}

#[derive(Debug, Default, Clone)]
pub struct Unconfigured {}

impl Model for Unconfigured {}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Recv {
    contno: u16,
    keyword: Str,
    args: Vec<Str>,
}

fn tokenize(line: &str) -> Result<Recv> {
    let mut recv = Recv::default();
    let mut s_contno = line.splitn(2, '_');
    recv.contno = s_contno
        .next()
        .ok_or(anyhow!("No controller number found in: {}", line))?
        .parse()?;
    let mut s_args = s_contno
        .next()
        .ok_or(anyhow!("short line after contno: {}", line))?
        .split('|');
    recv.keyword = s_args
        .next()
        .ok_or(anyhow!("no keyword after contno: {}", line))?
        .into();
    recv.args = s_args.map(Str::from).collect();
    Ok(recv)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Response {
    Contno(u16),
    Lst3,
    Lst3Item {
        busid: Str,
        serial: Str,
        status: Status,
        artno: Str,
        name: Str,
    },
    Event {
        busid: Str,
        data: Str,
    },
    Dataprint {
        val: u16,
    },
    Unkown(Recv),
    Error(String),
    KAL,
}

fn parse(recv: Recv) -> Response {
    Response::Unkown(recv)
}

pub struct Connection {
    stream: Framed<TcpStream, LinesCodec>,
    terminated: bool,
}

impl Connection {
    pub async fn new<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        let addr: Vec<_> = addr.to_socket_addrs()?.collect();
        info!("Connecting to {:?}", addr);
        let s = TcpStream::connect(&*addr).await?;
        Ok(Self {
            stream: Framed::new(s, LinesCodec::new()),
            terminated: false,
        })
    }

    pub async fn read(&mut self) -> Result<Response> {
        Ok(Response::KAL)
    }

    pub async fn write(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Stream for Connection {
    type Item = Response;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if self.terminated {
            Poll::Ready(None)
        } else {
            match self.stream.poll_next_unpin(cx) {
                Poll::Ready(Some(ref line)) => Poll::Ready(Some(Response::KAL)),
                Poll::Ready(None) => {
                    self.terminated = true;
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

impl FusedStream for Connection {
    fn is_terminated(&self) -> bool {
        self.terminated
    }
}
