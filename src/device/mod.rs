use crate::bus::Token;
use crate::{DeviceInfo, MqttMsg, Response, Status, TwoWay};

use enum_dispatch::enum_dispatch;
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Value out of range: {0:?}")]
    Value(String),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[enum_dispatch]
pub trait Device {
    fn info(&self) -> &DeviceInfo;

    fn info_mut(&mut self) -> &mut DeviceInfo;

    fn model(&self) -> &'static str;

    fn name(&self) -> &str {
        self.info().name.as_ref().unwrap_or(&self.info().busid)
    }

    fn configured(&self) -> bool {
        // overridden in [`Model::Unknown`]
        true
    }

    /// Returns list of 1-Wire busaddrs (e.g., OWD14_1) for which events should be routed to this
    /// component.
    fn register_1wire(&self) -> Vec<String>;

    /// Issue initialization commands sent to the device. Possible answers must be handled via
    /// [`handle_1wire`]. Additionally issue initial MQTT stuff.
    fn init(&self) -> TwoWay {
        TwoWay::mqtt(self.info().topic("status"), self.info().status)
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::OWDStatus(os) => self.handle_status(os.status),
            _ => TwoWay::default(),
        })
    }

    fn handle_status(&mut self, new: Status) -> TwoWay {
        self.info_mut().status = new;
        TwoWay::mqtt(self.info().topic("status"), new)
    }

    /// Returns a list of topics which should be handled by this device. Each topic is assiociated
    /// with an opaque token which helps during event processing to associate the message to the
    /// registered topic.
    fn register_mqtt(&self) -> Vec<(String, Token)> {
        Vec::new()
    }

    fn handle_mqtt(&self, _msg: MqttMsg, _token: Token) -> Result<TwoWay> {
        Ok(TwoWay::default())
    }
}

macro_rules! new {
    ($type:ty) => {
        pub fn new(info: DeviceInfo) -> Self {
            Self {
                info,
                ..Self::default()
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

fn digital_io(info: &'_ DeviceInfo, n: usize, inout: &'_ str, val: u32) -> TwoWay {
    let mut res = TwoWay::default();
    for bit in 0..n {
        res.push_mqtt(
            info,
            format_args!("/{}/ch{}", inout, bit + 1),
            bool2str(val & (1 << bit)),
        )
    }
    res
}

mod airquality;
mod controller2;
mod switch8;
mod temphum;

use airquality::AirQuality;
use controller2::Controller2;
use switch8::Switch8;
use temphum::TempHum;

#[enum_dispatch(Device)]
#[derive(Clone, Debug, PartialEq)]
pub enum Model {
    TempHum(TempHum),
    AirQuality(AirQuality),
    Switch8(Switch8),
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
            "[{}] {:-5} {:-13} ({}) S/N {} ",
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
}
