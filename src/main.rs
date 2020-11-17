#[macro_use]
extern crate log;

use anyhow::Result;
use structopt::StructOpt;

use esera_mqtt::{Bus, Connection, Response};
use rumqttc::{AsyncClient, MqttOptions};
use std::time::Duration;

#[derive(StructOpt, Debug)]
struct Opt {
    /// Host name or IP address of the ESERA controller
    #[structopt(value_name = "HOST/IP")]
    controller_addr: String,
    /// Port number
    #[structopt(short, long, default_value = "5000")]
    port: u16,
    /// MQTT broker address
    #[structopt(short = "H", long, default_value = "localhost")]
    mqtt_host: String,
    /// MQTT credentials (username:password)
    #[structopt(short = "c", long, default_value = "", env = "MQTT_CRED")]
    mqtt_cred: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let opt = Opt::from_args();
    info!("Connecting to controller");
    let mut conn = Connection::new((&*opt.controller_addr, opt.port)).await?;
    info!("Connecting to MQTT broker");
    let mut mqtt_opt = MqttOptions::new("esera-mqtt", opt.mqtt_host, 1883);
    let mut parts = opt.mqtt_cred.splitn(2, ':');
    match (parts.next(), parts.next()) {
        (Some(user), Some(pw)) => mqtt_opt.set_credentials(user, pw),
        (Some(user), None) => mqtt_opt.set_credentials(user, ""),
        _ => &mut mqtt_opt,
    };
    let (mqtt, mut mqtt_loop) = AsyncClient::new(mqtt_opt, 100);
    let mut bus = Bus::init(&mut conn).await?;
    let init_msgs = bus.scan(&mut conn).await?;
    debug!("devices on bus: {:#?}", bus);
    tokio::pin!(conn);
    let timer = tokio::time::interval(Duration::from_secs(300));
    tokio::pin!(timer);
    loop {
        tokio::select! {
            item = conn.next() => match item {
                Ok(Response::Devstatus { ref addr, data }) => {
                    bus.handle_devstatus(addr, data, &mqtt).await?
                }
                Ok(Response::Keepalive) => (),
                _ => info!("=== {:?}", item)
            },
            evt = mqtt_loop.poll() => match evt {
                Ok(msg) => debug!("received MQTT message: {:?}", msg),
                Err(e) => error!("MQTT error: {}", e)
            },
            _ = timer.tick() => bus.publish(init_msgs.clone(), &mqtt).await?
        }
        tokio::time::delay_for(Duration::from_millis(1)).await
    }
}
