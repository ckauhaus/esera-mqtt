use crate::parser;
use crate::{pick, Controller, Device, Status};

use rumqttc::{AsyncClient, QoS};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("While scanning 1-Wire bus: {0}")]
    Connection(#[from] crate::connection::Error),
    #[error("Don't understand bus id {0}")]
    Busid(String),
    #[error("MQTT error")]
    MQTT(#[from] rumqttc::ClientError),
    #[error("While initializing device {0} ({1}): {2}")]
    Initialize(String, String, #[source] crate::device::Error),
}

type Result<T, E = Error> = std::result::Result<T, E>;

fn busid2n(busid: &str) -> Result<usize> {
    busid
        .strip_prefix("OWD")
        .unwrap_or(busid)
        .parse()
        .map_err(|_| Error::Busid(busid.into()))
}

const BUSMAX: usize = 31;
type Devices = [Device; BUSMAX];

#[derive(Debug, Default)]
pub struct Bus {
    pub contno: u8,
    devices: Devices,
    evt_handlers: HashMap<String, usize>,
}

impl Bus {
    pub fn new(contno: u8, controller: Device) -> Self {
        let mut evt_handlers = HashMap::new();
        for addr in controller.register() {
            evt_handlers.insert(addr, 0);
        }
        let mut devices = Devices::default();
        devices[0] = controller;
        Self {
            contno,
            devices,
            evt_handlers,
        }
    }

    pub async fn init<C: Controller + Send>(ctrl: &mut C) -> Result<Self> {
        ctrl.send_line("SET,SYS,DATAPRINT,1").await?;
        pick(ctrl, parser::dataprint).await?;
        ctrl.send_line("GET,SYS,INFO").await?;
        let csi = pick(ctrl, parser::csi).await?;
        let ctrl_dev = Device::new("SYS".into(), csi.serno, Status::Online, csi.artno, None);
        Ok(Self::new(csi.contno, ctrl_dev))
    }

    pub async fn scan<C: Controller + Send>(
        &mut self,
        ctrl: &mut C,
    ) -> Result<Vec<(String, String)>> {
        info!("Building device list");
        // always re-initialize the controller
        let mut msgs = self.devices[0]
            .init(ctrl)
            .await
            .map_err(|e| Error::Initialize("SYS".into(), self.devices[0].serno.clone(), e))?;
        ctrl.send_line("GET,OWB,LISTALL1").await?;
        for e in pick(ctrl, parser::lst3).await? {
            let index = busid2n(&e.busid)?;
            let slot = &mut self.devices[index];
            if slot.serno != e.serno {
                *slot = Device::new(
                    e.busid,
                    e.serno,
                    e.status,
                    e.artno,
                    e.name.map(|n| n.trim().into()),
                );
                for busaddr in slot.register() {
                    self.evt_handlers.insert(busaddr, index);
                }
                msgs.extend(
                    slot.init(ctrl).await.map_err(|e| {
                        Error::Initialize(slot.busid.clone(), slot.serno.clone(), e)
                    })?,
                );
            }
        }
        Ok(msgs)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Device> {
        self.devices.iter()
    }

    pub fn handlers<'a>(&'a self) -> impl Iterator<Item = &'a str> + 'a {
        self.evt_handlers.keys().map(|k| k.as_str())
    }

    pub fn fmt_topic<S: AsRef<str>>(&self, tail: S) -> String {
        format!("ESERA/{}/{}", self.contno, tail.as_ref())
    }

    pub async fn handle_devstatus(&self, addr: &str, data: u32, mqtt: &AsyncClient) -> Result<()> {
        if let Some(i) = self.evt_handlers.get(addr) {
            let dev = &self.devices[*i];
            debug!("{}: handler({:?}, {})", addr, dev, data);
            self.publish(dev.status_update(addr, data), mqtt).await?;
        }
        Ok(())
    }

    pub async fn publish(&self, msgs: Vec<(String, String)>, mqtt: &AsyncClient) -> Result<()> {
        // XXX FuturesUnordered
        for (topic, value) in msgs {
            mqtt.publish(
                self.fmt_topic(topic),
                QoS::AtLeastOnce,
                false,
                value.into_bytes(),
            )
            .await?
        }
        Ok(())
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

#[cfg(test)]
mod test {
    use super::*;
    use assert_matches::assert_matches;
    use async_channel::bounded;
    use rumqttc::{Publish, Request};

    #[tokio::test]
    async fn devstatus_update() {
        let bus = Bus::new(3, Device::with_model("SYS", "11340"));
        let (tx, rx) = bounded(4);
        let (cancel_tx, _) = bounded(4);
        let mqtt = AsyncClient::from_senders(tx, cancel_tx);
        bus.handle_devstatus("SYS1_1", 3, &mqtt).await.unwrap();
        assert_matches!(rx.recv().await.unwrap(),
                Request::Publish(Publish { topic, payload, ..}) => {
                assert_eq!(topic, "ESERA/3/SYS/in/1");
                assert_eq!(payload, &b"1"[..])
        });
    }
}
