use super::{centi2float, Result, Token};
use crate::{Device, DeviceInfo, MqttMsg, Response, TwoWay};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TempHum {
    info: DeviceInfo,
}

impl TempHum {
    new!(TempHum);

    fn disc_topic(&self, sub: &str) -> String {
        format!(
            "homeassistant/sensor/{}/{}_{}/config",
            self.info.contno, self.info.serno, sub
        )
    }
}

impl Device for TempHum {
    std_methods!(TempHum);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 3, 4])
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::Devstatus(s) => match s.addr.rsplit('_').nth(0).unwrap() {
                "1" => TwoWay::mqtt(self.info.topic("temp"), centi2float(s.val)),
                "3" => TwoWay::mqtt(self.info.topic("hum"), centi2float(s.val)),
                "4" => TwoWay::mqtt(self.info.topic("dew"), centi2float(s.val)),
                other => panic!("BUG: Unknown busaddr {}", other),
            },
            _ => {
                warn!("[{}] TempHum: no handler for {:?}", self.info.contno, resp);
                TwoWay::default()
            }
        })
    }

    fn announce(&self) -> Vec<MqttMsg> {
        let dev = self.announce_device();
        let mut res = Vec::with_capacity(3);
        for (name, short, class, uom) in &[
            ("Temperature", "temp", "temperature", "°C"),
            ("Humidity", "hum", "humidity", "%"),
            ("Dewpoint", "dew", "temperature", "°C"),
        ] {
            res.push(MqttMsg::new(
                self.disc_topic(short),
                serde_json::to_string(&json!({
                    "availability_topic": self.info.topic("status"),
                    "device": &dev,
                    "device_class": class,
                    "expire_after": 600,
                    "name": format!("{} {}", self.name(), name),
                    "unique_id": format!("{}_{}", self.info.serno, short),
                    "state_topic": self.info.topic(short),
                    "unit_of_measurement": uom
                }))
                .unwrap(),
            ));
        }
        res
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        vec![
            (self.info.topic("temp"), -1),
            (self.info.topic("hum"), -1),
            (self.info.topic("dew"), -1),
            (self.info.topic("status"), -1),
        ]
    }

    fn handle_mqtt(&self, _msg: MqttMsg, _token: Token) -> Result<TwoWay> {
        // we don't process incoming MQTT messages
        Ok(TwoWay::default())
    }
}
