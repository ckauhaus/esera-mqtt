use super::{centi2float, digital_io, float2centi, str2bool, Error, Result, Token};
use crate::{parser::DIOStatus, Device, DeviceInfo, MqttMsg, Response, TwoWay};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Controller2 {
    info: DeviceInfo,
    dio: DIOStatus,
}

impl Controller2 {
    new!(Controller2);
}

impl Device for Controller2 {
    std_methods!(Controller2);

    fn init(&self) -> TwoWay {
        TwoWay::from_1wire("GET,SYS,DIO")
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

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        let mut t = Vec::with_capacity(20);
        for i in 1..=4 {
            t.push((self.info.fmt(format_args!("/set/ch{}", i)), i));
            t.push((self.info.fmt(format_args!("/out/ch{}", i)), -1));
            t.push((self.info.fmt(format_args!("/in/ch{}", i)), -1));
        }
        t.push((self.info.topic("set/ana"), 5));
        t.push((self.info.topic("out/ch5"), -1));
        t.push((self.info.topic("out/ana"), -1));
        t.push((self.info.topic("DIO"), -1));
        t
    }

    fn handle_mqtt(&self, msg: MqttMsg, token: Token) -> Result<TwoWay> {
        let pl = msg.payload();
        Ok(match token {
            i if i > 0 && i <= 4 => {
                TwoWay::from_1wire(format!("SET,SYS,OUT,{},{}", i, str2bool(pl) as u8))
            }
            5 => {
                let val: f32 = pl.parse().or(Err(Error::Value(pl.into())))?;
                if val < 0.0 || val > 10.0 {
                    return Err(Error::Value(pl.into()));
                } else {
                    TwoWay::from_1wire(format!("SET,SYS,OUTA,{}", float2centi(val)))
                }
            }
            _ => TwoWay::default(),
        })
    }
}
