use super::{centi2float, disc_topic, Result};
use crate::parser::{Msg, OW};
use crate::{Device, DeviceInfo, MqttMsg, TwoWay};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hub {
    info: DeviceInfo,
}

impl Hub {
    new!(Hub);
}

impl Device for Hub {
    std_methods!(Hub);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 2, 3, 4])
    }

    fn handle_1wire(&mut self, resp: OW) -> Result<TwoWay> {
        Ok(match resp.msg {
            Msg::Devstatus(s) => {
                debug!("[{}] Hub {} is {}", resp.contno, s.addr, s.val);
                match s.addr.rsplit('_').next().unwrap() {
                    "1" => TwoWay::from_mqtt(self.info.mqtt_msg("curr_12", centi2float(s.val))),
                    "2" => TwoWay::from_mqtt(self.info.mqtt_msg("volt_12", centi2float(s.val))),
                    "3" => TwoWay::from_mqtt(self.info.mqtt_msg("curr_5", centi2float(s.val))),
                    "4" => TwoWay::from_mqtt(self.info.mqtt_msg("volt_5", centi2float(s.val))),
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
        let mut res = Vec::new();
        let dev = self.announce_device();
        for voltage in &[12, 5] {
            for measure in &["current", "voltage"] {
                let topic = format!("{}_{}", &measure[0..4], voltage);
                res.push(MqttMsg::new(
                    disc_topic("sensor", &self.info, format_args!("{}", topic)),
                    serde_json::to_string(&json!({
                        "availability_topic": self.info.topic("status"),
                        "device_class": measure,
                        "device": &dev,
                        "expire_after": 300,
                        "name": format!("Hub {} {}V", measure, voltage),
                        "state_topic": self.info.topic(&topic),
                        "unique_id": format!("{}_{}", self.info.serno, topic),
                        "unit_of_measurement": if *measure == "current" { "mA" } else { "V" },
                    }))
                    .unwrap(),
                ))
            }
        }
        res
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test::cmp_ow;

    #[test]
    fn hub_devstatus() {
        let mut uut = Hub::new(DeviceInfo::new(1, "OWD1", "", "online", "", Some("HUB")).unwrap());
        cmp_ow(
            &mut uut,
            "1_OWD3_1|25119\n",
            "ESERA/1/HUB/curr_12",
            "251.19",
        );
        cmp_ow(&mut uut, "1_OWD3_2|1201\n", "ESERA/1/HUB/volt_12", "12.01");
        cmp_ow(&mut uut, "1_OWD3_3|19518\n", "ESERA/1/HUB/curr_5", "195.18");
        cmp_ow(&mut uut, "1_OWD3_4|490\n", "ESERA/1/HUB/volt_5", "4.9");
    }
}
