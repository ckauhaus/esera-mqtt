use super::{centi2float, Result, Token};
use crate::{Device, DeviceInfo, MqttMsg, Response, TwoWay};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AirQuality {
    info: DeviceInfo,
}

impl AirQuality {
    new!(AirQuality);
}

impl Device for AirQuality {
    std_methods!(AirQuality);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 3, 4, 5])
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::Devstatus(s) => match s.addr.rsplit('_').nth(0).unwrap() {
                "1" => TwoWay::mqtt(self.info.topic("temp"), centi2float(s.val)),
                "3" => TwoWay::mqtt(self.info.topic("hum"), centi2float(s.val)),
                "4" => TwoWay::mqtt(self.info.topic("dew"), centi2float(s.val)),
                "5" => TwoWay::mqtt(self.info.topic("co2"), centi2float(s.val)),
                other => panic!("BUG: Unknown busaddr {}", other),
            },
            Response::OWDStatus(os) => self.handle_status(os.status),
            _ => {
                warn!(
                    "[{}] AirQuality: no handler for {:?}",
                    self.info.contno, resp
                );
                TwoWay::default()
            }
        })
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        vec![
            (self.info.topic("temp"), -1),
            (self.info.topic("hum"), -1),
            (self.info.topic("dew"), -1),
            (self.info.topic("co2"), -1),
        ]
    }

    fn handle_mqtt(&self, _msg: MqttMsg, _token: Token) -> Result<TwoWay> {
        // we don't process incoming MQTT messages
        Ok(TwoWay::default())
    }
}
