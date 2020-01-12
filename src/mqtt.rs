use anyhow::{anyhow, Result};
use crossbeam::channel::Receiver as XBReceiver;
use rumqtt::{MqttClient, MqttOptions, Notification, QoS, Receiver, SecurityOptions};
use std::sync::RwLock;
use uuid::Uuid;

use crate::device::Devices;
use crate::owb::Evt;
use crate::Opt;

pub struct Client {
    mqtt: MqttClient,
    incoming: Receiver<Notification>,
    contno: u8,
}

impl Client {
    pub(crate) fn new(opt: &Opt) -> Result<Self> {
        let mqtt_options = MqttOptions::new(
            &format!("esera-{}", Uuid::new_v4()),
            &opt.mqtt_host,
            opt.mqtt_port,
        )
        .set_security_opts(SecurityOptions::UsernamePassword(
            opt.mqtt_user.clone(),
            opt.mqtt_pass.clone().unwrap_or_default(),
        ));
        let (mqtt, incoming) = MqttClient::start(mqtt_options)
            .map_err(|e| anyhow!("Failed to connect to MQTT broker: {:?}", e))?;
        Ok(Client {
            mqtt,
            incoming,
            contno: opt.contno,
        })
    }

    pub fn publish(&mut self, dev: &RwLock<Devices>, msgs: XBReceiver<Evt>) -> Result<()> {
        for evt in msgs {
            for (topic, payload) in expand(&evt, dev) {
                if let Err(e) = self.mqtt.publish(
                    format!("ESERA/{}/EVT/{}", self.contno, topic),
                    QoS::AtMostOnce,
                    false,
                    payload,
                ) {
                    error!("MQTT error: {}", e);
                    return Err(anyhow!("MQTT failed"));
                }
            }
        }
        self.mqtt.shutdown().ok();
        Ok(())
    }
}

fn expand<'a>(evt: &'a Evt, dev: &RwLock<Devices>) -> impl Iterator<Item = (String, &'a [u8])> {
    let dev = dev.read().unwrap();
    let info = dev.by_busid(&evt.busid);
    vec![(
        format!("{}/{}", info.friendly_name(), evt.sub),
        evt.msg.as_bytes(),
    )]
    .into_iter()
}
