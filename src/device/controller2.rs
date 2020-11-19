use super::{boolstr, Device, Model};
use crate::bus::MqttMsgs;
use crate::{parser, pick, Controller, Dio, Response};

use chrono::Local;
use futures::future::BoxFuture;
use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Don't know how to handle controller response {0:?}")]
    Unknown(Response),
    #[error("Trying to set non existing controller port {0}")]
    NoPort(String),
    #[error("Invalid data item {0}")]
    Invalid(String),
}

type Result<T, E = super::Error> = std::result::Result<T, E>;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Controller2 {
    inputs: u8,
    outputs: u8,
    ana: f32,
    dio: Dio,
}

impl Controller2 {
    async fn _init(&mut self, ctrl: &mut (dyn Controller + Send)) -> Result<()> {
        let now = Local::now();
        ctrl.send_line(&format!("SET,SYS,DATE,{}", now.format("%d.%m.%y")))
            .await?;
        pick(ctrl, parser::date).await?;
        ctrl.send_line(&format!("SET,SYS,TIME,{}", now.format("%H:%M:%S")))
            .await?;
        pick(ctrl, parser::time).await?;
        ctrl.send_line("SET,SYS,DATATIME,30").await?;
        pick(ctrl, parser::datatime).await?;
        ctrl.send_line("GET,SYS,DIO").await?;
        self.dio = pick(ctrl, parser::dio).await?;
        Ok(())
    }

    fn set_output(&mut self, port: &str, value: &[u8]) -> Result<String> {
        let value = String::from_utf8_lossy(value);
        let n = port.parse::<u8>().map_err(|_| Error::NoPort(port.into()))?;
        if n >= 1 && n <= 5 {
            // digital outputs
            let flag = match value {
                "0" => 0,
                "1" => 1,
                _ => return Err(Error::Invalid(value.into()).into()),
            };
            Ok(format!("SET,SYS,OUT,{},{}", n, flag))
        } else if n == 6 {
            // analog output
            let volt = value
                .parse::<f32>()
                .map_err(|_| Error::Invalid(value.into()))?
                * 100.0;
            if volt > 1000.0 || volt < 0.0 {
                return Error::Invalid(value.into()).into();
            }
            Ok(format!("SET,SYS,OUTA,{0:.0}", volt))
        } else {
            Err(Error::NoPort(port.into()).into())
        }
    }
}

lazy_static! {
    static ref R_SYSOUT: Regex = Regex::new(r"^SYS/OUT/(\d)/set$").unwrap();
}

impl Model for Controller2 {
    fn init<'a>(&'a mut self, ctrl: &'a mut (dyn Controller + Send)) -> BoxFuture<'a, Result<()>> {
        Box::pin(self._init(ctrl))
    }

    fn register_addrs(&self, _dev: &Device) -> Vec<String> {
        vec!["SYS1_1".into(), "SYS2_1".into(), "SYS3".into()]
    }

    fn generic_update(&mut self, resp: Response) -> Result<MqttMsgs> {
        Ok(match resp {
            Response::DIO(mode) => {
                self.dio = mode;
                vec![("SYS/DIO".into(), mode.to_string())]
            }
            r => return Err(Error::Unknown(r).into()),
        })
    }

    fn status_update(&mut self, addr: &str, data: u32) -> MqttMsgs {
        let mut res = Vec::new();
        match addr {
            "SYS1_1" => {
                self.inputs = (data & 0xff) as u8;
                for bit in 0..4 {
                    res.push((
                        format!("SYS/in/{}", bit + 1),
                        boolstr(data & 1 << bit).into(),
                    ))
                }
            }
            "SYS2_1" => {
                self.outputs = (data & 0xff) as u8;
                for bit in 0..5 {
                    res.push((
                        format!("SYS/out/{}", bit + 1),
                        boolstr(data & 1 << bit).into(),
                    ))
                }
            }
            "SYS3" => {
                let val = f32::from(data as u16) / 100.0;
                self.ana = val;
                res.push(("SYS/out/6".into(), format!("{:.2}", val)))
            }
            _ => warn!("Controller2: unknown bus addr '{}', ignoring", addr),
        }
        res
    }

    fn register_topics(&self, _dev: &Device) -> Vec<String> {
        let mut topics: Vec<String> = (1..=6)
            .map(|port| format!("SYS/out/{}/set", port))
            .collect();
        topics.push("SYS/DIO/set".into());
        topics
    }

    fn process_msg(&mut self, topic: &str, payload: &[u8]) -> Result<Vec<String>> {
        let mut res = Vec::new();
        if let Some(m) = R_SYSOUT.captures(topic) {
            return Ok(self.set_output(m[1], payload).map_err);
        }
        Ok(res)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::convert::AsRef;

    #[test]
    fn process_controller_event() {
        assert_eq!(
            Controller2::default().status_update("SYS1_1", 9),
            vec![
                ("SYS/in/1".into(), "1".into()),
                ("SYS/in/2".into(), "0".into()),
                ("SYS/in/3".into(), "0".into()),
                ("SYS/in/4".into(), "1".into())
            ]
        );
        assert_eq!(
            Controller2::default().status_update("SYS2_1", 12),
            vec![
                ("SYS/out/1".into(), "0".into()),
                ("SYS/out/2".into(), "0".into()),
                ("SYS/out/3".into(), "1".into()),
                ("SYS/out/4".into(), "1".into()),
                ("SYS/out/5".into(), "0".into())
            ]
        );
        assert_eq!(
            Controller2::default().status_update("SYS3", 526),
            vec![("SYS/out/6".into(), "5.26".into())]
        );
    }

    #[test]
    fn dio_conversions() {
        assert_eq!("2".parse::<Dio>().unwrap(), Dio::LinkedLevel);
        assert_eq!("LINKED+LEVEL".parse::<Dio>().unwrap(), Dio::LinkedLevel);
        assert_eq!(Dio::IndependentEdge.as_ref(), "INDEPENDENT+EDGE");
    }
}
