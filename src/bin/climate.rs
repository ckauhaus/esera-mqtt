#[macro_use]
extern crate log;

use anyhow::{Context, Result};
use rumqttc::{LastWill, MqttOptions, QoS};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process;
use structopt::StructOpt;

use esera_mqtt::climate::{Climate, Conf, BASE};
use esera_mqtt::{MqttConnection, MqttMsg, Routes, Token};

#[derive(StructOpt, Debug)]
struct Opt {
    /// MQTT broker address
    #[structopt(short = "H", long, default_value = "localhost")]
    mqtt_host: String,
    /// MQTT credentials (username:password)
    #[structopt(short = "C", long, default_value = "", env = "MQTT_CRED")]
    mqtt_cred: String,
    #[structopt(value_name = "PATH")]
    config: String,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(transparent)]
struct Configs(HashMap<String, Conf>);

impl Configs {
    fn read<P: AsRef<Path>>(file: P) -> Result<Self> {
        Ok(toml::from_slice(&fs::read(file.as_ref())?)?)
    }
}

#[derive(Debug, Clone, Default)]
struct HVACs {
    ctrl: Vec<Climate>,
}

impl HVACs {
    fn new(c: Configs) -> Self {
        Self {
            ctrl: c.0.into_iter().map(|(n, t)| Climate::new(n, t)).collect(),
        }
    }

    fn subscribe_topics(&self) -> impl Iterator<Item = (usize, Token, String)> + '_ {
        self.ctrl
            .iter()
            .enumerate()
            .flat_map(|(i, c)| c.subscribe().map(move |(tok, topic)| (i, tok, topic)))
    }

    fn announce(&self) -> impl Iterator<Item = MqttMsg> + '_ {
        self.ctrl.iter().map(|c| c.announce())
    }

    fn eval(&self) -> impl Iterator<Item = MqttMsg> + '_ {
        self.ctrl.iter().flat_map(|c| c.eval())
    }

    fn process(
        &mut self,
        idx: usize,
        tok: Token,
        topic: &str,
        payload: &str,
    ) -> Box<dyn Iterator<Item = MqttMsg>> {
        match self.ctrl[idx].process(tok, topic, payload) {
            Ok(resp) => Box::new(resp.into_iter()),
            Err(e) => {
                error!(
                    "Failed to process MQTT message ({} {}): {}",
                    topic, payload, e
                );
                Box::new(std::iter::empty())
            }
        }
    }
}

fn run(opt: Opt) -> Result<()> {
    let configs = Configs::read(&opt.config)
        .with_context(|| format!("Failed to read config file {}", opt.config))?;
    let client_id = format!("esera_mqtt.{}", process::id());
    // XXX extract mqtt_setup?
    let mut mqtt_opt = MqttOptions::new(&client_id, opt.mqtt_host.clone(), 1883);
    let mut parts = opt.mqtt_cred.splitn(2, ':');
    match (parts.next(), parts.next()) {
        (Some(user), Some(pw)) => mqtt_opt.set_credentials(user, pw),
        (Some(user), None) => mqtt_opt.set_credentials(user, ""),
        _ => &mut mqtt_opt,
    };
    mqtt_opt.set_last_will(LastWill {
        topic: format!("{}/status", BASE),
        message: "offline".into(),
        qos: QoS::AtMostOnce,
        retain: true,
    });
    let (mut mqtt, recv) = MqttConnection::new(&opt.mqtt_host, mqtt_opt)?;
    mqtt.send(MqttMsg::retain(format!("{}/status", BASE), "online"))
        .context("Cannot set status to online")?;
    let mut hvacs = HVACs::new(configs);
    let mut routes = Routes::new();
    mqtt.sendall(
        hvacs
            .subscribe_topics()
            .flat_map(|(idx, token, topic)| routes.register(topic, idx, token)),
    )?;
    // publish autoconfig entries
    mqtt.sendall(hvacs.announce())?;
    // set initial state
    mqtt.sendall(hvacs.eval())?;
    info!("Entering main loop");
    for msg in recv {
        match msg {
            MqttMsg::Pub {
                ref topic,
                ref payload,
                ..
            } => {
                if let Some(xs) = routes.get(topic) {
                    for (idx, tok) in xs {
                        mqtt.sendall(hvacs.process(*idx, *tok, topic, payload))?;
                    }
                }
            }
            MqttMsg::Reconnected => {
                for msg in routes.subscriptions() {
                    mqtt.send(msg)?
                }
            }
            _ => warn!("Unkown MQTT message type: {:?}", msg),
        }
    }
    Ok(())
}

fn main() {
    env_logger::init();
    let opt = Opt::from_args();
    info!("Connecting to MQTT broker at {}", opt.mqtt_host);
    if let Err(e) = run(opt) {
        error!("FATAL: {}", e);
        process::exit(1)
    }
}
