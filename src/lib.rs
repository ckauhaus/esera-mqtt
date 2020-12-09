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
    #[error(transparent)]
    Bus(#[from] bus::Error),
    #[error("No handler found for MQTT message {0:?}")]
    NoHandler(MqttMsg),
    #[error(transparent)]
    Device(#[from] device::Error),
}

type Result<T, E = Error> = std::result::Result<T, E>;

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
            "ESERA/{}/{}{}",
            self.contno,
            self.name.as_ref().unwrap_or(&self.busid),
            args
        )
    }

    /// Format MQTT message topic relating to this device
    pub fn topic<S: AsRef<str>>(&self, item: S) -> String {
        self.fmt(format_args!("/{}", item.as_ref()))
    }

    /// Creates list of busaddrs from busid and list of subaddresses
    pub fn mkbusaddrs(&self, addrs: &[u8]) -> Vec<String> {
        addrs
            .iter()
            .map(|i| format!("{}_{}", self.busid, i))
            .collect()
    }

    /// Returns bare device number as &str (e.g., "3" for "OWD3"). Non-OWD addrs will be returned
    /// unmodified (e.g., "SYS").
    pub fn devno(&self) -> &str {
        self.busid.strip_prefix("OWD").unwrap_or(&self.busid)
    }
}

/// Result datatype which may contain both mqtt messages and controller commands.
pub struct TwoWay {
    pub mqtt: Vec<MqttMsg>,
    pub ow: Vec<String>,
}

impl TwoWay {
    pub fn new(msgs: Vec<MqttMsg>, cmds: Vec<String>) -> Self {
        Self {
            mqtt: msgs,
            ow: cmds,
        }
    }

    pub fn from_1wire<S: Into<String>>(cmd: S) -> Self {
        Self {
            mqtt: Vec::default(),
            ow: vec![cmd.into()],
        }
    }

    pub fn from_mqtt(msg: MqttMsg) -> Self {
        Self {
            mqtt: vec![msg],
            ow: Vec::default(),
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

impl iter::FromIterator<TwoWay> for TwoWay {
    fn from_iter<I: IntoIterator<Item = TwoWay>>(iter: I) -> Self {
        let mut res = Self::default();
        for elem in iter {
            res.mqtt.extend(elem.mqtt);
            res.ow.extend(elem.ow)
        }
        res
    }
}

impl From<Vec<MqttMsg>> for TwoWay {
    fn from(msgs: Vec<MqttMsg>) -> Self {
        Self {
            mqtt: msgs,
            ow: Vec::default(),
        }
    }
}

impl Default for TwoWay {
    fn default() -> Self {
        Self {
            mqtt: Vec::default(),
            ow: Vec::default(),
        }
    }
}
