use super::{digital_io, disc_topic, str2bool, AnnounceDevice, Result, Token};
use crate::{Device, DeviceInfo, MqttMsg, Response, TwoWay};

use serde_json::json;
fn ann_out_ch(dev: &AnnounceDevice, name: &str, info: &DeviceInfo, ch: usize) -> MqttMsg {
    MqttMsg::new(
        disc_topic("switch", &info, format_args!("ch{}", ch)),
        serde_json::to_string(&json!({
                "availability_topic": info.topic("status"),
                "command_topic": info.fmt(format_args!("/set/ch{}", ch)),
                "state_topic": info.fmt(format_args!("/out/ch{}", ch)),
                "device": dev,
                "name": format!("Switch {}/{} out {}", info.contno, name, ch),
                "payload_on": "1",
                "payload_off": "0",
                "unique_id": format!("{}_ch{}", info.serno, ch),
                "qos": 1,
            }
        ))
        .unwrap(),
    )
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

    fn announce(&self) -> Vec<MqttMsg> {
        let mut res = Vec::with_capacity(20);
        let dev = self.announce_device();
        let trigger = |ch, dir, pl| {
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
                    "qos": 1,
                    "topic": self.info.fmt(format_args!("/in/ch{}", ch)),
                    "type": format!("button_short_{}", dir),
                    "subtype": format!("button_{}", ch)
                }))
                .unwrap(),
            )
        };
        for ch in 1..=8 {
            for (dir, pl) in &[("press", "1"), ("release", "0")] {
                res.push(trigger(ch, dir, pl));
            }
            res.push(ann_out_ch(&dev, self.name(), &self.info, ch));
        }
        res
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        (1..=8)
            .map(|i| (self.info.fmt(format_args!("/set/ch{}", i)), i - 1))
            .collect()
    }

    fn handle_mqtt(&self, msg: MqttMsg, token: Token) -> Result<TwoWay> {
        let pl = msg.payload();
        Ok(match token {
            i if i >= 0 && i < 8 => TwoWay::from_1wire(format!(
                "SET,OWD,OUT,{},{},{}",
                self.info.devno(),
                i,
                str2bool(pl) as u8
            )),
            _ => TwoWay::default(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Switch8Out {
    info: DeviceInfo,
}

impl Switch8Out {
    new!(Switch8Out);
}

impl Device for Switch8Out {
    std_methods!(Switch8Out);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[3])
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::Devstatus(s) => {
                debug!("[{}] Switch8Out {} is {:b}", s.contno, s.addr, s.val);
                match s.addr.rsplit('_').nth(0).unwrap() {
                    "3" => digital_io(&self.info, 8, "out", s.val),
                    other => panic!("BUG: Unknown busaddr {}", other),
                }
            }
            _ => {
                warn!(
                    "[{}] Switch8Out: no handler for {:?}",
                    self.info.contno, resp
                );
                TwoWay::default()
            }
        })
    }

    fn announce(&self) -> Vec<MqttMsg> {
        let dev = self.announce_device();
        (1..=8)
            .map(|ch| ann_out_ch(&dev, self.name(), &self.info, ch))
            .collect()
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        let mut t = Vec::with_capacity(8);
        for i in 1..=8 {
            t.push((self.info.fmt(format_args!("/set/ch{}", i)), i - 1));
        }
        t
    }

    fn handle_mqtt(&self, msg: MqttMsg, token: Token) -> Result<TwoWay> {
        let pl = msg.payload();
        Ok(match token {
            i if i >= 0 && i < 8 => TwoWay::from_1wire(format!(
                "SET,OWD,OUT,{},{},{}",
                self.info.devno(),
                i,
                str2bool(pl) as u8
            )),
            _ => TwoWay::default(),
        })
    }
}
