#[macro_use]
extern crate log;

use anyhow::Result;
use structopt::StructOpt;

use esera_mqtt::Response;

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

#[allow(unused)]
fn main() -> Result<()> {
    env_logger::init();
    let opt = Opt::from_args();
    info!("Connecting to controller(s)");
    // let mut conn = ControllerConnection::new(opt.controllers[0], opt.default_port)?;
    info!("Connecting to MQTT broker");
    // let mut mqtt_opt = MqttOptions::new("esera-mqtt", opt.mqtt_host, 1883);
    // let mut parts = opt.mqtt_cred.splitn(2, ':');
    // match (parts.next(), parts.next()) {
    //     (Some(user), Some(pw)) => mqtt_opt.set_credentials(user, pw),
    //     (Some(user), None) => mqtt_opt.set_credentials(user, ""),
    //     _ => &mut mqtt_opt,
    // };
    // let mut bus = Bus::init(&mut conn)?;
    // bus.scan(&mut conn)?;
    // debug!("devices on bus: {:#?}", bus);
    Ok(())
}
