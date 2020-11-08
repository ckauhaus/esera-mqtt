#![allow(unused)]

use std::convert::TryFrom;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum Error {
    #[error("Unexpected response: {0}")]
    Unexpected(String),
    #[error("Invalid data format: {0}")]
    Invalid(String),
    #[error("Failed to parse number")]
    Decimal(#[from] std::num::ParseIntError),
    #[error("Invalid UTF-8 in controller reponse")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Invalid UTF-8 in controller reponse")]
    FromUtf8(#[from] std::string::FromUtf8Error),
    #[error("Invalid syntax: {0}")]
    Syntax(String),
    #[error("Line too short (truncated?)")]
    Incomplete,
    #[error("Invalid status code")]
    Status(#[from] strum::ParseError),
}

type Result<T, E = Error> = std::result::Result<T, E>;
pub type PResult<'i, O> = nom::IResult<&'i str, O, nom::error::VerboseError<&'i str>>;

use strum_macros::{AsRefStr, Display, EnumDiscriminants, EnumString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
pub enum Status {
    #[strum(serialize = "S_0")]
    Online,
    #[strum(serialize = "S_1")]
    Err1,
    #[strum(serialize = "S_2")]
    Err2,
    #[strum(serialize = "S_3")]
    Err3,
    #[strum(serialize = "S_5")]
    Offline,
    #[strum(serialize = "S_10")]
    Unconfigured,
}

use Status::*;

#[derive(Debug, Clone, PartialEq, EnumDiscriminants, AsRefStr)]
#[strum_discriminants(name(ResponseKind))]
#[strum_discriminants(derive(AsRefStr))]
pub enum Response {
    Keepalive,
    Event(String),
    Dataprint(bool),
    Date(String),
    Time(String),
    List3(Vec<List3Item>),
    Unkown(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct List3Item {
    busid: String,
    serial: String,
    status: Status,
    artno: String,
    name: Option<String>,
}

use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::not_line_ending as till_nl;
use nom::character::streaming::{
    alphanumeric0, alphanumeric1, anychar, char as cc, digit1, line_ending, none_of,
    not_line_ending,
};
use nom::combinator::{all_consuming, map, map_res, not, opt, peek, recognize, value};
use nom::error::{Error as NomError, ErrorKind, ParseError};
use nom::multi::{many0, many1, many_m_n};
use nom::sequence::{preceded, separated_pair, terminated, tuple};

fn contno(i: &str) -> PResult<u8> {
    map_res(terminated(digit1, cc('_')), |val: &str| val.parse::<u8>())(i)
}

fn identifier(i: &str) -> PResult<&str> {
    recognize(many1(alt((alphanumeric1, tag("_")))))(i)
}

fn statement<'a>(key: &'static str) -> impl FnMut(&'a str) -> PResult<'a, u8> {
    terminated(contno, terminated(tag(key), cc('|')))
}

fn remainder(i: &str) -> PResult<&str> {
    terminated(not_line_ending, line_ending)(i)
}

pub fn kal(i: &str) -> PResult<Response> {
    value(Response::Keepalive, terminated(statement("KAL"), remainder))(i)
}

pub fn dataprint(i: &str) -> PResult<bool> {
    let (i, _) = statement("DATAPRINT")(i)?;
    map_res(remainder, |s: &str| match s {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(NomError::new(s, ErrorKind::ParseTo)),
    })(i)
}

// XXX recognize [0-9.]+
pub fn date(i: &str) -> PResult<String> {
    map(preceded(statement("DATE"), remainder), |s| s.to_owned())(i)
}

// XXX recognize [0-9:]+
pub fn time(i: &str) -> PResult<String> {
    map(preceded(statement("TIME"), remainder), |s| s.to_owned())(i)
}

pub fn lst3(i: &str) -> PResult<Vec<List3Item>> {
    let (i, _) = terminated(statement("LST3"), remainder)(i)?;
    let (i, res) = many1(map_res(
        tuple((
            preceded(tuple((tag("LST|"), contno)), alphanumeric1),
            preceded(cc('|'), alphanumeric1),
            preceded(cc('|'), identifier),
            preceded(cc('|'), alphanumeric1),
            opt(preceded(cc('|'), not_line_ending)),
            line_ending,
        )),
        |(busid, serial, status, artno, name, _nl)| -> Result<_, Error> {
            Ok(List3Item {
                busid: busid.to_owned(),
                serial: serial.to_owned(),
                status: status.parse()?,
                artno: artno.to_owned(),
                name: name.map(|n| n.to_owned()),
            })
        },
    ))(i)?;
    let _followed_by_non_list = many_m_n(4, 4, anychar)(i)?;
    Ok((i, res))
}

// XXX unsure about API
// pub fn parse(i: &str) -> Result<Response> {
//     alt((
//         map(kal, |_| Response::Keepalive),
//         map(dataprint, |v| Response::Dataprint(v)),
//         map(lst3, |v| Response::List3(v)),
//         map(line, |v| Response::Unkown(v.to_owned())),
//     ))(i)
// }

#[cfg(test)]
mod test {
    use super::*;
    use assert_matches::assert_matches;
    use pretty_assertions::assert_eq;

    #[test]
    fn parse_keepalive() {
        assert_eq!(kal("1_KAL|1\n").unwrap(), ("", Response::Keepalive));
    }

    #[test]
    fn parse_incomplete() {
        assert_matches!(kal("1_KAL|1").unwrap_err(), nom::Err::Incomplete(_));
    }

    #[test]
    fn parse_dataprint() {
        assert_eq!(dataprint("1_DATAPRINT|1\n").unwrap(), ("", true));
    }

    #[test]
    fn parse_date() {
        assert_eq!(
            date("2_DATE|03.11.20\n").unwrap(),
            ("", "03.11.20".to_owned())
        );
    }

    #[test]
    fn parse_time() {
        assert_eq!(
            time("3_TIME|0:00:52\n").unwrap(),
            ("", "0:00:52".to_owned())
        );
    }

    #[test]
    fn signal_incomplete_list() {
        let input = "\
1_LST3|0:02:54\n\
LST|1_OWD1|EF000019096A4026|S_0|11150\n";
        assert_matches!(lst3(input).unwrap_err(), nom::Err::Incomplete(_))
    }

    #[test]
    fn parse_lst3() {
        let input = "\
1_LST3|00:02:54\n\
LST|1_OWD1|EF000019096A4026|S_0|11150\n\
LST|1_OWD2|4300001982956429|S_0|DS2408\n\
LST|1_OWD4|FFFFFFFFFFFFFFFF|S_10|none\n\
1_EVT|0:02:55\n";
        let res = lst3(input);
        dbg!(&res);
        let (rem, mtch) = res.unwrap();
        assert_eq!(rem, "1_EVT|0:02:55\n");
        assert_eq!(
            mtch,
            vec![
                List3Item {
                    busid: "OWD1".into(),
                    serial: "EF000019096A4026".into(),
                    status: Online,
                    artno: "11150".into(),
                    name: None
                },
                List3Item {
                    busid: "OWD2".into(),
                    serial: "4300001982956429".into(),
                    status: Online,
                    artno: "DS2408".into(),
                    name: None
                },
                List3Item {
                    busid: "OWD4".into(),
                    serial: "FFFFFFFFFFFFFFFF".into(),
                    status: Unconfigured,
                    artno: "none".into(),
                    name: None
                },
            ]
        );
    }
}
