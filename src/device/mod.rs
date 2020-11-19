mod controller2;
use controller2::Controller2;

use crate::bus::MqttMsgs;
use crate::{Controller, Response};

use bitflags::bitflags;
use futures::future::{self, BoxFuture};
use std::fmt;
use std::sync::Mutex;
use strum_macros::{AsRefStr, Display, EnumString, IntoStaticStr};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Controller connection: {0}")]
    Connection(#[from] crate::connection::Error),
    #[error("While setting controller: {0}")]
    Controller2(#[from] controller2::Error),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, AsRefStr)]
pub enum Status {
    #[strum(serialize = "S_0")]
    Online,
    #[strum(serialize = "S_1")]
    Err1,
    #[strum(serialize = "S_2")]
    Err2,
    #[strum(serialize = "S_3")]
    Err3,
    #[strum(serialize = "S_5")]
    Offline,
    #[strum(serialize = "S_10")]
    Unconfigured,
}

#[derive(Debug)]
pub struct Device {
    pub busid: String,
    pub serno: String,
    pub status: Status,
    pub artno: String,
    pub name: Option<String>,
    model: Mutex<Box<dyn Model + Send>>,
}

impl Default for Device {
    fn default() -> Self {
        Self {
            busid: String::default(),
            serno: String::default(),
            status: Status::Unconfigured,
            artno: String::default(),
            name: None,
            model: Mutex::new(Box::new(Unknown)),
        }
    }
}

fn select_model(artno: &str, serno: &str) -> Box<dyn Model + Send> {
    match artno {
        "11221" => Box::new(Dimmer1::default()),
        "11228" => Box::new(Switch8_16A::default()),
        "11322" => Box::new(HubIII::default()),
        "11340" => Box::new(Controller2::default()),
        _ => {
            if artno != "none" {
                warn!("Unknown model: {} ({})", artno, serno);
            }
            Box::new(Unknown)
        }
    }
}

impl Device {
    pub fn new(
        busid: String,
        serno: String,
        status: Status,
        artno: String,
        name: Option<String>,
    ) -> Self {
        let model = Mutex::new(select_model(&artno, &serno));
        Self {
            busid,
            serno,
            status,
            artno,
            name,
            model,
        }
    }

    #[cfg(test)]
    pub fn with_model(busid: &str, artno: &str) -> Self {
        let model = Mutex::new(select_model(artno, "FFFFFFFFFFFFFFFF"));
        Self {
            busid: String::from(busid),
            serno: String::default(),
            status: Status::Unconfigured,
            artno: artno.into(),
            name: None,
            model,
        }
    }

    pub fn model_name(&self) -> &'static str {
        self.model.lock().unwrap().name()
    }

    pub async fn init(&mut self, ctrl: &mut (dyn Controller + Send)) -> Result<()> {
        self.model.lock().unwrap().init(ctrl).await
    }

    pub fn register_addrs(&self) -> Vec<String> {
        self.model.lock().unwrap().register_addrs(&self)
    }

    /// Returns list of MQTT topics which should be processed by this device (relative to root
    /// topic, e.g. ESERA/1)
    pub fn register_topics(&self) -> Vec<String> {
        self.model.lock().unwrap().register_topics(&self)
    }

    /// Process generic 1-Wire response
    pub fn generic_update(&self, evt: Response) -> Result<MqttMsgs> {
        self.model.lock().unwrap().generic_update(evt)
    }

    /// Process 1-Wire bus event
    pub fn status_update(&self, addr: &str, data: u32) -> MqttMsgs {
        self.model.lock().unwrap().status_update(addr, data)
    }

    /// Process MQTT message
    pub async fn process(
        &self,
        topic: &str,
        data: &[u8],
        ctrl: &mut (dyn Controller + Send),
    ) -> Result<()> {
        let mut m = self.model.lock().unwrap();
        for line in m.process_msg(topic, data)? {
            ctrl.send_line(&line).await?;
        }
        Ok(())
    }
}

trait Model: fmt::Debug {
    fn name(&self) -> &'static str {
        let n = std::any::type_name::<Self>();
        if let Some(pos) = n.rfind(':') {
            &n[pos + 1..]
        } else {
            n
        }
    }

    fn register_addrs(&self, _dev: &Device) -> Vec<String> {
        Vec::default()
    }

    fn register_topics(&self, _dev: &Device) -> Vec<String> {
        Vec::default()
    }

    fn init<'a>(&'a mut self, _ctrl: &'a mut (dyn Controller + Send)) -> BoxFuture<'a, Result<()>> {
        Box::pin(future::ready(Ok(())))
    }

    fn generic_update(&mut self, _resp: Response) -> Result<MqttMsgs> {
        Ok(MqttMsgs::default())
    }

    // XXX fold into generic_update?
    fn status_update(&mut self, _addr: &str, _data: u32) -> MqttMsgs {
        MqttMsgs::default()
    }

    fn process_msg(&mut self, _topic: &str, _payload: &[u8]) -> Result<Vec<String>> {
        Ok(Vec::default())
    }
}

fn boolstr<N: Into<u32>>(n: N) -> &'static str {
    match n.into() {
        0 => "0",
        _ => "1",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, AsRefStr, IntoStaticStr)]
pub enum Dio {
    #[strum(serialize = "0", to_string = "INDEPENDENT+LEVEL")]
    IndependentLevel,
    #[strum(serialize = "1", to_string = "INDEPENDENT+EDGE")]
    IndependentEdge,
    #[strum(serialize = "2", to_string = "LINKED+LEVEL")]
    LinkedLevel,
    #[strum(serialize = "3", to_string = "LINKED+EDGE")]
    LinkedEdge,
}

impl Default for Dio {
    fn default() -> Self {
        Dio::IndependentLevel
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct HubIII {
    voltage12: f32,
    current12: f32,
    voltage5: f32,
    current5: f32,
}

impl Model for HubIII {
    fn register_addrs(&self, d: &Device) -> Vec<String> {
        (1..=4).map(|i| format!("{}_{}", d.busid, i)).collect()
    }
}

bitflags! {
    #[derive(Default)]
    struct SwitchFlags: u8 {
        const CH1 = 1<<0;
        const CH2 = 1<<1;
        const CH3 = 1<<2;
        const CH4 = 1<<3;
        const CH5 = 1<<4;
        const CH6 = 1<<5;
        const CH7 = 1<<6;
        const CH8 = 1<<7;
    }

}

#[derive(Debug, Default, Clone, PartialEq)]
struct Switch8_16A {
    inputs: SwitchFlags,
    outputs: SwitchFlags,
}

impl Model for Switch8_16A {
    fn register_addrs(&self, d: &Device) -> Vec<String> {
        vec![format!("{}_1", d.busid), format!("{}_3", d.busid)]
    }
}

bitflags! {
    #[derive(Default)]
    struct DimmerFlags: u8 {
        const EXT_PB1 = 1<<0;
        const EXT_PB2 = 1<<1;
        const MODULE_PB1 = 1<<2;
        const MODULE_PB2 = 1<<3;
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Dimmer1 {
    inputs: DimmerFlags,
    ch1: u8,
    ch2: u8,
}

impl Model for Dimmer1 {
    fn register_addrs(&self, d: &Device) -> Vec<String> {
        vec![
            format!("{}_1", d.busid), // inputs
            format!("{}_3", d.busid), // dimmer channel 1
            format!("{}_4", d.busid), // dimmer channel 2
        ]
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Unknown;

impl Model for Unknown {
    fn register_addrs(&self, _dev: &Device) -> Vec<String> {
        vec![]
    }
}
