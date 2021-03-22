use super::{digital_io, disc_topic, str2bool, AnnounceDevice, Result, Token};
use crate::parser::{Msg, OW};
use crate::{Device, DeviceInfo, MqttMsg, TwoWay};

use serde_json::json;

fn ann_out_ch(dev: &AnnounceDevice, name: &str, info: &DeviceInfo, ch: u8) -> MqttMsg {
    MqttMsg::retain(
        disc_topic("switch", &info, format_args!("ch{}", ch)),
        serde_json::to_string(&json!({
                "availability_topic": info.status_topic(),
                "command_topic": info.fmt(format_args!("set/ch{}", ch)),
                "state_topic": info.fmt(format_args!("out/ch{}", ch)),
                "device": dev,
                "name": format!("Switch {}/{}.{}", info.contno, name, ch),
                "payload_on": "1",
                "payload_off": "0",
                "unique_id": format!("{}_ch{}", info.serno, ch),
            }
        ))
        .unwrap(),
    )
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Switch8 {
    info: DeviceInfo,
    inputs: i32,
}

impl Switch8 {
    new!(Switch8);
}

impl Device for Switch8 {
    std_methods!(Switch8);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 3])
    }

    fn handle_1wire(&mut self, resp: OW) -> Result<TwoWay> {
        Ok(match resp.msg {
            Msg::Devstatus(s) => match s.subaddr() {
                Some(1) => {
                    debug!(
                        "[{}] Switch8 {} inputs={:08b}",
                        resp.contno,
                        self.name(),
                        s.val
                    );
                    let res = digital_io(&self.info, 8, "in", s.val, None)
                        + digital_io(&self.info, 8, "button", s.val, Some(self.inputs));
                    self.inputs = s.val;
                    res
                }
                Some(3) => {
                    debug!(
                        "[{}] Switch8 {} outputs={:08b}",
                        resp.contno,
                        self.name(),
                        s.val
                    );
                    digital_io(&self.info, 8, "out", s.val, None)
                }
                _ => panic!("BUG: Unknown busaddr {}", s.addr),
            },
            _ => {
                warn!("[{}] Switch8: no handler for {:?}", self.info.contno, resp);
                TwoWay::default()
            }
        })
    }

    fn announce(&self) -> Vec<MqttMsg> {
        let mut res = Vec::with_capacity(20);
        let dev = self.announce_device();
        for ch in 1..=8 {
            res.push(self.announce_trigger(&dev, ch, "short", "0"));
            res.push(self.announce_trigger(&dev, ch, "short", "1"));
            res.push(ann_out_ch(&dev, self.name(), &self.info, ch));
        }
        res
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        (1..=8)
            .map(|i| (self.info.fmt(format_args!("set/ch{}", i)), i - 1))
            .collect()
    }

    fn handle_mqtt(&self, msg: &MqttMsg, token: Token) -> Result<TwoWay> {
        let pl = msg.payload();
        debug!("[{}] Switch8: handle {}", self.info.contno, pl);
        Ok(match token {
            i @ 0..=7 => TwoWay::from_1wire(format!(
                "SET,OWD,OUT,{},{},{}",
                self.info.devno(),
                i,
                str2bool(pl) as u8
            )),
            _ => {
                warn!("[{}] Switch8: invalid token {}", self.info.contno, token);
                TwoWay::default()
            }
        })
    }
}
