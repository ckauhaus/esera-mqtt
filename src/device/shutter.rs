use super::{digital_io, Device, DeviceInfo, MqttMsg, Result, Token, TwoWay};
use crate::parser::{Msg, OW};

use serde_json::json;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shutter {
    info: DeviceInfo,
    buttons: i32,
}

impl Shutter {
    new!(Shutter);
}

impl Device for Shutter {
    std_methods!(Shutter);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 3])
    }

    fn handle_1wire(&mut self, ow: OW) -> Result<TwoWay> {
        Ok(match ow.msg {
            Msg::Devstatus(s) => match s.subaddr() {
                Some(1) => {
                    debug!(
                        "[{}] Switch8 {} buttons={:02b}",
                        ow.contno,
                        self.name(),
                        s.val
                    );
                    let res = digital_io(&self.info, 2, "in", s.val, None)
                        + digital_io(&self.info, 2, "button", s.val, Some(self.buttons));
                    self.buttons = s.val;
                    res
                }
                Some(3) => {
                    debug!(
                        "[{}] Switch8 {} state={:02b}",
                        ow.contno,
                        self.name(),
                        s.val
                    );
                    let state = match s.val & 0b11 {
                        0b01 => "closing",
                        0b10 => "opening",
                        _ => "stopped",
                    };
                    TwoWay::from_mqtt(MqttMsg::new(self.info.topic("state"), state))
                }
                _ => panic!("BUG: Unknown busaddr {}", s.addr),
            },
            _ => {
                warn!(
                    "[{}] Shutter {}: no handler for {:?}",
                    ow.contno,
                    self.name(),
                    ow
                );
                TwoWay::default()
            }
        })
    }

    /// Channel 1: down/close
    /// Channel 2: up/open
    fn announce(&self) -> Vec<MqttMsg> {
        let mut res = Vec::with_capacity(10);
        let dev = self.announce_device();
        let i = &self.info;
        for button in &[1, 2] {
            res.push(self.announce_trigger(&dev, *button, "short", "0"));
            res.push(self.announce_trigger(&dev, *button, "short", "1"));
        }
        res.push(MqttMsg::new(
            format!(
                "homeassistant/cover/{}/{}/config",
                i.contno,
                i.serno.replace(
                    |c: char| { !c.is_ascii_alphanumeric() && c != '_' && c != '-' },
                    ""
                )
            ),
            serde_json::to_string(&json!({
                "availability_topic": i.status_topic(),
                "command_topic": i.topic("set"),
                "device": dev,
                "name": format!("Shutter {}/{}", i.contno, self.name()),
                "state_topic": i.topic("state"),
                "unique_id": i.serno,
                "optimistic": false,
            }))
            .unwrap(),
        ));
        res
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        vec![(self.info.topic("set"), 0)]
    }

    fn handle_mqtt(&self, msg: &MqttMsg, _: Token) -> Result<TwoWay> {
        let pl = msg.payload();
        debug!(
            "[{}] Shutter {}: MQTT: set {}",
            self.info.contno,
            self.name(),
            pl
        );
        let d = self.info.devno();
        Ok(match pl {
            "CLOSE" => TwoWay::from_1wire(format!("SET,OWD,SHT,{},1", d)),
            "OPEN" => TwoWay::from_1wire(format!("SET,OWD,SHT,{},2", d)),
            "STOP" => TwoWay::from_1wire(format!("SET,OWD,SHT,{},3", d)),
            _ => {
                error!(
                    "[{}] Dimmer {}: unrecognized MQTT command {}",
                    self.info.contno,
                    self.name(),
                    pl
                );
                TwoWay::default()
            }
        })
    }
}
