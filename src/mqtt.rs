use crossbeam::channel::{self, Receiver, Sender};
use rumqttc::{ConnectReturnCode, Event, MqttOptions, Packet, QoS};
use std::collections::HashMap;
use std::fmt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to connect to MQTT broker at {0}: {1}")]
    Connect(String, #[source] rumqttc::ConnectionError),
    #[error("Lost connection to MQTT broker at {0}")]
    Disconnected(String),
    #[error("Failed to subscribe topic {0}: {1}")]
    Subscribe(String, #[source] rumqttc::ClientError),
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub type MqttMsg = (String, String);

pub struct MqttConnection {
    client: rumqttc::Client,
    out_tx: Sender<MqttMsg>,
    out_rx: Receiver<MqttMsg>,
    subscriptions: HashMap<String, Receiver<MqttMsg>>,
}

impl fmt::Debug for MqttConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let subs = self
            .subscriptions
            .keys()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>();
        write!(f, "MqttConnection({})", subs.join(", "))
    }
}

impl MqttConnection {
    pub fn new(host: &str, opt: MqttOptions) -> Result<(Self, rumqttc::Connection)> {
        let (client, mut conn) = rumqttc::Client::new(opt, 100);
        let (out_tx, out_rx) = channel::unbounded();
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
                Err(e) => return Err(Error::Connect(host.into(), e)),
            }
        }
        if success {
            Ok((
                Self {
                    client,
                    out_tx,
                    out_rx,
                    subscriptions: HashMap::new(),
                },
                conn,
            ))
        } else {
            Err(Error::Disconnected(host.into()))
        }
    }

    pub fn out_tx(&self) -> Sender<MqttMsg> {
        self.out_tx.clone()
    }

    pub fn subscribe<S: Into<String> + Clone>(&mut self, topic: S) -> Result<Sender<MqttMsg>> {
        let (tx, rx) = channel::unbounded();
        self.client
            .subscribe(topic.clone(), QoS::AtLeastOnce)
            .map_err(|e| Error::Subscribe(topic.clone().into(), e))?;
        self.subscriptions.insert(topic.into(), rx);
        Ok(tx)
    }
}
