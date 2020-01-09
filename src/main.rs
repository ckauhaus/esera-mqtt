#[macro_use]
extern crate log;

// use rumqtt::{MqttClient, MqttOptions, QoS};
// use std::{thread, time::Duration};
// use crossbeam_channel::select;
use anyhow::{Result, Context, anyhow};
use structopt::StructOpt;
use std::net::IpAddr;
use dotenv::dotenv;
use tokio::io::{BufReader, AsyncBufReadExt, AsyncWriteExt};
use std::str::FromStr;
use tokio::net::TcpStream;
use chrono::{Utc};

pub struct ControllerConn {
    pub conn: BufReader<TcpStream>,
    pub contno: u8,
}

impl ControllerConn {
    async fn new(host: &str, port: u16, contno: u8) -> Result<Self> {
        let addr: IpAddr = host.parse()?;
        let e = TcpStream::connect((addr, port)).await.context("Failed to connect to the controller")?;
        let mut s = Self { conn: BufReader::new(e), contno };
        s.expect("SET,SYS,DATAPRINT,1", "DATAPRINT|1").await?;
        s.expect(format!("SET,SYS,CONTNO,{}", contno), format!("CONTNO|{}", contno)).await?;
        let now = Utc::now();
        let date = now.format("%d.%m.%y");
        let time = now.format("%H:%M:%S");
        s.expect(format!("SET,SYS,DATE,{}", date), format!("DATE|{}", date)).await?;
        s.expect(format!("SET,SYS,TIME,{}", time), format!("TIME|{}", time)).await?;
        Ok(s)
    }

    async fn write_line(&mut self, line: &str) -> Result<()> {
        self.conn.write_all(line.as_bytes()).await?;
        self.conn.write_all(b"\r\n").await?;
        Ok(())
    }

    async fn expect<S: AsRef<str>, T: AsRef<str>>(&mut self, send: S, expect: T) -> Result<()> {
        debug!("expect({:?}, {:?})", send.as_ref(), expect.as_ref());
        self.write_line(send.as_ref()).await?;
        let mut s = String::with_capacity(80);
        let exp = format!("_{}\r\n", expect.as_ref());
        for _ in 0..10 {
            s.clear();
            self.conn.read_line(&mut s).await?;
            debug!("expect: got {:?}", s.trim());
            if s.contains(&exp) {
                return Ok(())
            }
        }
        Err(anyhow!("Did not receive expected output {:?}", exp))
    }
}

#[tokio::main]
async fn run(opt: Opt) -> Result<()> {
    let mut ctrl = ControllerConn::new(&opt.esera_host, opt.esera_port, opt.contno).await?;
    let mut s = String::new();
    loop {
        ctrl.dispatch().await?;
    }
}

#[derive(StructOpt, Debug)]
struct Opt {
    #[structopt(short = "H", long, env = "MQTT_HOST", default_value="localhost")]
    mqtt_host: String,
    #[structopt(short = "p", long, env = "MQTT_PORT", default_value="1883")]
    mqtt_port: u16,
    #[structopt(short = "u", long, env = "MQTT_USER", default_value="esera-mqtt")]
    mqtt_user: String,
    #[structopt(short = "P", long, env = "MQTT_PASS")]
    mqtt_pass: Option<String>,
    #[structopt(short = "e", long, env = "ESERA_HOST")]
    esera_host: String,
    #[structopt(short = "o", long, env = "ESERA_PORT", default_value="5000")]
    esera_port: u16,
    #[structopt(short, long, env = "ESERA_CONTNO", default_value="1")]
    contno: u8
}

fn main() -> Result<()> {
    dotenv().ok();
    env_logger::init();
    let opt = Opt::from_args();
    debug!("{:?}", opt);
    run(opt)
}
