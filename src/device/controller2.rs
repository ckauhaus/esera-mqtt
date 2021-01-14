use super::{centi2float, digital_io, disc_topic, float2centi, str2bool, Error, Result, Token};
use crate::{parser::DIOStatus, Device, DeviceInfo, MqttMsg, Response, TwoWay};

use serde_json::json;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Controller2 {
    info: DeviceInfo,
    dio: DIOStatus,
    sw_version: String,
}

impl Controller2 {
    new!(Controller2);
}

impl Device for Controller2 {
    std_methods!(Controller2);

    fn init(&mut self) -> Vec<String> {
        vec!["GET,SYS,DIO".into()]
    }

    fn register_1wire(&self) -> Vec<String> {
        vec!["SYS1_1".into(), "SYS2_1".into(), "SYS3".into()]
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::CSI(csi) => {
                self.sw_version = csi.fw;
                TwoWay::mqtt(self.announce())
            }
            Response::DIO(dio) => {
                debug!("[{}] DIO status: {}", dio.contno, dio.status);
                self.dio = dio.status;
                TwoWay::from_mqtt(self.info.mqtt_msg("dio", dio.status))
            }
            Response::Devstatus(s) => {
                debug!("[{}] Controller2 {} => {:b}", s.contno, s.addr, s.val);
                match s.addr.as_ref() {
                    "SYS1_1" => digital_io(&self.info, 4, "in", s.val),
                    "SYS2_1" => digital_io(&self.info, 5, "out", s.val),
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
        let trigger = |ch, dur, dir, pl| {
            MqttMsg::new(
                disc_topic(
                    "device_automation",
                    &self.info,
                    format_args!("button_{}_{}", ch, dir),
                ),
                serde_json::to_string(&json!({
                    "device": &dev,
                    "automation_type": "trigger",
                    "payload": pl,
                    "topic": self.info.fmt(format_args!("/in/ch{}", ch)),
                    "type": format!("button_{}_{}", dur, dir),
                    "subtype": format!("button_{}", ch),
                }))
                .unwrap(),
            )
        };
        let dur = match self.dio {
            DIOStatus::LinkedEdge | DIOStatus::IndependentEdge => "short",
            DIOStatus::LinkedLevel | DIOStatus::IndependentLevel => "long",
        };
        for ch in 1..=4 {
            for (dir, pl) in &[("press", "1"), ("release", "0")] {
                res.push(trigger(ch, dur, dir, pl));
            }
        }
        for ch in 1..=5 {
            res.push(MqttMsg::new(
                disc_topic("switch", &self.info, format_args!("ch{}", ch)),
                serde_json::to_string(&json!({
                        "availability_topic": self.info.topic("status"),
                        "command_topic": self.info.fmt(format_args!("/set/ch{}", ch)),
                        "state_topic": self.info.fmt(format_args!("/out/ch{}", ch)),
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
                    "availability_topic": self.info.topic("status"),
                    "brightness_command_topic": self.info.topic("/set/ana"),
                    "brightness_state_topic": self.info.topic("/out/ana"),
                    "brightness_scale": 10.0,
                    "device": &dev,
                    "command_topic": self.info.topic("/set/ana"),
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
            t.push((self.info.fmt(format_args!("/set/ch{}", i)), i));
        }
        t.push((self.info.topic("set/ana"), 6));
        t
    }

    fn handle_mqtt(&self, msg: MqttMsg, token: Token) -> Result<TwoWay> {
        let pl = msg.payload();
        Ok(match token {
            i if i >= 1 && i <= 5 => {
                TwoWay::from_1wire(format!("SET,SYS,OUT,{},{}", i, str2bool(pl) as u8))
            }
            6 => {
                let val: f32 = pl.parse().map_err(|_| Error::Value(pl.into()))?;
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
