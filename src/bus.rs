use crate::device::*;
use crate::parser::Msg;
use crate::{parser, Device, DeviceInfo, MqttMsg, Routes, Status, TwoWay, CSI, OW};

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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bus {
    pub contno: u8,
    pub devices: [Model; 31],
    busaddrs: HashMap<String, usize>, // indexes into `devices`
}

impl Bus {
    /// Updates busaddr to device mapping.
    fn register_1wire(&mut self) {
        for (i, dev) in self.devices.iter().enumerate() {
            self.busaddrs
                .extend(dev.register_1wire().into_iter().map(|a| (a, i)))
        }
        debug!("[{}] 1-Wire Registry: {:?}", self.contno, self.busaddrs);
    }

    // XXX needs unit test (got several defects)
    // beware: slots may be unoccupied but devidx routing keys must be correct
    fn register_mqtt(&self, routes: &mut Routes<usize>) -> TwoWay {
        let mut res = TwoWay::default();
        routes.clear();
        for (i, dev) in self
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| d.configured())
        {
            dev.register_mqtt()
                .into_iter()
                .filter_map(|(topic, tok)| routes.register(topic, i, tok))
                .for_each(|msg| res += TwoWay::from_mqtt(msg));
        }
        debug!("MQTT registry: {:?}", routes);
        res
    }

    /// Performs device-specific initialization commands for all configured devices.
    fn init(&mut self) -> Vec<String> {
        self.devices
            .iter_mut()
            .filter(|m| m.configured())
            .flat_map(|d| d.init())
            .collect()
    }

    fn populate(&mut self, lst: parser::List3) {
        debug!("[{}] Loading device list", self.contno);
        for (i, dev) in lst.into_iter().enumerate().take(30) {
            // devices[0] is reserved for the controller
            let slot = &mut self.devices[i + 1];
            let status = dev.status;
            if slot.info().serno != dev.serno {
                *slot = Model::select(dev);
            }
            if slot.configured() {
                slot.info_mut().status = status;
            }
        }
        info!("{}", self);
        self.register_1wire();
    }

    pub fn set_controller(&mut self, contno: u8, csi: CSI) -> Result<TwoWay> {
        info!(
            "[{}] Controller {} S/N {} FW {}",
            contno, csi.artno, csi.serno, csi.fw
        );
        // initialize bus entry so that we know this item is occupied
        self.contno = contno;
        let slot = &mut self.devices[0];
        *slot = Model::select(DeviceInfo {
            contno,
            busid: "SYS".into(),
            serno: csi.serno.clone(),
            status: Status::Online,
            artno: csi.artno.clone(),
            name: None,
        });
        // push down to actual device handler
        // this allows for additional initialization actions there
        Ok(slot.handle_1wire(OW {
            contno,
            msg: Msg::CSI(csi),
        })?)
    }

    /// Collects device discovery messages from all devices.
    fn announce(&self) -> Vec<MqttMsg> {
        self.devices
            .iter()
            .filter(|m| m.configured())
            .flat_map(|d| d.announce())
            .collect()
    }

    /// Find index of registered busaddr (if any)
    fn index(&self, busaddr: &str) -> Option<usize> {
        self.busaddrs.get(busaddr).copied()
    }

    /// Main processing entry point for incoming 1-Wire events.
    pub fn handle_1wire(&mut self, resp: OW, routes: &mut Routes<usize>) -> Result<TwoWay> {
        let contno = resp.contno;
        match resp.msg {
            Msg::CSI(csi) => return self.set_controller(contno, csi),
            Msg::List3(l) => {
                self.populate(l);
                let res = self.register_mqtt(routes);
                let init_cmds = self.init();
                let discovery_ann = self.announce();
                return Ok(res + TwoWay::new(discovery_ann, init_cmds));
            }
            Msg::DIO(_) => return Ok(self.devices[0].handle_1wire(resp)?),
            Msg::Devstatus(ref s) => {
                debug!("[{}] {:?}", contno, resp.msg);
                if let Some(i) = self.index(&s.addr) {
                    return Ok(self.devices[i].handle_1wire(resp)?);
                }
            }
            Msg::OWDStatus(ref s) => {
                debug!("[{}] {:?}", contno, resp.msg);
                return Ok(self.devices[s.owd as usize].handle_1wire(resp)?);
            }
            Msg::Keepalive(_) => (),
            Msg::Evt(_) => (),
            Msg::Inf(_) => (),
            Msg::Err(e) => error!("Controller error {}", e),
            _ => warn!("Unknown controller event {:?}", resp),
        }
        Ok(TwoWay::default())
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
