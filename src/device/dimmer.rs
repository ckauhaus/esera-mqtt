use super::{bool2str, disc_topic, Error, Result, Token};
use crate::parser::{Msg, OW};
use crate::{Device, DeviceInfo, MqttMsg, TwoWay};

use serde_json::json;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dimmer {
    info: DeviceInfo,
}

impl Dimmer {
    new!(Dimmer);
}

impl Device for Dimmer {
    std_methods!(Dimmer);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 3, 4])
    }

    fn handle_1wire(&mut self, ow: OW) -> Result<TwoWay> {
        let mut res = TwoWay::default();
        match ow.msg {
            Msg::Devstatus(s) => {
                let func = s.addr.rsplit('_').next().unwrap();
                match func {
                    "1" => {
                        debug!(
                            "[{}] Dimmer {} buttons={:04b}",
                            ow.contno,
                            self.name(),
                            s.val
                        );
                        res += TwoWay::from_mqtt(MqttMsg::new(
                            self.info.topic("in/ch1"),
                            bool2str(s.val as u32 & 0b01),
                        ));
                        res += TwoWay::from_mqtt(MqttMsg::new(
                            self.info.topic("in/ch2"),
                            bool2str(s.val as u32 & 0b10),
                        ));
                    }
                    "3" | "4" => {
                        let ch = func.parse::<u8>().unwrap() - 2;
                        debug!(
                            "[{}] Dimmer {} channel{}={}",
                            ow.contno,
                            self.name(),
                            ch,
                            s.val
                        );
                        res += TwoWay::from_mqtt(MqttMsg::new(
                            self.info.fmt(format_args!("out/ch{}", ch)),
                            s.val,
                        ));
                    }
                    _ => warn!(
                        "[{}] Dimmer {}: unknown device address {}",
                        ow.contno,
                        self.name(),
                        s.addr
                    ),
                }
            }
            _ => {
                warn!(
                    "[{}] Dimmer {}: no handler for {:?}",
                    ow.contno,
                    self.name(),
                    ow
                );
            }
        }
        Ok(res)
    }

    fn announce(&self) -> Vec<MqttMsg> {
        let mut res = Vec::new();
        let dev = self.announce_device();
        for ch in &[1, 2] {
            res.push(self.announce_trigger(&dev, *ch, "short", "0"));
            res.push(self.announce_trigger(&dev, *ch, "short", "1"));
            res.push(MqttMsg::new(
                disc_topic("light", &self.info, format_args!("ch{}", ch)),
                serde_json::to_string(&json!({
                    "availability_topic": self.info.status_topic(),
                    "brightness_command_topic": self.info.fmt(format_args!("set/ch{}", ch)),
                    "command_topic": self.info.fmt(format_args!("set/ch{}", ch)),
                    "brightness_scale": 31,
                    "brightness_state_topic": self.info.fmt(format_args!("out/ch{}", ch)),
                    "device": dev,
                    "name": format!("Dimmer {}/{}.{}", self.info.contno, self.name(), ch),
                    "payload_on": "1",
                    "payload_off": "0",
                    "state_value_template": "{% if value != '0' %}1{% else %}0{% endif %}",
                    "state_topic": self.info.fmt(format_args!("out/ch{}", ch)),
                    "on_command_type": "brightness",
                    "unique_id": format!("{}_ch{}", self.info.serno, ch),
                }
                ))
                .unwrap(),
            ))
        }
        res
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        vec![
            (self.info.fmt(format_args!("set/ch1")), 1),
            (self.info.fmt(format_args!("set/ch2")), 2),
        ]
    }

    fn handle_mqtt(&self, msg: &MqttMsg, token: Token) -> Result<TwoWay> {
        let val: u8 = msg
            .payload()
            .parse()
            .map_err(|_| Error::Value("0..32".into()))?;
        debug!(
            "[{}] Dimmer {}: MQTT: set channel {} to {}",
            self.info.contno,
            self.name(),
            token,
            val
        );
        match (token, val) {
            (ch, val) if (1..=2).contains(&ch) && (0..32).contains(&val) => {
                return Ok(TwoWay::from_1wire(format!(
                    "SET,OWD,DIM,{},{},{}",
                    self.info.devno(),
                    ch,
                    val
                )))
            }
            _ => warn!(
                "[{}] Dimmer {}: invalid MQTT message {:?}",
                self.info.contno,
                self.name(),
                msg
            ),
        };
        Ok(TwoWay::default())
    }
}
