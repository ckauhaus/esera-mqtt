#[macro_use]
extern crate log;

use anyhow::{Context, Result};
use crossbeam::channel::{self, Receiver, Sender};
use rumqttc::{LastWill, MqttOptions, QoS};
use std::fmt;
use std::net::ToSocketAddrs;
use std::thread;
use structopt::StructOpt;
use thiserror::Error;

use esera_mqtt::{
    Bus, ControllerConnection, ControllerError, Device, MqttConnection, MqttMsg, Routes, OW,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Controller channel {0} closed")]
    ChanClosed(usize),
    #[error("MQTT broker connection closed")]
    MqttClosed,
}

#[derive(StructOpt, Debug)]
struct Opt {
    /// Host name or IP address of a ESERA controller
    ///
    /// Can optionally contain a port number separated with ":". If no port number is given, the
    /// default port number applices.
    #[structopt(value_name = "HOST|IP[:PORT]", required = true)]
    controllers: Vec<String>,
    /// Port number
    #[structopt(short = "p", long, default_value = "5000")]
    default_port: u16,
    /// MQTT broker address
    #[structopt(short = "H", long, default_value = "localhost")]
    mqtt_host: String,
    /// MQTT credentials (username:password)
    #[structopt(short = "C", long, default_value = "", env = "MQTT_CRED")]
    mqtt_cred: String,
}

type ChannelPair<O, I> = (Sender<O>, Receiver<I>);

fn ctrl_loop<A>(addr: A) -> Result<ChannelPair<String, Result<OW, ControllerError>>>
where
    A: ToSocketAddrs + Clone + fmt::Debug + Send + 'static,
{
    let (up_tx, up_rx) = channel::unbounded();
    let (down_tx, down_rx) = channel::unbounded();
    let mut c = ControllerConnection::new(addr.clone())?;
    // this is going to trigger registration which will be handled via ordinary event processing
    down_tx.send(c.csi()).ok();
    down_tx.send(c.list()).ok();
    thread::spawn(
        move || {
            if let Err(e) = c.event_loop(up_rx, down_tx) {
                error!("[{}] Controller event loop died: {}", c.contno, e)
            }
        }, // XXX return from this function and restart at outer level
           // while let Err(e) = c.reconnect(addr.clone()) {
           //     error!("[{}] Reconnect failed: {}", c.contno, e);
           //     info!("Retrying in 5s...");
           //     thread::sleep(Duration::new(5, 0));
           // }
           // down_tx.send(c.csi().map(OW::CSI)).ok();
           // down_tx.send(c.list().map(OW::List3)).ok();
    );
    Ok((up_tx, down_rx))
}

pub fn ctrl_create(
    addrs: &[String],
    default_port: u16,
) -> Result<Vec<ChannelPair<String, Result<OW, ControllerError>>>> {
    addrs
        .iter()
        .map(|c| {
            if c.find(':').is_some() {
                ctrl_loop(c.to_string())
            } else {
                ctrl_loop((c.to_string(), default_port))
            }
        })
        .collect()
}

// XXX move into mqtt.rs?
fn setup_mqtt(opt: &Opt) -> Result<(MqttConnection, Receiver<MqttMsg>)> {
    let client = format!("esera_mqtt.{}", std::process::id());
    let mut mqtt_opt = MqttOptions::new(&client, opt.mqtt_host.clone(), 1883);
    mqtt_opt.set_last_will(LastWill {
        topic: "ESERA/status".into(),
        message: "offline".into(),
        qos: QoS::AtMostOnce,
        retain: true,
    });
    let mut parts = opt.mqtt_cred.splitn(2, ':');
    match (parts.next(), parts.next()) {
        (Some(user), Some(pw)) => mqtt_opt.set_credentials(user, pw),
        (Some(user), None) => mqtt_opt.set_credentials(user, ""),
        _ => &mut mqtt_opt,
    };
    info!("Connecting to MQTT broker at {}", opt.mqtt_host);
    let (mut mqtt, mqtt_chan) = MqttConnection::new(&opt.mqtt_host, mqtt_opt)?;
    mqtt.send(MqttMsg::retain("ESERA/status", "online"))?;
    Ok((mqtt, mqtt_chan))
}

// XXX overlong parameter list
fn handle(
    senders: &[Sender<String>],
    receivers: &[Receiver<Result<OW, ControllerError>>],
    mqtt_chan: &Receiver<MqttMsg>,
    mqtt: &mut MqttConnection,
    bus: &mut [Bus],
    routes: &mut Routes<(u8, usize)>,
) -> Result<()> {
    let mut sel = channel::Select::new();
    for r in receivers {
        sel.recv(r);
    }
    let mqtt_idx = sel.recv(mqtt_chan);
    loop {
        let op = sel.select();
        match op.index() {
            i if i < receivers.len() => {
                match op.recv(&receivers[i]).map_err(|_| Error::ChanClosed(i))? {
                    Ok(resp) => {
                        bus[i].handle_1wire(resp, routes)?.send(mqtt, &senders[i])?;
                    }
                    Err(e) => error!("{}", e),
                };
            }
            i if i == mqtt_idx => {
                let msg = op.recv(&mqtt_chan).map_err(|_| Error::MqttClosed)?;
                if let MqttMsg::Pub { ref topic, .. } = msg {
                    for ((contno, dev), tok) in routes.lookup(topic) {
                        if let Some(i) = bus.iter().position(|b| b.contno == *contno) {
                            let res = bus[i].devices[*dev].handle_mqtt(&msg, *tok)?;
                            res.send(mqtt, &senders[i])?
                        } else {
                            warn!("No communication channel found for contno {}", contno);
                        }
                    }
                }
            }
            _ => panic!("BUG: unknown select() channel indexed"),
        }
    }
}

fn run(opt: Opt) -> Result<()> {
    let (ctrl_senders, ctrl_receivers): (Vec<_>, Vec<_>) =
        ctrl_create(&opt.controllers, opt.default_port)
            .context("Controller initialization failed")?
            .into_iter()
            .unzip();

    let (mut mqtt, mqtt_chan) = setup_mqtt(&opt)?;
    let mut bus = vec![Bus::default(); opt.controllers.len()];
    let mut routes = Routes::new();

    debug!("Entering main event loop");
    loop {
        match handle(
            &ctrl_senders,
            &ctrl_receivers,
            &mqtt_chan,
            &mut mqtt,
            &mut bus,
            &mut routes,
        ) {
            Ok(_) => continue,
            // Err(Error::ChanClosed(i)) => reconnect(i), // XXX
            // Err(Error::MqttClosed) => reregister(),    // XXX
            Err(e) => error!("{}", e),
        }
    }
}

fn main() {
    env_logger::init();
    let opt = Opt::from_args();
    if let Err(e) = run(opt) {
        error!("FATAL: {}", e);
        std::process::exit(1)
    }
}
