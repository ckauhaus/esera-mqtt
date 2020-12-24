use super::{digital_io, disc_topic, Result};
use crate::{Device, DeviceInfo, MqttMsg, Response, TwoWay};

use serde_json::json;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BinarySensor {
    info: DeviceInfo,
}

impl BinarySensor {
    new!(BinarySensor);
}

impl Device for BinarySensor {
    std_methods!(BinarySensor);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1])
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::Devstatus(s) => {
                debug!("[{}] BinarySensor {} is {:b}", s.contno, s.addr, s.val);
                match s.addr.rsplit('_').next().unwrap() {
                    "1" => digital_io(&self.info, 8, "in", s.val),
                    other => panic!("BUG: Unknown busaddr {}", other),
                }
            }
            _ => {
                warn!(
                    "[{}] BinarySensor: no handler for {:?}",
                    self.info.contno, resp
                );
                TwoWay::default()
            }
        })
    }

    fn announce(&self) -> Vec<MqttMsg> {
        let dev = self.announce_device();
        (1..=8)
            .map({
                |ch| {
                    MqttMsg::new(
                        disc_topic("binary_sensor", &self.info, format_args!("ch{}", ch)),
                        serde_json::to_string(&json!({
                            "availability_topic": self.info.topic("status"),
                            "device": &dev,
                            "expire_after": 300,
                            "name": format!("In {}/{}.{}", self.info.contno, self.name(), ch),
                            "payload_off": "0",
                            "payload_on": "1",
                            "state_topic": self.info.fmt(format_args!("/in/ch{}", ch)),
                            "unique_id": format!("{}_ch{}", self.info.serno, ch),
                        }))
                        .unwrap(),
                    )
                }
            })
            .collect()
    }
}
