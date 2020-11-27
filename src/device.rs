use crate::{parser, DeviceInfo, MqttMsg, Response, Status};

use crossbeam::channel::Sender;
use enum_dispatch::enum_dispatch;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Controller communication: {0}")]
    Connection(#[from] crate::controller::Error),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[enum_dispatch]
pub trait Device {
    /// Device initialization and retained MQTT info. Prepare to be called several times.
    fn setup<S>(&mut self, _conn: Sender<String>) {}

    fn handle_1wire(&mut self, _resp: Response) -> Result<Vec<MqttMsg>> {
        Ok(Vec::default())
    }

    fn handle_mqtt<S>(&self, _msg: MqttMsg) -> Result<Vec<String>> {
        Ok(Vec::default())
    }

    fn display(&self) -> String;
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
    fn select(info: DeviceInfo) -> Self {
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Controller2 {
    info: DeviceInfo,
    inputs: u8,
    outputs: u8,
    ana: f32,
    dio: parser::DIO,
}

impl Controller2 {
    new!(Controller2);
}

impl Device for Controller2 {
    fn display<'a>(&'a self) -> String {
        format!(
            "Controller2 ({}) @ {}/{}",
            self.info.artno, self.info.contno, self.info.serno
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
    fn display<'a>(&'a self) -> String {
        format!(
            "Unknown device ({}) @ {}/{}",
            self.info.artno, self.info.contno, self.info.serno
        )
    }
}
//         ctrl.send_line(&format!("SET,SYS,DATE,{}", now.format("%d.%m.%y")))
//             .await?;
//         pick(ctrl, parser::date).await?;
//         ctrl.send_line(&format!("SET,SYS,TIME,{}", now.format("%H:%M:%S")))
//             .await?;
//         pick(ctrl, parser::time).await?;
//         ctrl.send_line("SET,SYS,DATATIME,30").await?;
//         pick(ctrl, parser::datatime).await?;
//         ctrl.send_line("GET,SYS,DIO").await?;
//         self.dio = pick(ctrl, parser::dio).await?;
//         Ok(vec![("SYS/DIO".to_owned(), self.dio.to_string())])
