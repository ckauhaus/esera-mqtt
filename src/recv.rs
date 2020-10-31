use futures::sink::SinkExt;
use futures::stream::{Fuse, FusedStream, Stream, StreamExt};
use futures::task;
use std::fmt;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::str::FromStr;
use std::task::Poll;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LinesCodec, LinesCodecError};

#[derive(Error, Debug, PartialEq, Clone)]
pub enum Error {
    #[error("Response has invalid syntax: {0}")]
    Syntax(String),
    #[error("Response too short")]
    IncompleteInput,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Online = 0,
    Err1,
    Err2,
    Err3,
    Offline = 5,
    Unconfigured = 10,
}

impl Default for Status {
    fn default() -> Self {
        Status::Unconfigured
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recv<'l> {
    Reg {
        contno: u8,
        key: &'l str,
        sub: Option<u8>,
        arg: &'l str,
    },
    Lst {
        contno: Option<u8>,
        key: Option<&'l str>,
        args: Vec<&'l str>,
    },
}

impl Default for Recv<'_> {
    fn default() -> Self {
        Recv::Reg {
            contno: 0,
            key: "",
            sub: None,
            arg: "",
        }
    }
}

impl<'a> std::convert::TryFrom<&'a str> for Recv<'a> {
    type Error = Error;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        Ok(lexer::tokenize(s)
            .map_err(|e| match e {
                nom::Err::Error(err) | nom::Err::Failure(err) => {
                    Error::Syntax(nom::error::convert_error(s, err))
                }
                nom::Err::Incomplete(_) => Error::IncompleteInput,
            })?
            .1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response<'l> {
    Contno(u8),
    Event { busid: &'l str, port: u8, data: u8 },
    Dataprint(bool),
    Unkown(Recv<'l>),
    KAL,
    Date(String),
    Time(String),
}

type ConnectionInner = Fuse<Framed<TcpStream, LinesCodec>>;

pub struct Connection {
    stream: ConnectionInner,
}

impl Connection {
    pub async fn new<A: ToSocketAddrs>(addr: A) -> Result<Self, std::io::Error> {
        let addr: Vec<_> = addr.to_socket_addrs()?.collect();
        info!("Connecting to {:?}", addr);
        let s = TcpStream::connect(&*addr).await?;
        Ok(Self {
            stream: Framed::new(s, LinesCodec::new()).fuse(),
        })
    }
}

impl std::ops::Deref for Connection {
    type Target = Fuse<Framed<TcpStream, LinesCodec>>;
    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl std::ops::DerefMut for Connection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stream
    }
}

mod lexer {
    use super::*;

    use nom::branch::alt;
    use nom::bytes::complete::tag;
    use nom::character::complete::char as cc;
    use nom::character::complete::{alphanumeric1, anychar, digit1, none_of, not_line_ending};
    use nom::combinator::{all_consuming, map_res, opt, recognize, value};
    use nom::error::ErrorKind;
    use nom::multi::{many0, many1, separated_nonempty_list};
    use nom::sequence::{preceded, separated_pair, terminated, tuple};

    type IResult<'a, O> = nom::IResult<&'a str, O, nom::error::VerboseError<&'a str>>;

    fn contkeysub(input: &str) -> IResult<(u8, &str, Option<u8>)> {
        tuple((
            map_res(digit1, |s: &str| s.parse::<u8>()),
            preceded(cc('_'), recognize(alphanumeric1)),
            opt(preceded(
                cc('_'),
                map_res(digit1, |s: &str| s.parse::<u8>()),
            )),
        ))(input)
    }

    fn tok_regular(line: &str) -> IResult<Recv> {
        let (i, ((contno, key, sub), arg)) =
            separated_pair(contkeysub, cc('|'), recognize(many1(anychar)))(line)?;
        Ok((
            i,
            Recv::Reg {
                contno,
                key,
                sub,
                arg,
            },
        ))
    }

    fn tok_list(input: &str) -> IResult<Recv> {
        let (i, _) = tag("LST|")(input)?;
        let (i, (contkey, args)) = tuple((
            opt(terminated(contkeysub, cc('|'))),
            separated_nonempty_list(cc('|'), recognize(many0(none_of("|")))),
        ))(i)?;
        Ok((
            i,
            Recv::Lst {
                contno: contkey.map(|c| c.0),
                key: contkey.map(|c| c.1),
                args,
            },
        ))
    }

    pub fn tokenize<'a>(line: &'a str) -> IResult<Recv<'a>> {
        all_consuming(alt((tok_regular, tok_list)))(line)
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        fn tokenize_regular() {
            assert_eq!(
                tokenize("2_HW|V2.0").unwrap().1,
                Recv::Reg {
                    contno: 2,
                    key: "HW",
                    sub: None,
                    arg: "V2.0"
                }
            );
        }

        #[test]
        fn tokenize_regular_subaddr() {
            assert_eq!(
                tokenize("1_OWD1_1|2266").unwrap().1,
                Recv::Reg {
                    contno: 1,
                    key: "OWD1",
                    sub: Some(1),
                    arg: "2266"
                }
            );
        }

        #[test]
        fn tokenize_list() {
            assert_eq!(
                tokenize("LST|1_OWD1|EF000019096A4026|S_0|11150").unwrap().1,
                Recv::Lst {
                    contno: Some(1),
                    key: Some("OWD1"),
                    args: vec!["EF000019096A4026", "S_0", "11150"]
                }
            );
        }

        #[test]
        fn tokenize_naked_list() {
            assert_eq!(
                tokenize("LST|4300001982956429|DS2408").unwrap().1,
                Recv::Lst {
                    contno: None,
                    key: None,
                    args: vec!["4300001982956429", "DS2408"]
                }
            );
        }
    }
}
