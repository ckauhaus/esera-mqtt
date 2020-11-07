#![allow(unused)]

#[macro_use]
extern crate log;
use chrono::Local;
use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use tokio::prelude::*;

mod connection;
mod parser;

pub use connection::Connection;
pub use parser::{Response, Status};

#[derive(Debug)]
pub struct Device {
    pub serial: String,
    pub status: Status,
    pub artno: String,
    pub name: String,
    model: Box<dyn Model>,
}

impl Default for Device {
    fn default() -> Self {
        Self {
            serial: String::default(),
            status: Status::Unconfigured,
            artno: String::default(),
            name: String::default(),
            model: Box::new(Unconfigured {}),
        }
    }
}

#[derive(Debug, Default)]
pub struct Bus {
    devices: [Device; 31],
}

impl Bus {
    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &'a Device> {
        self.devices.iter()
    }
}

impl<'a> IntoIterator for &'a Bus {
    type Item = &'a Device;
    type IntoIter = <&'a [Device; 31] as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.devices.iter()
    }
}

#[derive(Debug, Default)]
pub struct DeviceTree(HashMap<u8, Bus>);

impl DeviceTree {
    fn new() -> Self {
        Self(HashMap::new())
    }
}

impl Deref for DeviceTree {
    type Target = HashMap<u8, Bus>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DeviceTree {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub trait Model: fmt::Debug {}

#[derive(Debug, Default, Clone)]
pub struct Controller2 {}

impl Model for Controller2 {}

#[derive(Debug, Default, Clone)]
pub struct Unconfigured {}

impl Model for Unconfigured {}

pub async fn init_controller<C: AsyncRead + AsyncWrite + Unpin + fmt::Debug>(
    conn: &mut Connection<C>,
) -> Result<DeviceTree, Box<dyn std::error::Error>> {
    conn.send_line("SET,SYS,DATAPRINT,1").await?;
    conn.wait(parser::dataprint).await?;
    let now = Local::now();
    conn.send_line(format!("SET,SYS,DATE,{}", now.format("%d.%m.%y")))
        .await?;
    conn.wait(parser::date).await?;
    conn.send_line(format!("SET,SYS,TIME,{}", now.format("%H:%M:%S")))
        .await?;
    conn.wait(parser::time).await?;
    // conn.send("GET,SYS,INFO").await?;
    // let bus = Bus::default();
    // let csi = conn.select(parser::csi).await?;
    // bus.set(0, Device::select(conn.first())?)
    // if let Response::ContNo(contno) = conn.pick("CONTNO").await? {
    //     devtree.insert(contno, bus);
    // } else {
    //     panic!("BUG: pick(CONTNO) did not return Response::ContNo");
    // }
    let mut devtree = DeviceTree::new();
    Ok(devtree)
}
