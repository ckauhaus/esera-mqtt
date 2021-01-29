use super::{centi2float, AnnounceDevice, Result};
use crate::parser::{Msg, OW};
use crate::{Device, DeviceInfo, MqttMsg, TwoWay};
use serde_json::json;

macro_rules! handlers {
    ( $( $n:expr => $topic:expr ),* ) => {
        fn register_1wire(&self) -> Vec<String> {
            let mut res = Vec::with_capacity(5);
            $( res.push(format!("{}_{}", self.info.busid, $n)); )*
            res
        }

        fn handle_1wire(&mut self, resp: OW) -> Result<TwoWay> {
            Ok(match resp.msg {
                Msg::Devstatus(s) => match
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::test::cmp_ow;

    #[test]
    fn airquality_devstatus() {
        let mut uut = AirQuality::new(DeviceInfo::new(1, "OWD3", "", "online", "", None).unwrap());
        cmp_ow(&mut uut, "1_OWD3_1|1976\n", "ESERA/1/OWD3/temp", "19.76");
        cmp_ow(&mut uut, "1_OWD3_3|5456\n", "ESERA/1/OWD3/hum", "54.56");
        cmp_ow(&mut uut, "1_OWD3_4|0\n", "ESERA/1/OWD3/dew", "0");
        cmp_ow(&mut uut, "1_OWD3_5|186518\n", "ESERA/1/OWD3/co2", "1865.18");
    }

    #[test]
    fn temphum_devstatus() {
        let mut uut = TempHum::new(DeviceInfo::new(1, "OWD2", "", "online", "", None).unwrap());
        cmp_ow(&mut uut, "1_OWD2_1|2087\n", "ESERA/1/OWD2/temp", "20.87");
        cmp_ow(&mut uut, "1_OWD2_1|-97\n", "ESERA/1/OWD2/temp", "-0.97");
        cmp_ow(&mut uut, "1_OWD2_3|5980\n", "ESERA/1/OWD2/hum", "59.8");
        cmp_ow(&mut uut, "1_OWD2_4|332\n", "ESERA/1/OWD2/dew", "3.32");
    }
}
