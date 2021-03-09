use super::{centi2float, digital_io, disc_topic, float2centi, str2bool, Error, Result, Token};
use crate::parser::{Msg, DIO, OW};
use crate::{Device, DeviceInfo, MqttMsg, TwoWay};

use serde_json::json;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Controller2 {
    info: DeviceInfo,
    dio: DIO,
    sw_version: String,
    inputs: i32,
}

impl Controller2 {
    new!(Controller2);
}

impl Device for Controller2 {
    std_methods!(Controller2);

    fn init(&mut self) -> Vec<String> {
        vec!["SET,SYS,OUTA,500".into(), "GET,SYS,DIO".into()]
    }

    fn register_1wire(&self) -> Vec<String> {
        vec!["SYS1_1".into(), "SYS2_1".into(), "SYS3".into()]
    }

    fn handle_1wire(&mut self, resp: OW) -> Result<TwoWay> {
        Ok(match resp.msg {
            Msg::CSI(csi) => {
                self.sw_version = csi.fw;
                TwoWay::mqtt(self.announce())
            }
            Msg::DIO(dio) => {
                debug!("[{}] DIO status: {}", resp.contno, dio);
                self.dio = dio;
                TwoWay::from_mqtt(self.info.mqtt_msg("dio", dio))
            }
            Msg::Devstatus(s) => {
                debug!("[{}] Controller2 {} => {:b}", resp.contno, s.addr, s.val);
                match s.addr.as_ref() {
                    "SYS1_1" => {
                        let res = digital_io(&self.info, 4, "in", s.val, None)
                            + digital_io(&self.info, 4, "button", s.val, Some(self.inputs));
                        self.inputs = s.val;
                        res
                    }
                    "SYS2_1" => digital_io(&self.info, 5, "out", s.val, None),
                    "SYS3" => TwoWay::from_mqtt(self.info.mqtt_msg("out/ana", centi2float(s.val))),
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

    fn announce(&self) -> Vec<MqttMsg> {
        let mut dev = self.announce_device();
        dev.sw_version = Some(self.sw_version.clone());
        dev.via_device = None;
        let mut res = Vec::with_capacity(20);
        let binary_sensor = |ch| {
            MqttMsg::new(
                disc_topic("binary_sensor", &self.info, format_args!("button_{}", ch)),
                serde_json::to_string(&json!({
                        "availability_topic": self.info.status_topic(),
                        "state_topic": self.info.fmt(format_args!("in/ch{}", ch)),
                        "device": &dev,
                        "name": format!("Controller.{} in {}", self.info.contno, ch),
                        "payload_on": "1",
                        "payload_off": "0",
                        "unique_id": format!("{}_in{}", self.info.serno, ch),
                }
                ))
                .unwrap(),
            )
        };
        let dur = match self.dio {
            DIO::LinkedEdge | DIO::IndependentEdge => "short",
            DIO::LinkedLevel | DIO::IndependentLevel => "long",
        };
        for ch in 1..=4 {
            res.push(self.announce_trigger(&dev, ch, dur, "0"));
            res.push(self.announce_trigger(&dev, ch, dur, "1"));
            res.push(binary_sensor(ch));
        }
        for ch in 1..=5 {
            res.push(MqttMsg::new(
                disc_topic("switch", &self.info, format_args!("ch{}", ch)),
                serde_json::to_string(&json!({
                        "availability_topic": self.info.status_topic(),
                        "command_topic": self.info.fmt(format_args!("set/ch{}", ch)),
                        "state_topic": self.info.fmt(format_args!("out/ch{}", ch)),
                        "device": &dev,
                        "name": format!("Controller.{} out {}", self.info.contno, ch),
                        "payload_on": "1",
                        "payload_off": "0",
                        "unique_id": format!("{}_ch{}", self.info.serno, ch),
                    }
                ))
                .unwrap(),
            ));
        }
        res.push(MqttMsg::new(
            disc_topic("light", &self.info, format_args!("ana")),
            serde_json::to_string(&json!({
                    "availability_topic": self.info.status_topic(),
                    "brightness_command_topic": self.info.topic("set/ana"),
                    "brightness_state_topic": self.info.topic("out/ana"),
                    "brightness_scale": 10.0,
                    "device": &dev,
                    "command_topic": self.info.topic("set/ana"),
                    "name": format!("Controller.{} analog out", self.info.contno),
                    "unique_id": format!("{}_ana", self.info.serno)
                }
            ))
            .unwrap(),
        ));
        res
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        let mut t = Vec::with_capacity(20);
        for i in 1..=5 {
            t.push((self.info.fmt(format_args!("set/ch{}", i)), i));
        }
        t.push((self.info.topic("set/ana"), 6));
        t
    }

    fn handle_mqtt(&self, msg: &MqttMsg, token: Token) -> Result<TwoWay> {
        let pl = msg.payload();
        Ok(match token {
            i @ 1..=5 => TwoWay::from_1wire(format!("SET,SYS,OUT,{},{}", i, str2bool(pl) as u8)),
            6 => {
                let val: f32 = pl.parse().map_err(|_| Error::Value(pl.into()))?;
                if !(0.0..=10.0).contains(&val) {
                    return Err(Error::Value(pl.into()));
                } else {
                    TwoWay::from_1wire(format!("SET,SYS,OUTA,{}", float2centi(val)))
                }
            }
            _ => TwoWay::default(),
        })
    }
}
