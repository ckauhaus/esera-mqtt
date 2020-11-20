use crate::parser::{self, Response};

use std::collections::VecDeque;
use std::fmt;
use std::io::prelude::*;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Transport(#[from] std::io::Error),
    #[error("Failed to parse controller reponse: {0}")]
    Parse(String),
    #[error("Controller connection lost while waiting for response")]
    Disconnected,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct ControllerConnection<S>
where
    S: Read + Write + fmt::Debug,
{
    pub queue: VecDeque<Result<Response>>,
    partial: String,
    stream: S,
}

impl ControllerConnection<TcpStream> {
    pub fn new<A: ToSocketAddrs>(addr: A) -> std::io::Result<Self> {
        let addr: Vec<_> = addr.to_socket_addrs()?.collect();
        info!("Connecting to 1wire at {:?}", addr);
        Ok(Self::from_stream(TcpStream::connect(&*addr)?))
    }
}

use std::convert::{TryFrom, TryInto};

impl<S: Read + Write + fmt::Debug> ControllerConnection<S> {
    pub fn from_stream(stream: S) -> Self {
        Self {
            queue: VecDeque::default(),
            partial: String::with_capacity(1 << 12),
            stream,
        }
    }

    /// Moves unparsed data from self.partial into queue as far as the parser allows.
    fn munch(&mut self) -> Option<Result<Response>> {
        let res = parser::parse(&self.partial);
        match res {
            Ok((rem, resp)) => {
                self.partial
                    .replace_range(0..(self.partial.len() - rem.len()), "");
                Some(Ok(resp))
            }
            Err(nom::Err::Incomplete(_)) => None, // try again later
            Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
                // delete one line
                let err = nom::error::convert_error(self.partial.as_ref(), e);
                self.partial
                    .replace_range(0..(self.partial.find('\n').map(|p| p + 1).unwrap_or(1)), "");
                Some(Err(Error::Parse(err)))
            }
        }
    }

    /// Gets additional data from underlying stream and parses it as fas as possible.
    /// Returns false if the underlying stream has been closed.
    fn receive(&mut self) -> Result<bool> {
        let mut buf = [0; 1 << 12];
        let len = self.stream.read(&mut buf)?;
        if len == 0 {
            return Ok(false);
        }
        let read = String::from_utf8_lossy(&buf[0..len]);
        debug!("<<< {}", read);
        self.partial.push_str(&read);
        while let Some(resp) = self.munch() {
            self.queue.push_back(resp);
        }
        Ok(true)
    }
}

impl<S> Iterator for ControllerConnection<S>
where
    S: Read + Write + fmt::Debug,
{
    type Item = Result<Response>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.queue.is_empty() {
            dbg!(&self.queue);
            match self.receive() {
                Ok(true) => (),
                Ok(false) => return None,
                Err(e) => return Some(Err(e)), // escalate transport errors quickly
            }
        }
        self.queue.pop_front()
    }
}

/// Usage: pick!(&mut conn, RESPONSE_VARIANT) -> Result<RESPONSE_TYPE>
macro_rules! pick {
    ($conn:expr, $res:tt) => {{
        let found = 'outer: loop {
            for (i, item) in $conn.queue.iter().enumerate() {
                if let Ok(resp) = item {
                    if let Response::$res(_) = resp {
                        break 'outer Ok(i);
                    }
                }
            }
            match $conn.receive() {
                Ok(true) => (),
                Ok(false) => break Err(Error::Disconnected),
                Err(e) => break Err(e),
            }
        };
        found.map(|i| {
            if let Response::$res(val) = $conn.queue.remove(i).unwrap().unwrap() {
                val
            } else {
                panic!("internal error: matched item {} disappeared from queue", i)
            }
        })
    }};
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
        assert!(c.queue.is_empty());
    }

    #[test]
    fn wait_should_cut_out_match() {
        let mut c = ControllerConnection::from_stream(Cursor::new(
            B("1_KAL|1\n\
               1_DATAPRINT|1\n\
               1_DATE|07.11.20\n")
            .to_vec(),
        ));
        assert_eq!(pick!(&mut c, Dataprint).unwrap(), true);
        assert_eq!(
            c.queue
                .into_iter()
                .map(|r| r.unwrap())
                .collect::<Vec<Response>>(),
            vec![Response::Keepalive(1), Response::Date("07.11.20".into())]
        );
    }
}
