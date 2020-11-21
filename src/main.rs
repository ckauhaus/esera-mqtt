#[macro_use]
extern crate log;

use anyhow::Result;
use crossbeam::channel::{self, Receiver, Sender};
use rumqttc::MqttOptions;
use structopt::StructOpt;

use esera_mqtt::{controller, MqttConnection, Response};

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

fn main() -> Result<()> {
    env_logger::init();
    let opt = Opt::from_args();
    info!("Connecting to MQTT broker");
    let mut mqtt_opt = MqttOptions::new("esera-mqtt", opt.mqtt_host.clone(), 1883);
    let mut parts = opt.mqtt_cred.splitn(2, ':');
    match (parts.next(), parts.next()) {
        (Some(user), Some(pw)) => mqtt_opt.set_credentials(user, pw),
        (Some(user), None) => mqtt_opt.set_credentials(user, ""),
        _ => &mut mqtt_opt,
    };
    let (mqtt, mqtt_conn) = MqttConnection::new(&opt.mqtt_host, mqtt_opt)?;
    let ctrl_chan = controller::create(&opt.controllers, opt.default_port)?;
    let mut sel = channel::Select::new();
    for (_, down) in &ctrl_chan {
        sel.recv(down);
    }
    loop {
        let oper = sel.select();
        let i = oper.index();
        match oper.recv(&ctrl_chan[i].1) {
            Ok(Ok(resp)) => debug!("{:?}", resp),
            Ok(Err(e)) => error!("{}", e),
            Err(_) => break, // channel closed
        };
    }
    Ok(())
}
