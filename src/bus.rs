use crate::device::*;
use crate::{parser, DeviceInfo, MqttMsg, Response, Status};

use crossbeam::channel::Sender;
use enum_dispatch::enum_dispatch;
use std::fmt;
use std::io::prelude::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Controller(#[from] crate::controller::Error),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Universe(Vec<Bus>);

impl Universe {
    fn set_controller(&mut self, csi: parser::CSI) {
        info!("[{}] 1-Wire controller: {:?}", csi.contno, csi)
    }

    fn populate(&mut self, lst: parser::List3) {
        info!("[{}] 1-Wire bus: {:?}", lst.contno, lst.items)
    }

    pub fn handle_1wire(&mut self, resp: Response) -> Result<Vec<MqttMsg>> {
        match resp {
            Response::CSI(csi) => self.set_controller(csi),
            Response::List3(l) => self.populate(l),
            _ => warn!("Unknown controller event {:?}", resp),
        }
        Ok(Vec::new())
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bus {
    contno: u8,
    devices: [Model; 31],
}
