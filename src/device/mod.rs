use crate::bus::Token;
use crate::{DeviceInfo, MqttMsg, Response, Status, TwoWay};

use enum_dispatch::enum_dispatch;
use serde::Serialize;
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Value out of range: {0:?}")]
    Value(String),
    #[error("Invalid bus id: {0}")]
    BusId(String, #[source] std::num::ParseIntError),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Default, Debug, Clone, PartialEq, Serialize)]
pub struct AnnounceDevice {
    pub identifiers: Vec<String>,
    pub model: String,
    pub name: String,
    pub manufacturer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sw_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via_device: Option<String>,
}

#[enum_dispatch]
pub trait Device {
    /// Generated via [`std_methods`].
    fn info(&self) -> &DeviceInfo;

    /// Generated via [`std_methods`].
    fn info_mut(&mut self) -> &mut DeviceInfo;

    /// Generic model type like "Controller2". Generated via [`std_methods`].
    fn model(&self) -> &'static str;

    /// Human readable name. Falls back to OWD id if none set.
    fn name(&self) -> &str {
        self.info().name.as_ref().unwrap_or(&self.info().busid)
    }

    /// Whether this device is internally configured or not. Should be generally true.
    fn configured(&self) -> bool {
        // overridden in [`Model::Unknown`]
        true
    }

    /// Initializes device. This involved setting custom struct fields or issueing commands to the
    /// 1-Wire device. 1-Wire responses to initialization commands must be processed via
    /// [`handle_1wire`].
    fn init(&mut self) -> Vec<String> {
        Vec::default()
    }

    /// Announces device discovery data via MQTT.
    fn announce(&self) -> Vec<MqttMsg> {
        vec![]
    }

    /// Helper to create (largely constant) device data in announcements. Override for controllers.
    fn announce_device(&self) -> AnnounceDevice {
        AnnounceDevice {
            identifiers: vec![self.info().serno.clone(), self.name().into()],
            model: format!("{} {}", self.model(), self.info().artno),
            name: format!("1-Wire bus {}/{}", self.info().contno, self.name()),
            manufacturer: "ESERA".into(),
            sw_version: None,
            // Assume that we have exclusively Controller2. Needs to be generalized in case
            // several controller types are in use.
            via_device: Some(format!("1-Wire {}/Controller2", self.info().contno)),
        }
    }

    /// Processes status updates from 1-Wire list results.
    fn set_status(&mut self, new: Status) -> Vec<MqttMsg> {
        self.info_mut().status = new;
        self.get_status()
    }

    /// Returns MQTT message announcing the device status
    fn get_status(&self) -> Vec<MqttMsg> {
        let info = self.info();
        vec![MqttMsg::retain(info.topic("status"), info.status)]
    }

    /// Returns list of 1-Wire busaddrs (e.g., OWD14_1) for which events should be routed to this
    /// component.
    fn register_1wire(&self) -> Vec<String>;

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay>;

    /// Returns a list of topics which should be handled by this device. Each topic is assiociated
    /// with an opaque token which helps during event processing to associate the message to the
    /// registered topic.
    fn register_mqtt(&self) -> Vec<(String, Token)> {
        Vec::default()
    }

    fn handle_mqtt(&self, _msg: MqttMsg, _token: Token) -> Result<TwoWay> {
        Ok(TwoWay::default())
    }
}

macro_rules! new {
    ($type:ty) => {
        pub fn new(info: DeviceInfo) -> Self {
            #[allow(clippy::needless_update)]
            Self {
                info,
                ..Default::default()
            }
        }
    };
}

macro_rules! std_methods {
    ($type:ty) => {
        fn info(&self) -> &DeviceInfo {
            &self.info
        }

        fn info_mut(&mut self) -> &mut DeviceInfo {
            &mut self.info
        }

        fn model(&self) -> &'static str {
            stringify!($type)
        }
    };
}

pub fn bool2str<N: Into<u32>>(n: N) -> &'static str {
    match n.into() {
        0 => "0",
        _ => "1",
    }
}

pub fn str2bool(s: &str) -> bool {
    match s {
        "1" => true,
        "on" => true,
        "true" => true,
        _ => false,
    }
}

fn float2centi(f: f32) -> u32 {
    (f * 100.) as u32
}

fn centi2float(c: u32) -> f32 {
    (c as f32) / 100.
}

fn disc_topic(typ: &str, info: &DeviceInfo, sub: fmt::Arguments) -> String {
    format!(
        "homeassistant/{}/{}/{}_{}/config",
        typ,
        info.contno,
        info.serno.replace(
            |c: char| { !c.is_ascii_alphanumeric() && c != '_' && c != '-' },
            ""
        ),
        sub
    )
}

fn digital_io(info: &'_ DeviceInfo, n: usize, inout: &'_ str, val: u32) -> TwoWay {
    let mut res = TwoWay::default();
    for bit in 0..n {
        res += TwoWay::from_mqtt(MqttMsg::new(
            info.fmt(format_args!("/{}/ch{}", inout, bit + 1)),
            bool2str(val & (1 << bit)),
        ))
    }
    res
}

mod airquality;
mod controller2;
mod switch8;

use airquality::{AirQuality, TempHum};
use controller2::Controller2;
use switch8::{Switch8, Switch8Out};

#[enum_dispatch(Device)]
#[derive(Clone, Debug, PartialEq)]
pub enum Model {
    TempHum(TempHum),
    AirQuality(AirQuality),
    Switch8(Switch8),
    Switch8Out(Switch8Out),
    Controller2(Controller2),
    Unknown(Unknown),
}

impl Model {
    pub fn select(info: DeviceInfo) -> Self {
        let a = info.artno.clone();
        match &*a {
            "11150" => Self::TempHum(TempHum::new(info)),
            "11151" => Self::AirQuality(AirQuality::new(info)),
            "11220" => Self::Switch8(Switch8::new(info)),
            "11228" => Self::Switch8(Switch8::new(info)),
            "11229" => Self::Switch8Out(Switch8Out::new(info)),
            "11340" => Self::Controller2(Controller2::new(info)),
            _ => Self::Unknown(Unknown::new(info)),
        }
    }
}

impl Default for Model {
    fn default() -> Self {
        Self::Unknown(Unknown::default())
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let info = self.info();
        write!(
            f,
            "[{}] {:-5} {:-13} ({}) S/N {:-17} ",
            info.contno,
            info.busid,
            self.model(),
            info.artno,
            info.serno
        )?;
        write!(
            f,
            "{}",
            match info.name {
                Some(ref n) => n,
                None => "-",
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Unknown {
    info: DeviceInfo,
}

impl Unknown {
    new!(Unknown);
}

impl Device for Unknown {
    std_methods!(Unknown);

    fn configured(&self) -> bool {
        false
    }

    fn register_1wire(&self) -> Vec<String> {
        Vec::new()
    }

    fn handle_1wire(&mut self, _resp: Response) -> Result<TwoWay> {
        Ok(TwoWay::default())
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        Vec::default()
    }

    fn handle_mqtt(&self, _msg: MqttMsg, _token: Token) -> Result<TwoWay> {
        Ok(TwoWay::default())
    }
}
