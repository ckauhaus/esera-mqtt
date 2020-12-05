#[macro_use]
extern crate log;

use anyhow::{Context, Result};
use crossbeam::channel::{self, Receiver, Sender};
use rumqttc::MqttOptions;
use std::fmt;
use std::net::ToSocketAddrs;
use std::thread;
use std::time::Duration;
use structopt::StructOpt;

use esera_mqtt::{ControllerConnection, ControllerError, MqttConnection, Response, Universe};

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
    #[structopt(short = "c", long, default_value = "", env = "MQTT_CRED")]
    mqtt_cred: String,
}

fn ctrl_loop<A>(addr: A) -> Result<(Sender<String>, Receiver<Result<Response, ControllerError>>)>
where
    A: ToSocketAddrs + Clone + fmt::Debug + Send + 'static,
{
    let (up_tx, up_rx) = channel::unbounded();
    let (down_tx, down_rx) = channel::unbounded();
    let mut c = ControllerConnection::new(addr.clone())?;
    down_tx.send(c.csi().map(|c| Response::CSI(c))).ok();
    // this is going to trigger registration which will be handled via ordinary event processing
    down_tx.send(c.list().map(|l| Response::List3(l))).ok();
    thread::spawn(move || loop {
        match c.event_loop(up_rx.clone(), down_tx.clone()) {
            Ok(_) => return,
            Err(e) => error!("[{}] Controller event loop died: {}", c.contno, e),
        }
        warn!("Reconnecting to {:?}", &addr);
        while let Err(e) = c.connect(addr.clone()) {
            error!("[{}] Reconnect failed: {}", c.contno, e);
            info!("Retrying in 5s...");
            thread::sleep(Duration::new(5, 0));
        }
    });
    Ok((up_tx, down_rx))
}

pub fn ctrl_create(
    addrs: &[String],
    default_port: u16,
) -> Result<Vec<(Sender<String>, Receiver<Result<Response, ControllerError>>)>> {
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

fn run(opt: Opt) -> Result<()> {
    let mut universe = Universe::default();
    let (ctrl_senders, ctrl_receivers): (Vec<_>, Vec<_>) =
        ctrl_create(&opt.controllers, opt.default_port)
            .context("Controller initialization failed")?
            .into_iter()
            .unzip();
    let mut sel = channel::Select::new();
    for down in &ctrl_receivers {
        sel.recv(&down);
    }

    let mut mqtt_opt = MqttOptions::new("esera-mqtt", opt.mqtt_host.clone(), 1883);
    let mut parts = opt.mqtt_cred.splitn(2, ':');
    match (parts.next(), parts.next()) {
        (Some(user), Some(pw)) => mqtt_opt.set_credentials(user, pw),
        (Some(user), None) => mqtt_opt.set_credentials(user, ""),
        _ => &mut mqtt_opt,
    };
    let (mut mqtt, mqtt_chan) = MqttConnection::new(&opt.mqtt_host, mqtt_opt)?;
    let mqtt_idx = sel.recv(&mqtt_chan);

    debug!("Entering main event loop");
    loop {
        let op = sel.select();
        match op.index() {
            i if i < ctrl_receivers.len() => {
                match op.recv(&ctrl_receivers[i]) {
                    Ok(Ok(resp)) => {
                        universe
                            .handle_1wire(resp)?
                            .send(&mut mqtt, &ctrl_senders[i])?;
                    }
                    Ok(Err(e)) => error!("{}", e),
                    Err(_) => break, // channel closed
                };
            }
            i if i == mqtt_idx => match op.recv(&mqtt_chan) {
                Ok(_msg) => todo!("handle incoming mqtt"),
                Err(_) => break, // channel closed
            },
            _ => panic!("BUG: unknown select() channel indexed"),
        }
    }
    Ok(())
}

fn main() {
    env_logger::init();
    let opt = Opt::from_args();
    info!("Connecting to MQTT broker at {}", opt.mqtt_host);
    std::process::exit(if let Err(e) = run(opt) {
        error!("FATAL: {}", e);
        1
    } else {
        0
    })
}
