use anyhow::{Context, Result};
use serde::Deserialize;
use slog::{debug, error, info, o, warn, Drain, Logger};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process;
use structopt::StructOpt;

use esera_mqtt::climate::{Climate, Conf, BASE};
use esera_mqtt::{MqttMsg, Routes, Token};

#[derive(StructOpt, Debug)]
struct Opt {
    /// MQTT broker address
    #[structopt(short = "H", long, default_value = "localhost", env = "MQTT_HOST")]
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
    fn new(c: Configs, log: &Logger) -> Self {
        Self {
            ctrl: c
                .0
                .into_iter()
                .map(|(n, t)| Climate::new(n, t, log))
                .collect(),
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
        log: &Logger,
    ) -> Box<dyn Iterator<Item = MqttMsg>> {
        match self.ctrl[idx].process(tok, topic, payload) {
            Ok(resp) => Box::new(resp.into_iter()),
            Err(e) => {
                error!(
                    log,
                    "Failed to process MQTT message ({} {}): {}", topic, payload, e
                );
                Box::new(std::iter::empty())
            }
        }
    }
}

fn run(opt: Opt, log: &Logger) -> Result<()> {
    let configs = Configs::read(&opt.config)
        .with_context(|| format!("Failed to read config file {}", opt.config))?;
    let (mut mqtt, recv) = esera_mqtt::MqttConnection::new(
        &opt.mqtt_host,
        &opt.mqtt_cred,
        &format!("{}/status", BASE),
        log.new(o!("mqtt" => opt.mqtt_host.clone())),
    )
    .context("Failed to connect to MQTT broker")?;
    let mut hvacs = HVACs::new(configs, log);
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
    debug!(log, "Entering main loop");
    for msg in recv {
        match msg {
            MqttMsg::Pub {
                ref topic,
                ref payload,
                ..
            } => {
                if let Some(xs) = routes.get(topic) {
                    for (idx, tok) in xs {
                        mqtt.sendall(hvacs.process(*idx, *tok, topic, payload, log))?;
                    }
                }
            }
            MqttMsg::Reconnected => {
                for msg in routes.subscriptions() {
                    mqtt.send(msg)?
                }
            }
            _ => warn!(log, "Unkown MQTT message type: {:?}", msg),
        }
    }
    Ok(())
}

fn main() {
    dotenv::dotenv().ok();
    let opt = Opt::from_args();
    #[cfg(not(debug_assertions))]
    let log = Logger::root(slog_journald::JournaldDrain.ignore_res(), o!());
    #[cfg(debug_assertions)]
    let log = {
        let d = slog_term::TermDecorator::new().build();
        let d = slog_term::FullFormat::new(d).build().fuse();
        let d = slog_async::Async::new(d).build().fuse();
        Logger::root(d, o!())
    };
    info!(log, "Connecting to MQTT broker"; "mqtt" => &opt.mqtt_host);
    if let Err(e) = run(opt, &log) {
        error!(log, "{}", e);
        process::exit(1)
    }
}
