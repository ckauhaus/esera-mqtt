mod connection;
mod device;
mod parser;

#[macro_use]
extern crate log;
use chrono::Local;
use std::fmt;
use std::ops::{Deref, DerefMut};
use tokio::prelude::*;

pub use connection::Connection;
pub use device::Device;
pub use parser::{Response, Status};

type Devices = [Device; 31];

#[derive(Debug, Default)]
pub struct Bus {
    devices: Devices,
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

impl Deref for Bus {
    type Target = Devices;
    fn deref(&self) -> &Self::Target {
        &self.devices
    }
}

impl DerefMut for Bus {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.devices
    }
}

// XXX move to connection.rs
pub async fn init_controller<C: AsyncRead + AsyncWrite + Unpin + fmt::Debug>(
    conn: &mut Connection<C>,
) -> Result<(u8, Bus), Box<dyn std::error::Error>> {
    conn.send_line("SET,SYS,DATAPRINT,1").await?;
    conn.pick(parser::dataprint).await?;
    let now = Local::now();
    conn.send_line(format!("SET,SYS,DATE,{}", now.format("%d.%m.%y")))
        .await?;
    conn.pick(parser::date).await?;
    conn.send_line(format!("SET,SYS,TIME,{}", now.format("%H:%M:%S")))
        .await?;
    conn.pick(parser::time).await?;
    conn.send_line("GET,SYS,INFO").await?;
    let csi = conn.pick(parser::csi).await?;
    dbg!(&csi);
    let mut bus = Bus::default();
    bus[0] = Device::new(csi.serno, Status::Online, csi.artno, None);
    Ok((csi.contno, bus))
}
