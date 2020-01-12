#[macro_use]
extern crate log;

use anyhow::Result;
use crossbeam::channel::bounded;
use crossbeam::thread;
use dotenv::dotenv;
use std::sync::RwLock;
use structopt::StructOpt;

mod controller;
mod device;
mod mqtt;
mod owb;

#[derive(StructOpt, Debug)]
struct Opt {
    #[structopt(short = "H", long, env = "MQTT_HOST", default_value = "localhost")]
    mqtt_host: String,
    #[structopt(short = "p", long, env = "MQTT_PORT", default_value = "1883")]
    mqtt_port: u16,
    #[structopt(short = "u", long, env = "MQTT_USER", default_value = "esera-mqtt")]
    mqtt_user: String,
    #[structopt(short = "P", long, env = "MQTT_PASS")]
    mqtt_pass: Option<String>,
    #[structopt(short = "e", long, env = "ESERA_HOST")]
    esera_host: String,
    #[structopt(short = "o", long, env = "ESERA_PORT", default_value = "5000")]
    esera_port: u16,
    #[structopt(short, long, env = "ESERA_CONTNO", default_value = "1")]
    contno: u8,
}

fn main() -> Result<()> {
    dotenv().ok();
    env_logger::init();
    let opt = Opt::from_args();
    debug!("{:?}", opt);
    let devinfo = RwLock::new(device::Devices::new());
    let mut ctrl = controller::Connection::new(&opt, &devinfo)?;
    let mut mqttc = mqtt::Client::new(&opt)?;
    let (tx, rx) = bounded(0xff);
    thread::scope(|s| -> Result<()> {
        let dispatcher = s.spawn(move |_| ctrl.dispatch(tx));
        let d2 = &devinfo;
        let publisher = s.spawn(move |_| mqttc.publish(d2, rx));
        publisher.join().unwrap()?;
        dispatcher.join().unwrap()?;
        Ok(())
    })
    .unwrap()?;
    Ok(())
}
