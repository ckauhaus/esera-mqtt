mod bus;
mod controller;
mod device;
mod mqtt;
mod parser;

#[macro_use]
extern crate log;
use crossbeam::channel;
use std::fmt;
use std::iter;
use thiserror::Error;

pub use bus::Universe;
pub use controller::ControllerConnection;
pub use controller::Error as ControllerError;
pub use device::Device;
pub use mqtt::{MqttConnection, MqttMsg};
pub use parser::{Response, Status, CSI};

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    MQTT(#[from] mqtt::Error),
    #[error(transparent)]
    Controller(#[from] channel::SendError<String>),
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub fn bool2str<N: Into<u32>>(n: N) -> &'static str {
    match n.into() {
        0 => "0",
        _ => "1",
    }
}

pub fn str2bool(s: &str) -> bool {
    matches!(s, "0")
}

pub fn float2centi(f: f32) -> u32 {
    (f * 100.) as u32
}

pub fn centi2float(c: u32) -> f32 {
    (c as f32) / 100.
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceInfo {
    pub contno: u8,
    pub busid: String,
    pub serno: String,
    pub status: Status,
    pub artno: String,
    pub name: Option<String>,
}

impl DeviceInfo {
    /// Format MQTT message topic relating to this device
    pub fn fmt(&self, args: fmt::Arguments) -> String {
        format!(
            "ESERA/{}/{}/{}",
            self.contno,
            self.name.as_ref().unwrap_or(&self.busid),
            args
        )
    }

    /// Format MQTT message topic relating to this device
    pub fn topic<S: AsRef<str>>(&self, item: S) -> String {
        self.fmt(format_args!("{}", item.as_ref()))
    }

    /// Creates list of busaddrs from busid and list of subaddresses
    pub fn mkbusaddrs(&self, addrs: &[u8]) -> Vec<String> {
        addrs
            .iter()
            .map(|i| format!("{}_{}", self.busid, i))
            .collect()
    }
}

/// Result datatype which may contain both mqtt messages and controller commands.
pub struct TwoWay<'a> {
    pub mqtt: Vec<MqttMsg>,
    pub ow: Box<dyn Iterator<Item = String> + 'a>,
}

impl<'a> TwoWay<'a> {
    pub fn new<I: IntoIterator<Item = String> + 'a>(msgs: Vec<MqttMsg>, cmds: I) -> Self {
        Self {
            mqtt: msgs,
            ow: Box::new(cmds.into_iter()),
        }
    }

    pub fn from_1wire<I: IntoIterator<Item = String> + 'a>(cmds: I) -> Self {
        Self {
            mqtt: Vec::default(),
            ow: Box::new(cmds.into_iter()),
        }
    }

    pub fn from_mqtt(msg: MqttMsg) -> Self {
        Self {
            mqtt: vec![msg],
            ow: Box::new(iter::empty()),
        }
    }

    pub fn mqtt<T: Into<String>, S: ToString>(topic: T, payload: S) -> Self {
        Self::from_mqtt(MqttMsg::new(topic, payload))
    }

    pub fn push_mqtt<S: ToString>(
        &mut self,
        devinfo: &DeviceInfo,
        detail: fmt::Arguments,
        payload: S,
    ) {
        self.mqtt.push(MqttMsg::new(devinfo.fmt(detail), payload))
    }

    pub fn send(self, mqtt: &mut MqttConnection, ctrl: &channel::Sender<String>) -> Result<()> {
        for msg in self.mqtt {
            mqtt.send(msg)?;
        }
        for cmd in self.ow {
            ctrl.send(cmd)?;
        }
        Ok(())
    }
}

impl<'a> From<Vec<MqttMsg>> for TwoWay<'a> {
    fn from(msgs: Vec<MqttMsg>) -> Self {
        Self {
            mqtt: msgs,
            ow: Box::new(iter::empty()),
        }
    }
}

impl<'a> Default for TwoWay<'a> {
    fn default() -> Self {
        Self {
            mqtt: Vec::default(),
            ow: Box::new(iter::empty()),
        }
    }
}
