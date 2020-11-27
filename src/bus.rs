use crate::device::*;
use crate::{parser, Device, DeviceInfo, Response, Status, TwoWay, CSI};

use std::fmt;
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
    fn cont(&mut self, contno: u8) -> &mut Bus {
        &mut self.0[contno as usize]
    }

    fn set_controller(&mut self, csi: parser::CSI) {
        let c = csi.contno as usize;
        if self.0.len() <= c {
            self.0.resize_with(c + 1, Bus::default);
        }
        info!(
            "[{}] Controller {} S/N {} FW {}",
            csi.contno, csi.artno, csi.serno, csi.fw
        );
        // initialize bus entry so that we know this item is occupied
        self.0[c].csi = csi.clone();
        self.0[c].devices[0] = Model::select(DeviceInfo {
            contno: csi.contno,
            busid: "SYS".into(),
            serno: csi.serno,
            status: Status::Online,
            artno: csi.artno,
            name: None,
        })
    }

    fn populate(&mut self, lst: parser::List3) {
        let c = lst.contno as usize;
        assert!(
            self.0.len() > c,
            "BUG: Trying to populate 1-Wire bus {} before setting controller info",
            c
        );
        info!("[{}] Loading device list", c);
        for (i, dev) in lst.items.into_iter().enumerate().take(30) {
            // devices[0] is reserved for the controller
            self.0[c].devices[i + 1] = Model::select(dev)
        }
        info!("{}", self.0[c]);
    }

    pub fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        match resp {
            Response::CSI(csi) => self.set_controller(csi),
            Response::List3(l) => {
                let c = l.contno as usize;
                self.populate(l);
                return Ok(self.0[c].init());
            }
            Response::Event(_) => (),
            Response::Info(_) => (),
            Response::DIO(ref dio) => return self.cont(dio.contno).devices[0].handle_1wire(resp),
            _ => warn!("Unknown controller event {:?}", resp),
        }
        Ok(TwoWay::default())
    }
}

impl fmt::Display for Universe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        for bus in self.0.iter().filter(|e| e.csi != CSI::default()) {
            write!(f, "{}", bus)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bus {
    csi: CSI,
    devices: [Model; 31],
}

impl fmt::Display for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        writeln!(f, "Controller {}:", self.csi.contno)?;
        for dev in self.devices.iter().filter(|m| m.configured()) {
            writeln!(f, "{}", dev)?;
        }
        Ok(())
    }
}

impl Bus {
    fn init(&self) -> TwoWay {
        TwoWay::from_1wire(
            self.devices
                .iter()
                .filter(|m| m.configured())
                .flat_map(|d| d.init()),
        )
    }
}
