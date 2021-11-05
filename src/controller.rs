use crate::parser::{self, Msg, MsgKind, OW};

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
    Controller(u16),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct ControllerConnection<S>
where
    S: Read + Write + fmt::Debug,
{
    pub queue: Mutex<VecDeque<Result<OW>>>,
    pub contno: u8,
    partial: Mutex<String>,
    reader: Mutex<S>,
    writer: Mutex<S>,
}

impl ControllerConnection<TcpStream> {
    pub fn new<A: ToSocketAddrs + fmt::Debug>(addr: A) -> Result<Self> {
        info!("Connecting to 1-Wire controller at {:?}", addr);
        let conn = TcpStream::connect(&addr)?;
        conn.set_nodelay(false)?;
        conn.set_read_timeout(Some(Duration::new(300, 0)))?;
        let reader = conn.try_clone().unwrap();
        let c = Self::from_streams(reader, conn);
        c.setup()?;
        Ok(c)
    }

    fn setup(&self) -> Result<()> {
        self.send_line("SET,SYS,DATAPRINT,1".to_owned())?;
        self.pick(MsgKind::Dataprint)?;
        let now = Local::now();
        self.send_line(format!("SET,SYS,DATE,{}", now.format("%d.%m.%y")))?;
        self.pick(MsgKind::Date)?;
        self.send_line(format!("SET,SYS,TIME,{}", now.format("%H:%M:%S")))?;
        self.pick(MsgKind::Time)?;
        self.send_line("SET,SYS,KALSENDTIME,120")?;
        self.pick(MsgKind::Kalsendtime)?;
        self.send_line("SET,SYS,DATATIME,30")?;
        self.pick(MsgKind::Datatime)?;
        self.send_line("SET,SYS,SAVE")?;
        self.pick(MsgKind::Save)?;
        Ok(())
    }
}

