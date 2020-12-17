use super::{centi2float, AnnounceDevice, Result};
use crate::{Device, DeviceInfo, MqttMsg, Response, TwoWay};
use serde_json::json;

macro_rules! handlers {
    ( $( $n:expr => $topic:expr ),* ) => {
        fn register_1wire(&self) -> Vec<String> {
            let mut res = Vec::with_capacity(5);
            $( res.push(format!("{}_{}", self.info.busid, $n)); )*
            res
        }

        fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
            Ok(match resp {
                Response::Devstatus(s) => match
                    s.addr
                        .rsplit('_')
                        .nth(0)
                        .unwrap()
                        .parse()
                        .map_err(|e| super::Error::BusId(s.addr.to_owned(), e))? {
                    $( $n => TwoWay::from_mqtt(self.info.mqtt_msg($topic, centi2float(s.val))), )*
                    other => panic!("BUG: Unknown busaddr {}", other),
                },
                _ => {
                    warn!("[{}] {}: no handler for {:?}", self.info.contno, self.model(), resp);
                    TwoWay::default()
                }
            })
        }
    };
}

/// Makes announcement config for air sensors
fn mkann(
    this: &dyn Device,
    name: &str,
    short: &str,
    class: &str,
    uom: &str,
    dev: &AnnounceDevice,
) -> MqttMsg {
    let info = this.info();
    let name = format!("{} {}", this.name(), name);
    info!("Announcing entity {}", name);
    MqttMsg::retain(
        format!(
            "homeassistant/sensor/{}/{}_{}/config",
            info.contno, info.serno, short
        ),
        serde_json::to_string(&json!({
            "availability_topic": info.topic("status"),
            "device": &dev,
            "device_class": class,
            "expire_after": 600,
            "name": name,
            "qos": 1,
            "unique_id": format!("{}_{}", info.serno, short),
            "state_topic": info.topic(short),
            "unit_of_measurement": uom
        }))
        .unwrap(),
    )
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AirQuality {
    info: DeviceInfo,
}

impl AirQuality {
    new!(AirQuality);
}

impl Device for AirQuality {
    std_methods!(AirQuality);

    handlers!(
        1 => "temp",
        3 => "hum",
        4 => "dew",
        5 => "co2"
    );

    fn announce(&self) -> Vec<MqttMsg> {
        let dev = self.announce_device();
        vec![
            mkann(self, "Temperature", "temp", "temperature", "°C", &dev),
            mkann(self, "Humidity", "hum", "humidity", "%", &dev),
            mkann(self, "Dewpoint", "dew", "temperature", "°C", &dev),
            mkann(self, "CO2", "co2", "pressure", "ppm", &dev),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TempHum {
    info: DeviceInfo,
}

impl TempHum {
    new!(TempHum);
}

impl Device for TempHum {
    std_methods!(TempHum);

    handlers!(
        1 => "temp",
        3 => "hum",
        4 => "dew"
    );

    fn announce(&self) -> Vec<MqttMsg> {
        let dev = self.announce_device();
        vec![
            mkann(self, "Temperature", "temp", "temperature", "°C", &dev),
            mkann(self, "Humidity", "hum", "humidity", "%", &dev),
            mkann(self, "Dewpoint", "dew", "temperature", "°C", &dev),
        ]
    }
}
