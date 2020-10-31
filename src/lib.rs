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

mod recv;
pub use recv::{Connection, Response};

#[derive(Debug)]
pub struct Device {
    serial: String,
    status: recv::Status,
    artno: String,
    name: String,
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
