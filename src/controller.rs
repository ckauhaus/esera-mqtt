use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use crossbeam::channel::{unbounded, Sender};
use std::io::{BufRead, BufReader, Write};
use std::mem::{discriminant, Discriminant};
use std::net::IpAddr;
use std::net::TcpStream;
use std::sync::RwLock;

use crate::device::{DevInfo, Devices};
use crate::owb::{Evt, Resp, Resp::*};
use crate::Opt;

pub struct Connection<'l> {
    conn: BufReader<TcpStream>,
    contno: u8,
    dev: &'l RwLock<Devices>,
}

impl<'l> Connection<'l> {
    pub(crate) fn new(opt: &Opt, dev: &'l RwLock<Devices>) -> Result<Self> {
        let addr: IpAddr = opt.esera_host.parse()?;
        let e = TcpStream::connect((addr, opt.esera_port))
            .context("Failed to connect to the controller")?;
        let mut s = Self {
            conn: BufReader::new(e),
            contno: opt.contno,
            dev,
        };
        // XXX untested
        s.expect("SET,SYS,DATAPRINT,1", 10, |r| match r {
            Dataprint(_) => Ok(None),
            other => Ok(Some(other)),
        })?;
        // s.expect("SET,SYS,DATAPRINT,1", "DATAPRINT|1")?;
        // // XXX
        // if let Err(e) = s.expect("GET,SYS,CONTNO", format!("CONTNO|{}", opt.contno)) {
        //     return Err(anyhow!("Wrong controller number {}\n{}", opt.contno, e));
        // }
        let now = Utc::now();
        let date = now.format("%d.%m.%y");
        let time = now.format("%H:%M:%S");
        // s.expect(format!("SET,SYS,DATE,{}", date), format!("DATE|{}", date))?;
        // s.expect(format!("SET,SYS,TIME,{}", time), format!("TIME|{}", time))?;
        // s.expect("SET,SYS,DATATIME,10", "DATATIME|10")?;
        Ok(s)
    }

    /// Write a single line and reads lines until `action` does not consume any more.
    /// `action` should return Ok(None) if it did something useful with the passed response or
    /// Ok(Some(resp)) if that response should be passed over to a reduced standard processing.
    /// Returning Err(_) causes `expect` to terminate right away.
    fn expect<F>(&mut self, write: &str, maxtries: usize, action: F) -> Result<()>
    where
        F: Fn(Resp) -> Result<Option<Resp>>,
    {
        debug!("expect({:?})", write);
        self.write_line(write)?;
        let (tx_blackhole, _) = unbounded();
        let mut tries = 0;
        let mut some_action_performed = false;
        loop {
            match action(self.next()?)? {
                None => some_action_performed = true,
                Some(unconsumed) => {
                    self.process(unconsumed, &tx_blackhole)?;
                    if some_action_performed {
                        return Ok(());
                    }
                    tries += 1;
                }
            }
            if tries > maxtries {
                return Err(anyhow!("expect({:?}): too many tries, aborting", write));
            }
        }
    }

    fn write_line(&mut self, line: &str) -> Result<()> {
        let tcp = self.conn.get_mut();
        debug!("write: {}", line);
        Ok(tcp.write_all(format!("{}\r\n", line).as_bytes())?)
    }

    fn get_device_info(&mut self) -> Result<()> {
        debug!("requesting device list");
        Ok(self.write_line("GET,OWB,LISTALL1")?)
    }

    fn update_device_info(&self, devinfo: DevInfo) {
        self.dev.write().unwrap().insert(devinfo)
    }

    fn get_sys_info(&mut self) -> Result<()> {
        debug!("requesting system info");
        Ok(self.write_line("GET,SYS,INFO")?)
    }

    fn update_sys_info(&self, artno: &str) {
        let mut dev = self.dev.write().unwrap();
        dev.insert(DevInfo::new("SYS1", "", 0, artno, ""));
        dev.insert(DevInfo::new("SYS2", "", 0, artno, ""));
        dev.insert(DevInfo::new("SYS3", "", 0, artno, ""));
    }

    fn next(&mut self) -> Result<Resp> {
        let mut buf = String::with_capacity(80);
        loop {
            buf.clear();
            self.conn.read_line(&mut buf)?;
            debug!("read: {}", buf);
            match Resp::parse(self.contno, &buf) {
                Ok(resp) => match resp {
                    ERR(e) => warn!("1-Wire error: {}", e.as_str()),
                    _ => return Ok(resp),
                },
                Err(e) => error!("{}", e),
            }
        }
    }

    fn process(&self, resp: Resp, tx: &Sender<Evt>) -> Result<()> {
        match resp {
            Event(msg) => {
                // if !dev.read().unwrap().has(&msg.busid) {
                //     self.get_device_info()?;
                // }
                tx.send(msg)?;
            }
            Info(devinfo) => self.update_device_info(devinfo),
            Artno(artno) => self.update_sys_info(artno.as_str()),
            Other(dev, msg) => debug!("Other({}, {})", dev.as_str(), msg),
            _ => (),
        }
        Ok(())
    }

    pub fn dispatch(&mut self, tx: Sender<Evt>) -> Result<()> {
        loop {
            let r = self.next()?;
            self.process(r, &tx)?;
        }
    }
}
