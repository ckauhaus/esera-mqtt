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

    ow_sensor_handlers!(
        1 => "cur_12",
        2 => "vcc_12",
        3 => "cur_5",
        4 => "vcc_5"
    );

    fn announce(&self) -> Vec<MqttMsg> {
        let mut res = Vec::new();
        let dev = self.announce_device();
        for voltage in &[12, 5] {
            for (name, measure) in &[("cur", "current"), ("vcc", "voltage")] {
                let topic = format!("{}_{}", name, voltage);
                res.push(MqttMsg::new(
                    disc_topic("sensor", &self.info, format_args!("{}", topic)),
                    serde_json::to_string(&json!({
                        "availability_topic": self.info.topic("status"),
                        "device_class": measure,
                        "device": &dev,
                        "expire_after": 300,
                        "name": format!("Hub.{} {} {}V", self.info.contno, measure, voltage),
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
        cmp_ow(&mut uut, "1_OWD3_1|25119\n", "ESERA/1/HUB/cur_12", "251.19");
        cmp_ow(&mut uut, "1_OWD3_2|1201\n", "ESERA/1/HUB/vcc_12", "12.01");
        cmp_ow(&mut uut, "1_OWD3_3|19518\n", "ESERA/1/HUB/cur_5", "195.18");
        cmp_ow(&mut uut, "1_OWD3_4|490\n", "ESERA/1/HUB/vcc_5", "4.9");
    }
}
