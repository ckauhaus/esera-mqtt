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
pub struct Universe {
    bus: Vec<Bus>,
    topics: HashMap<String, (u8, DevIdx, Token)>, // indexes into controller, device
}

impl Universe {
    // controller/device
    fn cd(&mut self, contno: u8, devno: u8) -> &mut Model {
        &mut self.bus[contno as usize].devices[devno as usize]
    }

    // controller/busaddr
    fn ca(&mut self, contno: u8, addr: &str) -> Option<&mut Model> {
        match self.bus[contno as usize].index(addr) {
            Some(i) => Some(&mut self.bus[contno as usize].devices[i]),
            None => None,
        }
    }

    fn set_controller(&mut self, csi: CSI, conn_idx: usize) -> Result<TwoWay> {
        let c = csi.contno as usize;
        if self.bus.len() <= c {
            self.bus.resize_with(c + 1, Bus::default);
        }
        self.bus[c].set_controller(csi, conn_idx)
    }

    fn populate(&mut self, lst: parser::List3) -> TwoWay {
        let mut res = TwoWay::default();
        let c = lst.contno as usize;
        assert!(
            self.bus.len() > c,
            "BUG: Trying to populate 1-Wire bus {} before setting controller info",
            c
        );
        debug!("[{}] Loading device list", c);
        for (i, dev) in lst.items.into_iter().enumerate().take(30) {
            // devices[0] is reserved for the controller
            let slot = &mut self.bus[c].devices[i + 1];
            let status = dev.status;
            if slot.info().serno != dev.serno {
                *slot = Model::select(dev);
            }
            res += TwoWay::mqtt(slot.set_status(status));
        }
        info!("{}", self.bus[c]);
        self.bus[c].register_1wire();
        for (topic, addr) in self.bus[c].register_mqtt() {
            res += TwoWay::from_mqtt(MqttMsg::sub(&topic));
            self.topics.insert(topic, addr);
        }
        debug!("[{}] MQTT Registry: {:?}", self.bus[c].contno, self.topics);
        res
    }

    /// Main processing entry point for incoming 1-Wire events.
    pub fn handle_1wire(&mut self, resp: Response, conn_idx: usize) -> Result<TwoWay> {
        match resp {
            Response::CSI(csi) => return self.set_controller(csi, conn_idx),
            Response::List3(l) => {
                let c = l.contno as usize;
                let res = self.populate(l);
                let init_cmds = self.bus[c].init();
                let announcements = self.bus[c].announce();
                return Ok(res + TwoWay::new(announcements, init_cmds));
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

    /// Find index of registered MQTT topic (if any)
    fn route(&self, topic: &str) -> Option<(&Bus, &Model, Token)> {
        match self.topics.get(topic) {
            Some(&(c, i, tok)) => {
                let bus = &self.bus[c as usize];
                Some((bus, &bus.devices[i], tok))
            }
            None => None,
        }
    }

    /// Processes MQTT message, returns MQTT/1Wire results as well as the controller index to pass
    /// the latter to.
    pub fn handle_mqtt(&mut self, msg: MqttMsg) -> Result<(TwoWay, usize), crate::Error> {
        if let Some((bus, dev, tok)) = self.route(&msg.topic()) {
            info!("MQTT event: {}", msg);
            let i = bus.connection_idx;
            return dev.handle_mqtt(msg, tok).map(|res| Ok((res, i)))?;
        }
        Err(crate::Error::NoHandler(msg))
    }
}

impl fmt::Display for Universe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        for bus in self.bus.iter().filter(|b| b.configured()) {
            write!(f, "{}", bus)?;
        }
        Ok(())
    }
}

pub(crate) type DevIdx = usize;
pub(crate) type Token = isize;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bus {
    contno: u8,
    connection_idx: usize,
    devices: [Model; 31],
    busaddrs: HashMap<String, usize>, // indexes into `devices`
}

impl Bus {
    fn configured(&self) -> bool {
        self.contno != 0
    }

    fn register_1wire(&mut self) {
        for (i, dev) in self.devices.iter().enumerate() {
            self.busaddrs
                .extend(dev.register_1wire().into_iter().map(|a| (a, i)))
        }
        debug!("[{}] 1Wire Registry: {:?}", self.contno, self.busaddrs);
    }

    fn register_mqtt(&self) -> impl Iterator<Item = (String, (u8, DevIdx, Token))> + '_ {
        let contno = self.contno;
        self.devices.iter().enumerate().flat_map(move |(i, dev)| {
            dev.register_mqtt()
                .into_iter()
                .map(move |(topic, tok)| (topic, (contno, i, tok)))
        })
    }

    /// Find index of registered busaddr (if any)
    fn index(&self, busaddr: &str) -> Option<usize> {
        self.busaddrs.get(busaddr).copied()
    }

    fn init(&mut self) -> Vec<String> {
        self.devices
            .iter_mut()
            .filter(|m| m.configured())
            .flat_map(|d| d.init())
            .collect()
    }

    fn announce(&self) -> Vec<MqttMsg> {
        self.devices
            .iter()
            .filter(|m| m.configured())
            .flat_map(|d| {
                let mut msgs = d.announce();
                msgs.extend(d.get_status());
                msgs
            })
            .collect()
    }

    /// connection_idx: opaque identifier passed to the calling context when decoding incoming MQTT
    /// messages
    fn set_controller(&mut self, csi: CSI, connection_idx: usize) -> Result<TwoWay> {
        info!(
            "[{}] Controller {} S/N {} FW {}",
            csi.contno, csi.artno, csi.serno, csi.fw
        );
        // initialize bus entry so that we know this item is occupied
        self.contno = csi.contno;
        self.connection_idx = connection_idx;
        let slot = &mut self.devices[0];
        *slot = Model::select(DeviceInfo {
            contno: csi.contno,
            busid: "SYS".into(),
            serno: csi.serno.clone(),
            status: Status::Online,
            artno: csi.artno.clone(),
            name: None,
        });
        Ok(slot.handle_1wire(Response::CSI(csi))?)
    }
}

impl fmt::Display for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        writeln!(f, "Controller {}:", self.contno)?;
        for dev in self.devices.iter().filter(|m| m.configured()) {
            writeln!(f, "{}", dev)?;
        }
        Ok(())
    }
}
