use crossbeam::channel::{self, Receiver, Sender};
use rumqttc::{ConnectReturnCode, Event, MqttOptions, Packet, QoS};
use std::fmt;
use std::thread;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to connect to MQTT broker at {0}: {1}")]
    Connect(String, #[source] rumqttc::ConnectionError),
    #[error("Lost connection to MQTT broker")]
    Disconnected,
    #[error("Failed to subscribe topic {0}: {1}")]
    Subscribe(String, #[source] rumqttc::ClientError),
    #[error("Failed to publish MQTT message: {0}")]
    Send(#[from] rumqttc::ClientError),
    #[error("Failed to decode UTF-8 message payload: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error(transparent)]
    Channel(#[from] channel::SendError<MqttMsg>),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq)]
pub enum MqttMsg {
    Pub {
        topic: String,
        payload: String,
        retain: bool,
    },
    Sub {
        topic: String,
    },
    Reconnected, // XXX TODO handle downstream
}

impl MqttMsg {
    pub fn new<S: Into<String>, P: ToString>(topic: S, payload: P) -> Self {
        Self::Pub {
            topic: topic.into(),
            payload: payload.to_string(),
            retain: false,
        }
    }

    pub fn retain<S: Into<String>, P: ToString>(topic: S, payload: P) -> Self {
        Self::Pub {
            topic: topic.into(),
            payload: payload.to_string(),
            retain: true,
        }
    }

    pub fn sub<S: Into<String>>(topic: S) -> Self {
        Self::Sub {
            topic: topic.into(),
        }
    }

    /// Returns topic of a message. Panics if this message does not contain a topic.
    pub fn topic(&self) -> &str {
        match self {
            Self::Pub { ref topic, .. } => topic,
            Self::Sub { ref topic } => topic,
            _ => panic!(
                "Attempted to call MqttMsg::topic of a message without payload ({:?})",
                self
            ),
        }
    }

    /// Returns payload of a publish message. Panics if this is not a publish message.
    pub fn payload(&self) -> &str {
        match self {
            Self::Pub { ref payload, .. } => payload,
            _ => panic!(
                "Attempted to call MqttMsg::payload of a non-publish message ({:?})",
                self
            ),
        }
    }

    /// Returns true if this is a publish message which fits topic pattern as per MQTT match
    /// syntax.
    pub fn matches(&self, topic_pattern: &str) -> bool {
        if let Self::Pub { topic, .. } = self {
            rumqttc::matches(topic, topic_pattern)
        } else {
            false
        }
    }
}

impl fmt::Display for MqttMsg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pub {
                topic,
                payload,
                retain,
            } => write!(
                f,
                "{} {}{}",
                topic,
                payload,
                if *retain { " (retain)" } else { "" }
            ),
            Self::Sub { topic } => write!(f, "Subscribe {}", topic),
            Self::Reconnected => write!(f, "Reconnected to broker"),
        }
    }
}

fn process_packet(pck: Packet, tx: &Sender<MqttMsg>) -> Result<()> {
    match pck {
        Packet::Publish(p) => {
            let msg = MqttMsg::new(p.topic, String::from_utf8(p.payload.to_vec())?);
            debug!("==< {:?}", msg);
            tx.send(msg).map_err(Error::from)
        }
        Packet::Disconnect => Err(Error::Disconnected),
        Packet::ConnAck(rumqttc::ConnAck {
            code: ConnectReturnCode::Accepted,
            ..
        }) => {
            info!("Reconnected to MQTT broker");
            tx.send(MqttMsg::Reconnected).map_err(Error::from)
        }
        _ => Ok(()),
    }
}

pub struct MqttConnection {
    host: String,
    client: rumqttc::Client,
}

impl MqttConnection {
    pub fn new(host: &str, opt: MqttOptions) -> Result<(Self, Receiver<MqttMsg>)> {
        let host = host.to_owned();
        let (client, mut conn) = rumqttc::Client::new(opt, 10);
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
            let (tx, rx) = channel::unbounded();
            let this = Self { host, client };
            this.recv_loop(conn, tx);
            Ok((this, rx))
        } else {
            Err(Error::Disconnected)
        }
    }

    fn recv_loop(&self, mut conn: rumqttc::Connection, tx: Sender<MqttMsg>) {
        let host = self.host.clone();
        std::thread::Builder::new()
            .name("MQTT reader".into())
            .spawn(move || {
                let mut retry = 200;
                for evt in conn.iter() {
                    match evt {
                        Ok(Event::Incoming(pck)) => match process_packet(pck, &tx) {
                            Err(Error::Send(_)) => {
                                info!("Disconnecting from MQTT broker {}", host);
                                return;
                            }
                            Err(e) => warn!("Failed to process incoming packet: {}", e),
                            Ok(_) => (),
                        },
                        Ok(Event::Outgoing(_)) => (),
                        Err(e) => {
                            error!("{}, reconnecting in {} ms", e, retry);
                            thread::sleep(Duration::from_millis(retry));
                            if retry < 20_000 {
                                retry = retry * 6 / 5;
                            }
                        }
                    }
                }
            })
            .unwrap();
    }

    pub fn send(&mut self, msg: MqttMsg) -> Result<()> {
        debug!("==> {:?}", msg);
        match msg {
            MqttMsg::Pub {
                topic,
                payload,
                retain,
            } => self
                .client
                .publish(topic, QoS::AtMostOnce, retain, payload.as_bytes())?,
            MqttMsg::Sub { topic } => self.client.subscribe(topic, QoS::AtMostOnce)?,
            MqttMsg::Reconnected => (), // XXX bail out instead?
        }
        Ok(())
    }

    pub fn sendall<I: Iterator<Item = MqttMsg>>(&mut self, mut msgs: I) -> Result<()> {
        msgs.try_for_each(|msg| self.send(msg))
    }

    pub fn subscribe(&mut self, topic: &str) -> Result<()> {
        self.client
            .subscribe(topic, QoS::AtMostOnce)
            .map_err(|e| Error::Subscribe(topic.into(), e))
    }
}

impl fmt::Debug for MqttConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "MqttConnection({})", self.host)
    }
}
