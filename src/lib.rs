mod bus;
pub mod controller;
mod device;
mod mqtt;
mod parser;

#[macro_use]
extern crate log;
use std::iter;

pub use bus::Universe;
pub use controller::ControllerConnection;
pub use device::Device;
pub use mqtt::{MqttConnection, MqttMsg};
pub use parser::{DeviceInfo, Response, Status, CSI};

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

    pub fn mqtt<A: AsRef<str>, S: Into<String>>(
        devinfo: &DeviceInfo,
        detail: A,
        payload: S,
    ) -> Self {
        Self {
            mqtt: vec![MqttMsg::new(devinfo.topic(detail), payload)],
            ow: Box::new(iter::empty()),
        }
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
