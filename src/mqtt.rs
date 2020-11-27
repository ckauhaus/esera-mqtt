use crossbeam::channel::{self, Receiver, Sender};
use parking_lot::RwLock;
use rumqttc::{ConnectReturnCode, Event, MqttOptions, Packet, Publish, QoS};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to connect to MQTT broker at {0}: {1}")]
    Connect(String, #[source] rumqttc::ConnectionError),
    #[error("Lost connection to MQTT broker at {0}")]
    Disconnected(String),
    #[error("Failed to subscribe topic {0}: {1}")]
    Subscribe(String, #[source] rumqttc::ClientError),
    #[error("Failed to publish MQTT message: {0}")]
    Send(#[from] rumqttc::ClientError),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MqttMsg {
    topic: String,
    payload: String,
}

impl MqttMsg {
    pub fn new<S: Into<String>, P: ToString>(topic: S, payload: P) -> Self {
        Self {
            topic: topic.into(),
            payload: payload.to_string(),
        }
    }
}

type Subscriptions = Arc<RwLock<HashMap<String, Sender<Publish>>>>;

pub struct MqttConnection {
    client: rumqttc::Client,
    subscriptions: Subscriptions,
}

fn process_packet(pck: Packet, subs: &Subscriptions) -> bool {
    match pck {
        Packet::Publish(msg) => {
            debug!("=== {:?}", msg);
            let subs = subs.read();
            let topic = msg.topic.clone();
            for (t, ch) in subs.iter() {
                if rumqttc::matches(&topic, t) {
                    if let Err(_) = ch.send(msg) {
                        warn!("MQTT: subscription channel for {} has been closed", topic)
                    }
                    break;
                }
            }
        }
        Packet::Disconnect => return false,
        _ => (),
    }
    true
}

fn start_mqtt_loop(host: String, mut conn: rumqttc::Connection, subs: Subscriptions) {
    std::thread::Builder::new()
        .name("MQTT reader".into())
        .spawn(move || {
            for evt in conn.iter() {
                match evt {
                    Ok(Event::Incoming(pck)) => {
                        if !process_packet(pck, &subs) {
                            return;
                        }
                    }
                    Ok(Event::Outgoing(_)) => (),
                    Err(e) => warn!("MQTT: {}", e),
                }
            }
            // underlying transport closed
            error!("MQTT connection to {} unexpectly lost", host);
        })
        .unwrap();
}

impl MqttConnection {
    pub fn new(host: &str, opt: MqttOptions) -> Result<Self> {
        let host = host.to_owned();
        let (client, mut conn) = rumqttc::Client::new(opt, 100);
        let mut success = false;
        for item in conn.iter().take(3) {
            match item {
                Ok(Event::Incoming(Packet::ConnAck(rumqttc::ConnAck {
                    code: ConnectReturnCode::Accepted,
                    ..
                }))) => {
                    success = true;
                    break;
                }
                Ok(other) => warn!(
                    "Unexpected response while connecting to MQTT broker: {:?}",
                    other
                ),
                Err(e) => return Err(Error::Connect(host, e)),
            }
        }
        if success {
            let subscriptions = Arc::new(RwLock::new(HashMap::new()));
            start_mqtt_loop(host, conn, subscriptions.clone());
            Ok(Self {
                client,
                subscriptions,
            })
        } else {
            Err(Error::Disconnected(host))
        }
    }

    pub fn subscribe<S: Into<String> + Clone>(&mut self, topic: S) -> Result<Receiver<Publish>> {
        let (tx, rx) = channel::unbounded();
        self.client
            .subscribe(topic.clone(), QoS::AtLeastOnce)
            .map_err(|e| Error::Subscribe(topic.clone().into(), e))?;
        self.subscriptions.write().insert(topic.into(), tx);
        Ok(rx)
    }

    pub fn send(&mut self, msg: MqttMsg) -> Result<()> {
        debug!("+++ {}: {}", msg.topic, msg.payload);
        Ok(self
            .client
            .publish(msg.topic, QoS::AtLeastOnce, false, msg.payload.as_bytes())?)
    }

    pub fn disconnect(&mut self) {
        self.client.disconnect().ok();
    }
}

impl fmt::Debug for MqttConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let subscriptions = self.subscriptions.read();
        let items = subscriptions.keys().map(|s| s.as_ref()).collect::<Vec<_>>();
        write!(f, "MqttConnection({})", items.join(", "))
    }
}
