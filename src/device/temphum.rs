use super::{centi2float, Result, Token};
use crate::{Device, DeviceInfo, MqttMsg, Response, TwoWay};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TempHum {
    info: DeviceInfo,
}

impl TempHum {
    new!(TempHum);
}

impl Device for TempHum {
    std_methods!(TempHum);

    fn init(&self) -> TwoWay {
        let dev = json!({
            "identifiers": [self.info.serno],
            "model": self.model(),
            "name": self.name()
        });
        TwoWay::new(
            vec![
                MqttMsg::new(
                    format!(
                        "homeassistant/sensor/{}/{}_temp/config",
                        self.info.contno, self.info.serno
                    ),
                    serde_json::to_string(&json!({
                        "~": self.info.fmt(format_args!("")),
                        "device": &dev,
                        "name": format!("{}_temp", self.name()),
                        "unique_id": format!("{}_temp", self.info.serno),
                        "availability_topic": "~/status",
                        "status_topic": "~/temp",
                        "device_class": "temperature",
                        "unit_of_measurement": "°C"
                    }))
                    .unwrap(),
                ),
                MqttMsg::new(
                    format!(
                        "homeassistant/sensor/{}/{}_hum/config",
                        self.info.contno, self.info.serno
                    ),
                    serde_json::to_string(&json!({
                        "~": self.info.fmt(format_args!("")),
                        "device": &dev,
                        "name": format!("{}_hum", self.info.name.as_ref().unwrap_or(&self.info.busid)),
                        "unique_id": format!("{}_hum", self.info.serno),
                        "availability_topic": "~/status",
                        "status_topic": "~/hum",
                        "device_class": "humidity",
                        "unit_of_measurement": "%"
                    }))
                    .unwrap(),
                ),
                MqttMsg::new(self.info.topic("status"), self.info.status),
            ],
            vec![],
        )
    }

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
            Response::OWDStatus(os) => self.handle_status(os.status),
            _ => {
                warn!("[{}] TempHum: no handler for {:?}", self.info.contno, resp);
                TwoWay::default()
            }
        })
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        vec![
            (self.info.topic("temp"), -1),
            (self.info.topic("hum"), -1),
            (self.info.topic("dew"), -1),
        ]
    }

    fn handle_mqtt(&self, _msg: MqttMsg, _token: Token) -> Result<TwoWay> {
        // we don't process incoming MQTT messages
        Ok(TwoWay::default())
    }
}
