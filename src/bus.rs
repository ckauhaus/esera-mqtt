use crate::device::*;
use crate::{parser, Device, DeviceInfo, MqttMsg, Response, Status, TwoWay, CSI};

use std::collections::HashMap;
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Controller(#[from] crate::controller::Error),
    #[error(transparent)]
    MQTT(#[from] rumqttc::ClientError),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Universe(Vec<Bus>);

impl Universe {
    // controller/device
    fn cd(&mut self, contno: u8, devno: usize) -> &mut Model {
        &mut self.0[contno as usize].devices[devno]
    }

    // controller/busaddr
    fn ca(&mut self, contno: u8, addr: &str) -> Option<&mut Model> {
        match self.0[contno as usize].index(addr) {
            Some(i) => Some(&mut self.0[contno as usize].devices[i]),
            None => None,
        }
    }

    fn set_controller(&mut self, csi: CSI) -> Result<TwoWay> {
        let c = csi.contno as usize;
        if self.0.len() <= c {
            self.0.resize_with(c + 1, Bus::default);
        }
        Ok(self.0[c].set_controller(csi))
    }

    fn populate(&mut self, lst: parser::List3) {
        let c = lst.contno as usize;
        assert!(
            self.0.len() > c,
            "BUG: Trying to populate 1-Wire bus {} before setting controller info",
            c
        );
        debug!("[{}] Loading device list", c);
        for (i, dev) in lst.items.into_iter().enumerate().take(30) {
            // devices[0] is reserved for the controller
            let slot = &mut self.0[c].devices[i + 1];
            if slot.info().serno != dev.serno {
                *slot = Model::select(dev);
            }
        }
        info!("{}", self.0[c]);
        self.0[c].register_1wire();
    }

    pub fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        match resp {
            Response::CSI(csi) => return self.set_controller(csi),
            Response::List3(l) => {
                let c = l.contno as usize;
                self.populate(l);
                // XXX update status for all devices on bus
                return Ok(self.0[c].init());
            }
            Response::DIO(ref dio) => return self.cd(dio.contno, 0).handle_1wire(resp),
            Response::Devstatus(ref s) => {
                debug!("[{}] {:?}", s.contno, resp);
                if let Some(dev) = self.ca(s.contno, &s.addr) {
                    return dev.handle_1wire(resp);
                }
            }
            Response::Keepalive(_) => (),
            Response::Event(_) => (),
            Response::Info(_) => (),
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
    busaddrs: HashMap<String, usize>, // indexes into `devices`
    topic: String,
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

    fn register_1wire(&mut self) {
        for (i, dev) in self.devices.iter().enumerate() {
            self.busaddrs
                .extend(dev.register_1wire().into_iter().map(|a| (a, i)))
        }
        debug!("[{}] Registry: {:?}", self.csi.contno, self.busaddrs);
    }

    fn set_controller(&mut self, csi: CSI) -> TwoWay {
        info!(
            "[{}] Controller {} S/N {} FW {}",
            csi.contno, csi.artno, csi.serno, csi.fw
        );
        // initialize bus entry so that we know this item is occupied
        self.csi = csi.clone();
        self.devices[0] = Model::select(DeviceInfo {
            contno: csi.contno,
            busid: "SYS".into(),
            serno: csi.serno,
            status: Status::Online,
            artno: csi.artno,
            name: None,
        });
        self.topic = format!("ESERA/{}/#", self.csi.contno);
        TwoWay::from_mqtt(MqttMsg::sub(&self.topic))
    }

    /// Find index of registered busaddr (if any)
    fn index(&self, busaddr: &str) -> Option<usize> {
        self.busaddrs.get(busaddr).copied()
    }
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
