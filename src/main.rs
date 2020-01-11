#[macro_use]
extern crate log;

// use rumqtt::{MqttClient, MqttOptions, QoS};
// use std::{thread, time::Duration};
// use crossbeam_channel::select;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use dotenv::dotenv;
use std::net::IpAddr;
use structopt::StructOpt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{channel, Receiver, Sender};

mod owb;
use owb::{DevInfo, Msg, Resp, Resp::*};

struct ControllerConn {
    conn: BufReader<TcpStream>,
    contno: u8,
    devices: Vec<Option<DevInfo>>,
    buf: String,
}

impl ControllerConn {
    async fn new(host: &str, port: u16, contno: u8) -> Result<Self> {
        let addr: IpAddr = host.parse()?;
        let e = TcpStream::connect((addr, port))
            .await
            .context("Failed to connect to the controller")?;
        let mut s = Self {
            conn: BufReader::new(e),
            contno,
            devices: vec![None; 30],
            buf: String::with_capacity(80),
        };
        s.expect("SET,SYS,DATAPRINT,1", "DATAPRINT|1").await?;
        let now = Utc::now();
        let date = now.format("%d.%m.%y");
        let time = now.format("%H:%M:%S");
        s.expect(format!("SET,SYS,DATE,{}", date), format!("DATE|{}", date))
            .await?;
        s.expect(format!("SET,SYS,TIME,{}", time), format!("TIME|{}", time))
            .await?;
        s.expect("SET,SYS,DATATIME,10", "DATATIME|10").await?;
        s.get_device_info().await?;
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
        let exp = format!("_{}\r\n", expect.as_ref());
        let mut s = String::with_capacity(80);
        for _ in 0..50 {
            s.clear();
            self.conn.read_line(&mut s).await?;
            debug!("expect: got {:?}", s.trim());
            if s.contains(&exp) {
                return Ok(());
            }
        }
        Err(anyhow!("Did not receive expected output {:?}", exp))
    }

    async fn get_device_info(&mut self) -> Result<()> {
        debug!("request device list");
        self.write_line("GET,OWB,LISTALL1").await?;
        Ok(())
    }

    fn update_device_info(&mut self, devinfo: DevInfo) {
        info!("{:?}", devinfo);
        let n = devinfo.n as usize;
        if n >= self.devices.len() {
            self.devices.resize_with(n + 1, Option::default)
        }
        self.devices[n] = Some(devinfo);
    }

    async fn dispatch(&mut self, mut tx: Sender<Msg>) -> Result<()> {
        loop {
            self.conn.read_line(&mut self.buf).await?;
            debug!("dispatch({:?})", self.buf);
            match Resp::parse(self.contno, &mut self.buf) {
                Ok((rest, resp)) => {
                    self.buf = rest;
                    match resp {
                        Dev(msg) => {
                            let n = msg.dev as usize;
                            if n >= self.devices.len() || self.devices[n].is_none() {
                                self.get_device_info().await?;
                            }
                            tx.send(msg).await?;
                        }
                        Info(devinfo) => self.update_device_info(devinfo),
                        Other(dev, msg) => debug!("Other({}, {})", dev.as_str(), msg),
                        ERR(e) => warn!("1-Wire error: {}", e.as_str()),
                        _ => (),
                    }
                }
                Err(e) => {
                    error!("{}", e);
                    self.buf.clear()
                }
            }
        }
    }
}

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

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    env_logger::init();
    let opt = Opt::from_args();
    debug!("{:?}", opt);
    let mut ctrl = ControllerConn::new(&opt.esera_host, opt.esera_port, opt.contno).await?;
    let (tx, rx) = channel(4);
    ctrl.dispatch(tx).await
}
