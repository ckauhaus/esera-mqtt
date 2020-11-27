use crate::{parser, DeviceInfo, MqttMsg, Response, TwoWay};

use enum_dispatch::enum_dispatch;
use std::fmt;

type Result<T, E = crate::bus::Error> = std::result::Result<T, E>;

#[enum_dispatch]
pub trait Device {
    fn info(&self) -> &DeviceInfo;

    fn name(&self) -> &'static str;

    fn configured(&self) -> bool {
        // overridden in [`Model::Unknown`]
        true
    }

    /// Issue initialization commands sent to the device. Possible answers must be handled via
    /// [`handle_1wire`].
    fn init(&self) -> Vec<String> {
        Vec::new()
    }

    fn handle_1wire(&mut self, _resp: Response) -> Result<TwoWay> {
        Ok(TwoWay::default())
    }

    fn handle_mqtt<S>(&self, _msg: MqttMsg) -> Result<Vec<String>> {
        Ok(Vec::default())
    }
}

#[enum_dispatch(Device)]
#[derive(Clone, Debug, PartialEq)]
pub enum Model {
    Controller2(Controller2),
    // HubIII(HubIII),
    // Dimmer1(Dimmer1),
    // Switch8_16A(Switch8_16A),
    Unknown(Unknown),
}

impl Model {
    pub fn select(info: DeviceInfo) -> Self {
        let a = info.artno.clone();
        match &*a {
            "11340" => Self::Controller2(Controller2::new(info)),
            //         "11221" => Box::new(Dimmer1::default()),
            //         "11228" => Box::new(Switch8_16A::default()),
            //         "11322" => Box::new(HubIII::default()),
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
            "[{}] {} {} ({}) S/N {} ",
            info.contno,
            info.busid,
            self.name(),
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

macro_rules! new {
    ($type:ty) => {
        fn new(info: DeviceInfo) -> Self {
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

        fn name(&self) -> &'static str {
            stringify!($type)
        }
    };
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Controller2 {
    info: DeviceInfo,
    inputs: u8,
    outputs: u8,
    ana: f32,
    dio: parser::DIOStatus,
}

impl Controller2 {
    new!(Controller2);
}

impl Device for Controller2 {
    std_methods!(Controller2);

    fn init(&self) -> Vec<String> {
        vec!["GET,SYS,DIO".into()]
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::DIO(dio) => {
                debug!("[{}] DIO status: {}", dio.contno, dio.status);
                self.dio = dio.status;
                TwoWay::mqtt(&self.info, "DIO", dio.status)
            }
            _ => {
                warn!(
                    "[{}] Controller2: don't know how to handle {:?}",
                    self.info.contno, resp
                );
                TwoWay::default()
            }
        })
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
}
