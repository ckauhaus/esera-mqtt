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
    Device(#[from] crate::device::Error),
    #[error(transparent)]
    MQTT(#[from] rumqttc::ClientError),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Universe(Vec<Bus>);

impl Universe {
    // controller/device
    fn cd(&mut self, contno: u8, devno: u8) -> &mut Model {
        &mut self.0[contno as usize].devices[devno as usize]
    }

    // controller/busaddr
    fn ca(&mut self, contno: u8, addr: &str) -> Option<&mut Model> {
        match self.0[contno as usize].index(addr) {
            Some(i) => Some(&mut self.0[contno as usize].devices[i]),
            None => None,
        }
    }

    fn set_controller(&mut self, csi: CSI, conn_idx: usize) -> Result<TwoWay> {
        let c = csi.contno as usize;
        if self.0.len() <= c {
            self.0.resize_with(c + 1, Bus::default);
        }
        Ok(self.0[c].set_controller(csi, conn_idx))
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
        self.0[c].register_mqtt();
    }

    /// Main processing entry point for incoming 1-Wire events.
    pub fn handle_1wire(&mut self, resp: Response, conn_idx: usize) -> Result<TwoWay> {
        match resp {
            Response::CSI(csi) => return self.set_controller(csi, conn_idx),
            Response::List3(l) => {
                let c = l.contno as usize;
                self.populate(l);
                // XXX update status for all devices on bus
                return Ok(self.0[c].init());
            }
            Response::DIO(ref dio) => return Ok(self.cd(dio.contno, 0).handle_1wire(resp)?),
            Response::Devstatus(ref s) => {
                debug!("[{}] {:?}", s.contno, resp);
                if let Some(dev) = self.ca(s.contno, &s.addr) {
                    return Ok(dev.handle_1wire(resp)?);
                }
            }
            Response::OWDStatus(ref s) => {
                debug!("[{}] {:?}", s.contno, resp);
                return Ok(self.cd(s.contno, s.owd).handle_1wire(resp)?);
            }
            Response::Keepalive(_) => (),
            Response::Event(_) => (),
            Response::Info(_) => (),
            _ => warn!("Unknown controller event {:?}", resp),
        }
        Ok(TwoWay::default())
    }

    /// Returns controller index (as defined via handle_1wire/CSI) if the given message matches a
    /// registered MQTT topic
    fn by_topic(&self, msg: &MqttMsg) -> Option<&Bus> {
        self.0
            .iter()
            .filter(|b| b.configured())
            .find(|b| msg.matches(&b.topic_pattern))
    }

    /// Processes MQTT message, returns MQTT/1Wire results as well as the controller index to pass
    /// the latter to.
    pub fn handle_mqtt(&mut self, msg: MqttMsg) -> Result<(TwoWay, usize), crate::Error> {
        if let Some(bus) = self.by_topic(&msg) {
            let i = bus.connection_idx;
            if let Some((dev, tok)) = bus.route(&msg.topic()) {
                return dev.handle_mqtt(msg, tok).map(|res| Ok((res, i)))?;
            }
        }
        Err(crate::Error::NoHandler(msg))
    }
}

impl fmt::Display for Universe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        for bus in self.0.iter().filter(|b| b.configured()) {
            write!(f, "{}", bus)?;
        }
        Ok(())
    }
}

pub(crate) type DevIdx = usize;
pub(crate) type Token = isize;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bus {
    csi: CSI,
    connection_idx: usize,
    topic_pattern: String,
    devices: [Model; 31],
    busaddrs: HashMap<String, usize>, // indexes into `devices`
    topics: HashMap<String, (DevIdx, Token)>, // indexes into `devices`
}

impl Bus {
    fn init(&self) -> TwoWay {
        self.devices
            .iter()
            .filter(|m| m.configured())
            .map(|d| d.init())
            .collect()
    }

    fn configured(&self) -> bool {
        self.csi.contno != 0
    }

    fn register_1wire(&mut self) {
        for (i, dev) in self.devices.iter().enumerate() {
            self.busaddrs
                .extend(dev.register_1wire().into_iter().map(|a| (a, i)))
        }
        debug!("[{}] 1Wire Registry: {:?}", self.csi.contno, self.busaddrs);
    }

    fn register_mqtt(&mut self) {
        for (i, dev) in self.devices.iter().enumerate() {
            self.topics.extend(
                dev.register_mqtt()
                    .into_iter()
                    .map(|(top, tok)| (top, (i, tok))),
            )
        }
        debug!("[{}] MQTT Registry: {:?}", self.csi.contno, self.topics);
    }

    /// Find index of registered busaddr (if any)
    fn index(&self, busaddr: &str) -> Option<usize> {
        self.busaddrs.get(busaddr).copied()
    }

    /// Find index of registered MQTT topic (if any)
    fn route(&self, topic: &str) -> Option<(&Model, Token)> {
        match self.topics.get(topic) {
            Some(&(i, tok)) => Some((&self.devices[i], tok)),
            None => None,
        }
    }

    /// connection_idx: opaque identifier passed to the calling context when decoding incoming MQTT
    /// messages
    fn set_controller(&mut self, csi: CSI, connection_idx: usize) -> TwoWay {
        info!(
            "[{}] Controller {} S/N {} FW {}",
            csi.contno, csi.artno, csi.serno, csi.fw
        );
        // initialize bus entry so that we know this item is occupied
        self.connection_idx = connection_idx;
        self.csi = csi.clone();
        self.devices[0] = Model::select(DeviceInfo {
            contno: csi.contno,
            busid: "SYS".into(),
            serno: csi.serno,
            status: Status::Online,
            artno: csi.artno,
            name: None,
        });
        self.topic_pattern = format!("ESERA/{}/#", self.csi.contno);
        TwoWay::from_mqtt(MqttMsg::sub(&self.topic_pattern))
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
