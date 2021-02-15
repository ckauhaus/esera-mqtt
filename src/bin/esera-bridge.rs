#[macro_use]
extern crate log;

use anyhow::{Context, Result};
use crossbeam::channel::{self, Receiver, Sender};
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
    #[error("Controller channel closed")]
    ChanClosed,
    #[error("MQTT broker connection closed")]
    MqttClosed,
}

#[derive(StructOpt, Debug)]
struct Opt {
    /// Host name or IP address of a ESERA controller
    ///
    /// Can optionally contain a port number separated with ":". If no port number is given, the
    /// default port number applies.
    #[structopt(value_name = "HOST|IP[:PORT]")]
    controller: String,
    /// Port number
    #[structopt(short = "p", long, default_value = "5000")]
    default_port: u16,
    /// MQTT broker address
    #[structopt(short = "H", long, default_value = "localhost", env = "MQTT_HOST")]
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
    let mut c = ControllerConnection::new(addr)?;
    // this is going to trigger registration which will be handled via ordinary event processing
    down_tx.send(c.csi()).ok();
    down_tx.send(c.list()).ok();
    thread::spawn(move || {
        if let Err(e) = c.event_loop(up_rx, down_tx) {
            error!("[{}] Controller event loop died: {}", c.contno, e)
        }
    });
    Ok((up_tx, down_rx))
}

struct App {
    opt: Opt,
    ctrl_tx: Sender<String>,
    ctrl_rx: Receiver<Result<OW, ControllerError>>,
    bus: Bus,
    routes: Routes<usize>,
}

impl App {
    fn new(opt: Opt) -> Result<Self> {
        let (ctrl_tx, ctrl_rx) = if opt.controller.find(':').is_some() {
            ctrl_loop(opt.controller.clone())
        } else {
            ctrl_loop((opt.controller.clone(), opt.default_port))
        }
        .context("Failed to set up initial controller connection")?;
        Ok(Self {
            opt,
            ctrl_tx,
            ctrl_rx,
            bus: Bus::default(),
            routes: Routes::new(),
        })
    }

    fn handle(&mut self) -> Result<()> {
        // process first controller message separately to figure out controller number
        let resp = self.ctrl_rx.recv().map_err(|_| Error::ChanClosed)??;
        let (mut mqtt, mqtt_chan) = MqttConnection::new(
            &self.opt.mqtt_host,
            &self.opt.mqtt_cred,
            format!("ESERA/{}/status", resp.contno),
            None,
        )?;
        self.bus.handle_1wire(resp, &mut self.routes)?;
        let mut sel = channel::Select::new();
        let mqtt_idx = sel.recv(&mqtt_chan);
        let ctrl_idx = sel.recv(&self.ctrl_rx);
        loop {
            let op = sel.select();
            match op.index() {
                i if i == ctrl_idx => {
                    match op.recv(&self.ctrl_rx).map_err(|_| Error::ChanClosed)? {
                        Ok(resp) => self
                            .bus
                            .handle_1wire(resp, &mut self.routes)?
                            .send(&mut mqtt, &self.ctrl_tx)?,
                        Err(e) => error!("{}", e),
                    };
                }
                i if i == mqtt_idx => {
                    let msg = op.recv(&mqtt_chan).map_err(|_| Error::MqttClosed)?;
                    match msg {
                        MqttMsg::Pub { ref topic, .. } => {
                            for (dev, tok) in self.routes.lookup(topic) {
                                self.bus.devices[*dev]
                                    .handle_mqtt(&msg, *tok)?
                                    .send(&mut mqtt, &self.ctrl_tx)?
                            }
                        }
                        MqttMsg::Reconnected => {
                            info!("Renewing MQTT subscriptions");
                            for msg in self.routes.subscriptions() {
                                mqtt.send(msg)?;
                            }
                        }
                        _ => (), // ignore
                    }
                }
                _ => panic!("BUG: unknown select() channel indexed"),
            }
        }
    }

    fn run(&mut self) -> Result<()> {
        debug!("Entering main event loop");
        loop {
            match self.handle() {
                Ok(_) => continue,
                // Err(Error::ChanClosed(i)) => reconnect(i), // XXX
                // Err(Error::MqttClosed) => reregister(),    // XXX
                Err(e) => error!("{}", e),
            }
        }
    }
}

fn main() {
    dotenv::dotenv().ok();
    env_logger::builder().format_timestamp(None).init();
    if let Err(e) = App::new(Opt::from_args()).and_then(|mut app| app.run()) {
        error!("FATAL: {}", e);
        std::process::exit(1)
    }
}
