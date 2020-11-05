use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::char as cc;
use nom::character::complete::{alphanumeric1, anychar, digit1, none_of};
use nom::combinator::{all_consuming, map_res, opt, recognize};
use nom::multi::{many0, many1, separated_nonempty_list};
use nom::sequence::{preceded, separated_pair, terminated, tuple};
use std::convert::TryFrom;
use std::str::FromStr;
use thiserror::Error;

type Str = smol_str::SmolStr;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Unexpected response: {0}")]
    Unexpected(String),
    #[error("Invalid data format: {0}")]
    Invalid(String),
    #[error("Failed to parse number")]
    Decimal(#[source] std::num::ParseIntError),
    #[error("Invalid syntax: {0}")]
    Syntax(String),
    #[error("Line too short (truncated?)")]
    Incomplete,
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Recv<'l> {
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

impl<'a> TryFrom<&'a str> for Recv<'a> {
    type Error = Error;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        Ok(tokenize(s)
            .map_err(|e| match e {
                nom::Err::Error(err) | nom::Err::Failure(err) => {
                    Error::Syntax(nom::error::convert_error(s, err))
                }
                nom::Err::Incomplete(_) => Error::Incomplete,
            })?
            .1)
    }
}

impl<'a> TryFrom<&'a String> for Recv<'a> {
    type Error = Error;

    fn try_from(s: &'a String) -> Result<Self, Self::Error> {
        Self::try_from(s.as_str())
    }
}

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

fn tokenize<'a>(line: &'a str) -> IResult<Recv<'a>> {
    all_consuming(alt((tok_regular, tok_list)))(line)
}

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

impl FromStr for Status {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "S_0" => Ok(Self::Online),
            "S_1" => Ok(Self::Err1),
            "S_2" => Ok(Self::Err2),
            "S_3" => Ok(Self::Err3),
            "S_5" => Ok(Self::Offline),
            "S_10" => Ok(Self::Unconfigured),
            _ => Err(Error::Unexpected(format!(
                "Failed to parse status code {:?}",
                s
            ))),
        }
    }
}

use strum_macros::{AsRefStr, EnumDiscriminants};

#[derive(Debug, Clone, PartialEq, Eq, EnumDiscriminants, AsRefStr)]
#[strum_discriminants(name(ResponseKind))]
#[strum_discriminants(derive(AsRefStr))]
pub enum Response {
    Contno(u8),
    Artno(String),
    Event {
        busid: Str,
        port: u8,
        data: u8,
    },
    Dataprint(bool),
    KAL,
    Evt,
    Date(String),
    Time(String),
    List(u8), // list mode to follow
    Lst3 {
        busid: Str,
        serial: Str,
        status: Status,
        artno: Str,
        name: Option<Str>,
    },
    Unkown(String),
}

pub struct Parser {
    list_mode: u8,
}

impl Parser {
    pub fn new() -> Self {
        Self { list_mode: 0 }
    }

    fn parse_reg<'a>(
        &mut self,
        line: &'a str,
        key: &'a str,
        _sub: Option<u8>,
        arg: &'a str,
    ) -> Result<Response> {
        Ok(match key {
            "CONTNO" => Response::Contno(arg.parse().map_err(Error::Decimal)?),
            "ARTNO" => Response::Artno(arg.to_owned()),
            "DATE" => Response::Date(arg.to_owned()),
            "TIME" => Response::Time(arg.to_owned()),
            "DATAPRINT" => match arg {
                "1" => Response::Dataprint(true),
                "0" => Response::Dataprint(false),
                _ => return Err(Error::Invalid(key.to_owned())),
            },
            "KAL" => Response::KAL,
            "LST3" => {
                self.list_mode = 3;
                Response::List(3)
            }
            _ => Response::Unkown(line.to_owned()),
        })
    }

    fn parse_lst<'a>(
        &mut self,
        line: &'a str,
        key: Option<&'a str>,
        args: &'a [&'a str],
    ) -> Result<Response> {
        Ok(match self.list_mode {
            3 => {
                if args.len() < 3 || args.len() > 4 {
                    return Err(Error::Invalid(
                        "LST3 contains wrong number of fields".into(),
                    ));
                }
                Response::Lst3 {
                    busid: key
                        .map(|b| Str::from(b))
                        .ok_or_else(|| Error::Invalid("busid required in LST3 format".into()))?,
                    serial: args[0].into(),
                    status: args[1].parse().map_err(|_| {
                        Error::Invalid(format!("Cannot recognize status {:?}", args[1]))
                    })?,
                    artno: args[2].into(),
                    name: args.get(3).map(|n| Str::from(*n)),
                }
            }
            _ => Response::Unkown(line.to_owned()),
        })
    }

    pub fn parse(&mut self, line: &str) -> Result<Response> {
        let recv = Recv::try_from(line)?;
        match recv {
            Recv::Reg { key, sub, arg, .. } => self.parse_reg(line, key, sub, arg),
            Recv::Lst { key, args, .. } => self.parse_lst(line, key, &args),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use assert_matches::assert_matches;

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
    fn tokenize_short_line() {
        assert_matches!(tokenize("1_OWD|").unwrap_err(), nom::Err::Error(_));
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

    #[test]
    fn parse_keepalive() {
        let mut p = Parser::new();
        assert_eq!(p.parse("1_KAL|1").unwrap(), Response::KAL);
    }

    #[test]
    fn parse_dataprint() {
        assert_eq!(
            Parser::new().parse("1_DATAPRINT|1").unwrap(),
            Response::Dataprint(true)
        );
    }

    #[test]
    fn parse_lst3() {
        let mut p = Parser::new();
        assert_eq!(p.parse("1_LST3|0:02:54").unwrap(), Response::List(3));
        assert_eq!(
            p.parse("LST|1_OWD3|F400001BA2F0EC29|S_5|DS2408").unwrap(),
            Response::Lst3 {
                busid: "OWD3".into(),
                serial: "F400001BA2F0EC29".into(),
                status: Status::Offline,
                artno: "DS2408".into(),
                name: None
            }
        );
        assert_eq!(
            p.parse("LST|1_OWD1|EF000019096A4026|S_0|11150|TEMP")
                .unwrap(),
            Response::Lst3 {
                busid: "OWD1".into(),
                serial: "EF000019096A4026".into(),
                status: Status::Online,
                artno: "11150".into(),
                name: Some("TEMP".into())
            }
        );
    }
}
