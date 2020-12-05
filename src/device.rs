use crate::{bool2str, centi2float, parser, DeviceInfo, MqttMsg, Response, TwoWay};

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

    /// Returns list of 1-Wire busaddrs (e.g., OWD14_1) for which events should be routed to this
    /// component.
    fn register_1wire(&self) -> Vec<String>;

    /// Issue initialization commands sent to the device. Possible answers must be handled via
    /// [`handle_1wire`].
    fn init(&self) -> Vec<String> {
        Vec::new()
    }

    fn handle_1wire(&mut self, _resp: Response) -> Result<TwoWay> {
        Ok(TwoWay::default())
    }

    /// Returns a list of topics which should be handled by this device
    fn register_mqtt(&self) -> Vec<String> {
        Vec::new()
    }

    fn handle_mqtt<S>(&self, _msg: MqttMsg) -> Result<Vec<String>> {
        Ok(Vec::default())
    }
}

#[enum_dispatch(Device)]
#[derive(Clone, Debug, PartialEq)]
pub enum Model {
    TempHum(TempHum),
    AirQualityPro(AirQualityPro),
    Switch8(Switch8),
    Controller2(Controller2),
    Unknown(Unknown),
}

impl Model {
    pub fn select(info: DeviceInfo) -> Self {
        let a = info.artno.clone();
        match &*a {
            "11150" => Self::TempHum(TempHum::new(info)),
            "11151" => Self::AirQualityPro(AirQualityPro::new(info)),
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
pub struct TempHum {
    info: DeviceInfo,
}

impl TempHum {
    new!(TempHum);
}

impl Device for TempHum {
    std_methods!(TempHum);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 3, 4])
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::Devstatus(s) => match s.addr.rsplit('_').nth(0).unwrap() {
                "1" => TwoWay::mqtt(self.info.topic("temp"), centi2float(s.val)),
                "3" => TwoWay::mqtt(self.info.topic("hum"), centi2float(s.val)),
                "4" => TwoWay::mqtt(self.info.topic("dew"), centi2float(s.val)),
                other => panic!("BUG: Unknown busaddr {}", other),
            },
            _ => {
                warn!("[{}] TempHum: no handler for {:?}", self.info.contno, resp);
                TwoWay::default()
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AirQualityPro {
    info: DeviceInfo,
}

impl AirQualityPro {
    new!(AirQualityPro);
}

impl Device for AirQualityPro {
    std_methods!(AirQualityPro);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 3, 4, 5])
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::Devstatus(s) => match s.addr.rsplit('_').nth(0).unwrap() {
                "1" => TwoWay::mqtt(self.info.topic("temp"), centi2float(s.val)),
                "3" => TwoWay::mqtt(self.info.topic("hum"), centi2float(s.val)),
                "4" => TwoWay::mqtt(self.info.topic("dew"), centi2float(s.val)),
                "5" => TwoWay::mqtt(self.info.topic("co2"), centi2float(s.val)),
                other => panic!("BUG: Unknown busaddr {}", other),
            },
            _ => {
                warn!(
                    "[{}] AirQualityPro: no handler for {:?}",
                    self.info.contno, resp
                );
                TwoWay::default()
            }
        })
    }
}

fn digital_io<'a>(info: &'_ DeviceInfo, n: usize, inout: &'_ str, val: u32) -> TwoWay<'a> {
    let mut res = TwoWay::default();
    for bit in 0..n {
        res.push_mqtt(
            info,
            format_args!("{}/ch{}", inout, bit + 1),
            bool2str(val & (1 << bit)),
        )
    }
    res
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Controller2 {
    info: DeviceInfo,
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

    fn register_1wire(&self) -> Vec<String> {
        vec!["SYS1_1".into(), "SYS2_1".into(), "SYS3".into()]
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::DIO(dio) => {
                debug!("[{}] DIO status: {}", dio.contno, dio.status);
                self.dio = dio.status;
                TwoWay::mqtt(self.info.topic("DIO"), dio.status)
            }
            Response::Devstatus(s) => {
                debug!("[{}] Controller2 {} => {:b}", s.contno, s.addr, s.val);
                match s.addr.as_ref() {
                    "SYS1_1" => digital_io(&self.info, 4, "in", s.val),
                    "SYS2_1" => digital_io(&self.info, 5, "out", s.val),
                    "SYS3" => TwoWay::mqtt(self.info.topic("out/ana"), centi2float(s.val)),
                    other => panic!("BUG: Unknown busaddr {}", other),
                }
            }
            _ => {
                warn!(
                    "[{}] Controller2: no handler for {:?}",
                    self.info.contno, resp
                );
                TwoWay::default()
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Switch8 {
    info: DeviceInfo,
}

impl Switch8 {
    new!(Switch8);
}

impl Device for Switch8 {
    std_methods!(Switch8);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 3])
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::Devstatus(s) => {
                debug!("[{}] Switch8 {} is {:b}", s.contno, s.addr, s.val);
                match s.addr.rsplit('_').nth(0).unwrap() {
                    "1" => digital_io(&self.info, 8, "in", s.val),
                    "3" => digital_io(&self.info, 8, "out", s.val),
                    other => panic!("BUG: Unknown busaddr {}", other),
                }
            }
            _ => {
                warn!("[{}] Switch8: no handler for {:?}", self.info.contno, resp);
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

    fn register_1wire(&self) -> Vec<String> {
        Vec::new()
    }
}