/// Moves raw data out of `partial` as far as the parser allows.
fn munch(partial: &mut String) -> Option<Result<OW>> {
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
    pub fn from_streams(reader: S, writer: S) -> Self {
        Self {
            queue: Mutex::new(VecDeque::default()),
            contno: 0,
            partial: Mutex::new(String::with_capacity(1 << 12)),
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
        }
    }

    /// Writes a single line to the underlaying stream. Newline will be appended.
    pub fn send_line<L: Into<String>>(&self, line: L) -> Result<(), std::io::Error> {
        let mut line = line.into();
        debug!("[{}] >>> {}", self.contno, line.trim());
        if !line.ends_with("\r\n") {
            line.push_str("\r\n");
        }
        let mut w = self.writer.lock();
        w.write_all(line.as_bytes())?;
        w.flush()
    }

    /// Gets additional data from underlying stream and parses it as fas as possible.
    /// Returns false if the underlying stream has been closed.
    fn receive(&self) -> Result<bool> {
        let mut buf = [0; 1 << 10];
        let len = self.reader.lock().read(&mut buf)?;
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

    /// Returns top queue item or waits for new data if the queue is empty.
    pub fn get(&self) -> Option<Result<OW>> {
        while self.queue.lock().is_empty() {
            thread::sleep(Duration::from_millis(10));
            match self.receive() {
                Ok(true) => (),
                Ok(false) => return None,
                Err(e) => return Some(Err(e)), // escalate transport errors quickly
            }
        }
        self.queue.lock().pop_front()
    }

    pub fn csi(&mut self) -> Result<OW> {
        self.send_line("GET,SYS,INFO")?;
        let csi = self.pick(MsgKind::CSI)?;
        self.contno = csi.contno;
        Ok(csi)
    }

    pub fn list(&self) -> Result<OW> {
        self.send_line("GET,OWB,LISTALL1")?;
        self.pick(MsgKind::List3)
    }

    /// Pulls a message of the specified kind from the queue (out of order). Waits for more data
    /// until a message of the given kind is present.
    pub fn pick(&self, kind: MsgKind) -> Result<OW> {
        loop {
            {
                let mut queue = self.queue.lock();
                for (i, item) in queue.iter().enumerate() {
                    if let Ok(resp) = item {
                        if MsgKind::from(&resp.msg) == kind {
                            return queue.remove(i).unwrap();
                        }
                        if let Msg::Err(e) = resp.msg {
                            return Err(Error::Controller(e));
                        }
                    }
                }
            }
            // item not already present in queue, wait for more data
            thread::sleep(Duration::from_millis(10));
            match self.receive() {
                Ok(true) => (),
                Ok(false) => return Err(Error::Disconnected),
                Err(e) => return Err(e),
            }
        }
    }
}

impl<S> ControllerConnection<S>
where
    S: Read + Write + fmt::Debug + Send,
{
    pub fn event_loop(&self, up: Receiver<String>, down: Sender<Result<OW>>) -> Result<()> {
        let done = AtomicCell::new(false);
        crossbeam::scope(|sc| {
            let hdl = vec![
                sc.builder()
                    .name("reader".into())
                    .spawn(|_| {
                        while let Some(item) = self.get() {
                            if down.send(item).is_err() {
                                // channel closed
                                done.store(true);
                                return Ok(());
                            }
                            if done.load() {
                                // other thread has exited
                                return Ok(());
                            }
                        }
                        warn!("[{}] Controller connection unexpectely lost", self.contno);
                        done.store(true);
                        Err(Error::Disconnected)
                    })
                    .unwrap(),
                sc.builder()
                    .name("writer".into())
                    .spawn(|_| {
                        while let Ok(line) = up.recv() {
                            if let Err(e) = self.send_line(line) {
                                error!("[{}] Cannot send controller event: {}", self.contno, e);
                                done.store(true);
                                return Err(Error::Disconnected);
                            }
                            if done.load() {
                                // other thread has exited
                                return Ok(());
                            }
                            thread::sleep(Duration::from_millis(50));
                        }
                        done.store(true);
                        // channel closed
                        Ok(())
                    })
                    .unwrap(),
            ];
            hdl.into_iter().try_for_each(|h| h.join().unwrap())
        })
        .unwrap()
    }
}

impl<S> Iterator for ControllerConnection<S>
where
    S: Read + Write + fmt::Debug,
{
    type Item = Result<OW>;

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
        let mut c = ControllerConnection::from_streams(
            Cursor::new(B("1_EVT|21:02:43\n").to_vec()),
            Cursor::new(Vec::new()),
        );
        assert_matches!(
            c.next(),
            Some(Ok(OW {
                msg: Msg::Evt(_),
                ..
            }))
        );
    }

    #[test]
    fn wait_on_closed_reader_should_fail() {
        let mut c = ControllerConnection::from_streams(
            Cursor::new(B("").to_vec()),
            Cursor::new(Vec::new()),
        );
        assert_matches!(c.next(), None);
    }

    #[test]
    fn parse_garbage() {
        let mut c = ControllerConnection::from_streams(
            Cursor::new(B("<BS>i������J���Ӈ��\n1_INF|21:28:53\n").to_vec()),
            Cursor::new(Vec::new()),
        );
        assert_matches!(c.next(), Some(Err(Error::Parse(_))));
        assert_matches!(
            c.next(),
            Some(Ok(OW {
                msg: Msg::Inf(_),
                ..
            }))
        );
        assert_matches!(c.next(), None);
    }

    #[test]
    fn pick_should_return_match() {
        let c = ControllerConnection::from_streams(
            Cursor::new(B("1_DATE|20.09.20\n").to_vec()),
            Cursor::new(Vec::new()),
        );
        let res = c.pick(MsgKind::Date).unwrap();
        assert_eq!(res.msg, Msg::Date("20.09.20".into()));
        assert!(c.queue.lock().is_empty());
    }

    #[test]
    fn wait_should_cut_out_match() {
        let c = ControllerConnection::from_streams(
            Cursor::new(
                B("1_KAL|1\n\
               1_DATAPRINT|1\n\
               1_DATE|07.11.20\n")
                .to_vec(),
            ),
            Cursor::new(Vec::new()),
        );
        let res = c.pick(MsgKind::Dataprint).unwrap();
        assert_matches!(
            res,
            OW {
                msg: Msg::Dataprint('1'),
                ..
            }
        );
        let mut q = c.queue.into_inner().into_iter().map(|r| r.unwrap());
        assert_eq!(q.next().unwrap().msg, Msg::Keepalive('1'));
        assert_eq!(q.next().unwrap().msg, Msg::Date("07.11.20".into()));
        assert_eq!(q.next(), None);
    }
}
