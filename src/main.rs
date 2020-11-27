#[macro_use]
extern crate log;

use anyhow::{Context, Result};
use crossbeam::channel;
use rumqttc::MqttOptions;
use structopt::StructOpt;

use esera_mqtt::{controller, MqttConnection, Universe};

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

fn run(mqtt: &mut MqttConnection, opt: Opt) -> Result<()> {
    let mut universe = Universe::default();
    let ctrl_chan = controller::create(&opt.controllers, opt.default_port)
        .context("Controller initialization failed")?;
    let mut sel = channel::Select::new();
    for (_, down) in &ctrl_chan {
        sel.recv(down);
    }
    debug!("Entering main event loop");
    loop {
        let oper = sel.select();
        let i = oper.index();
        match oper.recv(&ctrl_chan[i].1) {
            Ok(Ok(resp)) => {
                let res = universe.handle_1wire(resp)?;
                for msg in res.mqtt {
                    mqtt.send(msg)?;
                }
                for cmd in res.ow {
                    &ctrl_chan[i].0.send(cmd)?;
                }
            }
            Ok(Err(e)) => error!("{}", e),
            Err(_) => break, // channel closed
        };
    }
    Ok(())
}

fn main() {
    env_logger::init();
    let opt = Opt::from_args();
    info!("Connecting to MQTT broker at {}", opt.mqtt_host);
    let mut mqtt_opt = MqttOptions::new("esera-mqtt", opt.mqtt_host.clone(), 1883);
    let mut parts = opt.mqtt_cred.splitn(2, ':');
    match (parts.next(), parts.next()) {
        (Some(user), Some(pw)) => mqtt_opt.set_credentials(user, pw),
        (Some(user), None) => mqtt_opt.set_credentials(user, ""),
        _ => &mut mqtt_opt,
    };
    let mut mqtt = match MqttConnection::new(&opt.mqtt_host, mqtt_opt) {
        Ok(mqtt) => mqtt,
        Err(e) => {
            error!("Failed to connect to MQTT broker: {}", e);
            std::process::exit(2);
        }
    };
    let exitcode = if let Err(e) = run(&mut mqtt, opt) {
        error!("FATAL: {}", e);
        1
    } else {
        0
    };
    mqtt.disconnect();
    std::process::exit(exitcode);
}
