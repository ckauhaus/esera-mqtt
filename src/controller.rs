use crate::parser::{self, Response};
use crate::pick;

use chrono::Local;
use crossbeam::atomic::AtomicCell;
use crossbeam::channel::{Receiver, Sender};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::fmt;
use std::io::prelude::*;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::thread;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Transport(#[from] std::io::Error),
    #[error("Failed to parse controller response: {0}")]
    Parse(String),
    #[error("Controller connection lost while waiting for response")]
    Disconnected,
    #[error("Controller communication protocol error ({0})")]
    Controller(u8),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct ControllerConnection<S>
where
    S: Read + Write + fmt::Debug,
{
    pub queue: Mutex<VecDeque<Result<Response>>>,
    pub contno: u8,
    partial: Mutex<String>,
    stream: Mutex<S>,
}

impl ControllerConnection<TcpStream> {
    pub fn new<A: ToSocketAddrs + fmt::Debug>(addr: A) -> Result<Self> {
        info!("Connecting to 1-Wire controller at {:?}", addr);
        let conn = TcpStream::connect(&addr)?;
        conn.set_nodelay(false)?;
        let c = Self::from_stream(conn);
        c.setup()?;
        Ok(c)
    }

    pub fn connect<A: ToSocketAddrs>(&mut self, addr: A) -> Result<()> {
        let addr: Vec<_> = addr.to_socket_addrs()?.collect();
        info!("Connecting to 1-Wire controller at {:?}", addr);
        self.stream = Mutex::new(TcpStream::connect(&*addr)?);
        self.setup()
    }

    fn setup(&self) -> Result<()> {
        // self.send_line(format!("SET,SYS,RST,1"))?;
        // pick!(self, Rst)?;
        // pick!(self, Rdy)?;
        self.send_line(format!("SET,SYS,DATAPRINT,1"))?;
        pick!(self, Dataprint)?;
        let now = Local::now();
        self.send_line(format!("SET,SYS,DATE,{}", now.format("%d.%m.%y")))?;
        pick!(self, Date)?;
        self.send_line(format!("SET,SYS,TIME,{}", now.format("%H:%M:%S")))?;
        pick!(self, Time)?;
        self.send_line("SET,SYS,DATATIME,20")?;
        pick!(self, Datatime)?;
        self.send_line("SET,SYS,SAVE")?;
        pick!(self, Save)?;
        Ok(())
    }
}

/// Moves raw data out of `partial` as far as the parser allows.
fn munch(partial: &mut String) -> Option<Result<Response>> {
    let res = parser::parse(partial).map(|(rem, resp)| (rem.len(), resp));
    match res {
        Ok((rem, resp)) => {
            partial.replace_range(0..(partial.len() - rem), "");
            Some(Ok(resp))
        }
        Err(nom::Err::Incomplete(_)) => None, // try again later
        Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
            // delete one line
            let err = nom::error::convert_error(partial.as_ref(), e);
            partial.replace_range(0..(partial.find('\n').map(|p| p + 1).unwrap_or(1)), "");
            Some(Err(Error::Parse(err)))
        }
    }
}

impl<S> ControllerConnection<S>
where
    S: Read + Write + fmt::Debug,
{
    pub fn from_stream(stream: S) -> Self {
        Self {
            queue: Mutex::new(VecDeque::default()),
            contno: 0,
            partial: Mutex::new(String::with_capacity(1 << 12)),
            stream: Mutex::new(stream),
        }
    }

    /// Writes a single line to the underlaying stream. Newline will be appended.
    pub fn send_line<L: Into<String>>(&self, line: L) -> Result<(), std::io::Error> {
        let mut line = line.into();
        line.push_str("\r\n");
        debug!("[{}] >>> {}", self.contno, line.trim());
        self.stream.lock().write_all(line.as_bytes())
    }

    /// Gets additional data from underlying stream and parses it as fas as possible.
    /// Returns false if the underlying stream has been closed.
    fn receive(&self) -> Result<bool> {
        let mut buf = [0; 1 << 10];
        let len = self.stream.lock().read(&mut buf)?;
        if len == 0 {
            return Ok(false);
        }
        let read = String::from_utf8_lossy(&buf[0..len]);
        debug!("[{}] <<< {}", self.contno, read.trim());
        let mut partial = self.partial.lock();
        let mut queue = self.queue.lock();
        partial.push_str(&read);
        while let Some(resp) = munch(&mut partial) {
            queue.push_back(resp);
        }
        Ok(true)
    }

    pub fn get(&self) -> Option<Result<Response>> {
        while self.queue.lock().is_empty() {
            thread::sleep(Duration::from_millis(25));
            match self.receive() {
                Ok(true) => (),
                Ok(false) => return None,
                Err(e) => return Some(Err(e)), // escalate transport errors quickly
            }
        }
        self.queue.lock().pop_front()
    }

    pub fn csi(&mut self) -> Result<parser::CSI> {
        self.send_line("GET,SYS,INFO")?;
        let csi = pick!(&self, CSI)?;
        self.contno = csi.contno;
        Ok(csi)
    }

    pub fn list(&self) -> Result<parser::List3> {
        self.send_line("GET,OWB,LISTALL1")?;
        pick!(&self, List3)
    }
}

