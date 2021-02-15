use crate::parser::OW;
use crate::{DeviceInfo, MqttMsg, Token, TwoWay};

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
        self.info().name()
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
        let info = self.info();
        let mut identifiers = vec![info.serno.clone()];
        if let Some(name) = &info.name {
            identifiers.push(format!("{}/{}", info.contno, name))
        }
        AnnounceDevice {
            identifiers,
            model: format!("{} {}", self.model(), info.artno),
            name: format!("1-Wire bus {}/{}", info.contno, self.name()),
            manufacturer: "ESERA".into(),
            sw_version: None,
            via_device: if info.busid != "SYS" {
                Some(format!("{}/SYS", info.contno))
            } else {
                None
            },
        }
    }

    /// Returns list of 1-Wire busaddrs (e.g., OWD14_1) for which events should be routed to this
    /// component.
    fn register_1wire(&self) -> Vec<String>;

    fn handle_1wire(&mut self, resp: OW) -> Result<TwoWay>;

    /// Returns a list of topics which should be handled by this device. Each topic is assiociated
    /// with an opaque token which helps during event processing to associate the message to the
    /// registered topic.
    fn register_mqtt(&self) -> Vec<(String, Token)> {
        Vec::default()
    }

    fn handle_mqtt(&self, _msg: &MqttMsg, _token: Token) -> Result<TwoWay> {
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

macro_rules! ow_sensor_handlers {
    ( $( $n:expr => $topic:expr ),* ) => {
        fn register_1wire(&self) -> Vec<String> {
            let mut res = Vec::with_capacity(5);
            $( res.push(format!("{}_{}", self.info.busid, $n)); )*
            res
        }

        fn handle_1wire(&mut self, resp: OW) -> Result<TwoWay> {
            Ok(match resp.msg {
                Msg::Devstatus(s) => match
                    s.addr
                        .rsplit('_')
                        .nth(0)
                        .unwrap()
                        .parse()
                        .map_err(|e| super::Error::BusId(s.addr.to_owned(), e))? {
                    $( $n => TwoWay::from_mqtt(self.info.mqtt_msg($topic, centi2float(s.val))), )*
                    other => panic!("BUG: Unknown busaddr {}", other),
                },
                _ => {
                    warn!("[{}] {}: no handler for {:?}", self.info.contno, self.model(), resp);
                    TwoWay::default()
                }
            })
        }
    };
}

pub fn bool2str<N: Into<u32>>(n: N) -> &'static str {
    if n.into() == 0 {
        "0"
    } else {
        "1"
    }
}

pub fn str2bool(s: &str) -> bool {
    matches!(s, "1" | "on" | "true")
}

fn float2centi(f: f32) -> i32 {
    (f * 100.) as i32
}

fn centi2float(c: i32) -> f32 {
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

fn digital_io(
    info: &'_ DeviceInfo,
    n: usize,
    inout: &'_ str,
    val: i32,
    previous: Option<i32>,
) -> TwoWay {
    assert!(
        val >= 0,
        "DigitalIO value must be positive ({} has {})",
        info.busid,
        val
    );
    let mut res = TwoWay::default();
    for bit in 0..n {
        if let Some(old) = previous {
            if val & (1 << bit) != old & (1 << bit) {
                let new = bool2str(val as u32 & (1 << bit));
                info!(
                    "[{}] state change name={} busid={} channel={} new={}",
                    info.contno,
                    info.name(),
                    info.busid,
                    bit + 1,
                    new
                );
                res += TwoWay::from_mqtt(MqttMsg::new(
                    info.fmt(format_args!("{}/ch{}", inout, bit + 1)),
                    new,
                ))
            }
        } else {
            res += TwoWay::from_mqtt(MqttMsg::new(
                info.fmt(format_args!("{}/ch{}", inout, bit + 1)),
                bool2str(val as u32 & (1 << bit)),
            ))
        }
    }
    res
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn digio_mqtt() {
        assert_eq!(
            digital_io(
                &DeviceInfo::new(1, "SYS", "", "online", "", None).unwrap(),
                3,
                "in",
                0b101,
                None
            ),
            TwoWay::new(
                vec![
                    MqttMsg::new("ESERA/1/SYS/in/ch1", "1"),
                    MqttMsg::new("ESERA/1/SYS/in/ch2", "0"),
                    MqttMsg::new("ESERA/1/SYS/in/ch3", "1"),
                ],
                vec![]
            )
        )
    }

    #[test]
    fn digio_diff_against_old_state() {
        assert_eq!(
            digital_io(
                &DeviceInfo::new(1, "SYS", "", "online", "", None).unwrap(),
                3,
                "but",
                0b010,
                Some(0b100)
            ),
            TwoWay::new(
                vec![
                    MqttMsg::new("ESERA/1/SYS/but/ch2", "1"),
                    MqttMsg::new("ESERA/1/SYS/but/ch3", "0"),
                ],
                vec![]
            )
        )
    }
}

mod airquality;
mod binary_sensor;
mod controller2;
mod hub;
mod switch8;

use airquality::{AirQuality, TempHum};
use binary_sensor::BinarySensor;
use controller2::Controller2;
use hub::Hub;
use switch8::Switch8;

#[enum_dispatch(Device)]
#[derive(Clone, Debug, PartialEq)]
pub enum Model {
    AirQuality(AirQuality),
    BinarySensor(BinarySensor),
    Controller2(Controller2),
    Hub(Hub),
    Switch8(Switch8),
    TempHum(TempHum),
    Unknown(Unknown),
}

impl Model {
    pub fn select(info: DeviceInfo) -> Self {
        let a = info.artno.clone();
        match &*a {
            "11150" => Self::TempHum(TempHum::new(info)),
            "11151" => Self::AirQuality(AirQuality::new(info)),
            "11216" => Self::BinarySensor(BinarySensor::new(info)),
            "11220" | "11228" | "11229" => Self::Switch8(Switch8::new(info)),
            "11322" => Self::Hub(Hub::new(info)),
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
        write!(f, "{}", info.name.as_deref().unwrap_or("-"))
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

    fn handle_1wire(&mut self, _resp: OW) -> Result<TwoWay> {
        Ok(TwoWay::default())
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        Vec::default()
    }

    fn handle_mqtt(&self, _msg: &MqttMsg, _token: Token) -> Result<TwoWay> {
        Ok(TwoWay::default())
    }
}
