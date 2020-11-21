use crate::{parser, pick, ControllerConnection, MqttMsg, Response, Status};

use enum_dispatch::enum_dispatch;
use std::fmt;
use std::io::prelude::*;
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
    fn setup<S>(&mut self, _conn: &mut ControllerConnection<S>) -> Result<Vec<MqttMsg>>
    where
        S: Read + Write + fmt::Debug,
    {
        Ok(Vec::default())
    }

    fn handle_1wire(&mut self, _resp: Response) -> Result<Vec<MqttMsg>> {
        Ok(Vec::default())
    }

    fn handle_mqtt<S>(&self, _msg: MqttMsg, _conn: &mut ControllerConnection<S>) -> Result<()>
    where
        S: Read + Write + fmt::Debug,
    {
        Ok(())
    }
}

/// Generic parameters common to all devices on the bus
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeviceInfo {
    pub artno: String,
    pub contno: u8,
    pub busid: String,
    pub serno: String,
    pub status: Status,
    pub name: Option<String>,
}

impl DeviceInfo {
    fn new(
        artno: String,
        contno: u8,
        busid: String,
        serno: String,
        status: Status,
        name: Option<String>,
    ) -> Self {
        Self {
            artno,
            contno,
            busid,
            serno,
            status,
            name,
        }
    }

    fn mqtt_msg<T: AsRef<str>, P: Into<String>>(&self, topic: T, payload: P) -> MqttMsg {
        (
            format!("ESERA/{}/{}", self.contno, topic.as_ref()),
            payload.into(),
        )
    }
}

#[enum_dispatch(Device)]
pub enum Model {
    Controller2(Controller2),
    // HubIII(HubIII),
    // Dimmer1(Dimmer1),
    // Switch8_16A(Switch8_16A),
    Unknown(Unknown),
}

impl Model {
    fn select(
        artno: String,
        contno: u8,
        busid: String,
        serno: String,
        status: Status,
        name: Option<String>,
    ) -> Self {
        let a = artno.clone();
        let info = DeviceInfo::new(artno, contno, busid, serno, status, name);
        match &*a {
            "11340" => Self::Controller2(Controller2::new(info)),
            //         "11221" => Box::new(Dimmer1::default()),
            //         "11228" => Box::new(Switch8_16A::default()),
            //         "11322" => Box::new(HubIII::default()),
            _ => Self::Unknown(Unknown::new(info)),
        }
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

impl Device for Controller2 {}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Unknown {
    info: DeviceInfo,
}

impl Unknown {
    new!(Unknown);
}

impl Device for Unknown {}
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