/// Usage: pick!(&mut conn, RESPONSE_VARIANT) -> Result<RESPONSE_TYPE>
#[macro_export]
macro_rules! pick {
    ($conn:expr, $res:tt) => {
        (|| {
            let found = 'outer: loop {
                for (i, item) in $conn.queue.lock().iter().enumerate() {
                    if let Ok(resp) = item {
                        match resp {
                            Response::$res(_) => break 'outer Ok(i),
                            Response::Err(e) => return Err(Error::Controller(*e)),
                            _ => (),
                        }
                    }
                }
                // item not already present in queue, wait for more data
                std::thread::sleep(Duration::from_millis(25));
                match $conn.receive() {
                    Ok(true) => (),
                    Ok(false) => break Err(Error::Disconnected),
                    Err(e) => break Err(e),
                }
            };
            found.map(|i| {
                if let Response::$res(val) = $conn.queue.lock().remove(i).unwrap().unwrap() {
                    val
                } else {
                    panic!("internal error: matched item {} disappeared from queue", i)
                }
            })
        })()
    };
}

impl<S> ControllerConnection<S>
where
    S: Read + Write + fmt::Debug + Send,
{
    pub fn event_loop(&self, up: Receiver<String>, down: Sender<Result<Response>>) -> Result<()> {
        let done = AtomicCell::new(false);
        crossbeam::scope(|sc| {
            sc.builder()
                .name("reader".into())
                .spawn(|_| {
                    while let Some(item) = self.get() {
                        if down.send(item).is_err() {
                            // channel closed
                            done.store(true);
                            return;
                        }
                        if done.load() {
                            return;
                        }
                    }
                    // underlying transport closed
                    warn!("[{}] Controller connection unexpectly lost", self.contno);
                    done.store(true);
                })
                .unwrap();
            sc.builder()
                .name("writer".into())
                .spawn(|_| {
                    while let Ok(line) = up.recv() {
                        if let Err(e) = self.send_line(line) {
                            error!("[{}] Cannot send controller event: {}", self.contno, e);
                            done.store(true);
                            return;
                        }
                        if done.load() {
                            return;
                        }
                    }
                    done.store(true)
                })
                .unwrap();
        })
        .unwrap();
        Ok(())
    }
}

impl<S> Iterator for ControllerConnection<S>
where
    S: Read + Write + fmt::Debug,
{
    type Item = Result<Response>;

    fn next(&mut self) -> Option<Self::Item> {
        self.get()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use assert_matches::assert_matches;
    use bstr::B;
    use std::io::Cursor;

    #[test]
    fn get_next_item() {
        let mut c = ControllerConnection::from_stream(Cursor::new(B("1_EVT|21:02:43\n").to_vec()));
        assert_matches!(c.next(), Some(Ok(Response::Event(_))));
    }

    #[test]
    fn wait_on_closed_reader_should_fail() {
        let mut c = ControllerConnection::from_stream(Cursor::new(B("").to_vec()));
        assert_matches!(c.next(), None);
    }

    #[test]
    fn parse_garbage() {
        let mut c = ControllerConnection::from_stream(Cursor::new(
            B("<BS>i������J���Ӈ��\n1_INF|21:28:53\n").to_vec(),
        ));
        assert_matches!(c.next(), Some(Err(Error::Parse(_))));
        assert_matches!(c.next(), Some(Ok(Response::Info(_))));
        assert_matches!(c.next(), None);
    }

    #[test]
    fn pick_should_return_match() {
        let mut c = ControllerConnection::from_stream(Cursor::new(B("1_DATE|20.09.20\n").to_vec()));
        assert_eq!(pick!(&mut c, Date).unwrap(), "20.09.20".to_string());
        assert!(c.queue.lock().is_empty());
    }

    #[test]
    fn wait_should_cut_out_match() {
        let mut c = ControllerConnection::from_stream(Cursor::new(
            B("1_KAL|1\n\
               1_DATAPRINT|1\n\
               1_DATE|07.11.20\n")
            .to_vec(),
        ));
        assert_eq!(pick!(&mut c, Dataprint).unwrap().flag, '1');
        assert_eq!(
            c.queue
                .into_inner()
                .into_iter()
                .map(|r| r.unwrap())
                .collect::<Vec<Response>>(),
            vec![Response::Keepalive(1), Response::Date("07.11.20".into())]
        );
    }
}
