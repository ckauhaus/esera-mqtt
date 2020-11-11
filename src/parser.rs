#![allow(unused)]
use crate::Status::{self, *};

use std::convert::TryFrom;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum Error {
    #[error("Invalid UTF-8 in controller reponse")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Invalid UTF-8 in controller reponse")]
    FromUtf8(#[from] std::string::FromUtf8Error),
    #[error("Invalid syntax: {0}")]
    Syntax(String),
    #[error("Invalid status code")]
    Status(#[from] strum::ParseError),
    #[error("Cannot parse numeric argument")]
    ParseInt(#[from] std::num::ParseIntError),
}

type Result<T, E = Error> = std::result::Result<T, E>;
pub type PResult<'i, O> = nom::IResult<&'i str, O, nom::error::VerboseError<&'i str>>;

use strum_macros::{AsRefStr, Display, EnumDiscriminants, EnumString};

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
    CSI(CSI),
    Devstatus { addr: String, data: u32 },
}

use nom::branch::alt;
use nom::bytes::streaming::{tag, take_till1};
use nom::character::streaming::{
    alphanumeric0, alphanumeric1, anychar, char as cc, digit1, line_ending, none_of,
    not_line_ending, one_of,
};
use nom::combinator::{all_consuming, map, map_res, not, opt, peek, recognize, value};
use nom::error::{Error as NomError, ErrorKind, ParseError};
use nom::multi::{many0, many1, many_m_n};
use nom::sequence::{delimited, preceded, separated_pair, terminated, tuple};

fn contno(i: &str) -> PResult<u8> {
    map_res(terminated(digit1, cc('_')), |val: &str| val.parse())(i)
}

fn header<'a>(key: &'static str) -> impl FnMut(&'a str) -> PResult<'a, u8> {
    terminated(contno, terminated(tag(key), cc('|')))
}

fn remainder(i: &str) -> PResult<&str> {
    terminated(not_line_ending, line_ending)(i)
}

fn val<'a>(key: &'static str) -> impl FnMut(&'a str) -> PResult<'a, &'a str> {
    delimited(header(key), not_line_ending, line_ending)
}

pub fn kal(i: &str) -> PResult<Response> {
    value(Response::Keepalive, terminated(header("KAL"), remainder))(i)
}

pub fn evt(i: &str) -> PResult<String> {
    map(
        delimited(
            header("EVT"),
            recognize(many1(one_of("0123456789:"))),
            line_ending,
        ),
        String::from,
    )(i)
}

pub fn dataprint(i: &str) -> PResult<bool> {
    map(
        delimited(header("DATAPRINT"), one_of("01"), line_ending),
        |c| !matches!(c, '0'),
    )(i)
}

pub fn datatime(i: &str) -> PResult<u8> {
    map(delimited(header("DATATIME"), digit1, line_ending), |s| {
        s.parse().unwrap()
    })(i)
}

pub fn date(i: &str) -> PResult<String> {
    map(
        delimited(
            header("DATE"),
            recognize(many1(one_of("0123456789."))),
            line_ending,
        ),
        String::from,
    )(i)
}

pub fn time(i: &str) -> PResult<String> {
    map(
        delimited(
            header("TIME"),
            recognize(many1(one_of("0123456789:"))),
            line_ending,
        ),
        String::from,
    )(i)
}

#[derive(Debug, Clone, PartialEq)]
pub struct CSI {
    pub date: String,
    pub time: String,
    pub artno: String,
    pub serno: String,
    pub fw: String,
    pub hw: String,
    pub contno: u8,
}

pub fn csi(i: &str) -> PResult<CSI> {
    map(
        tuple((
            val("CSI"),
            val("DATE"),
            val("TIME"),
            val("ARTNO"),
            val("SERNO"),
            val("FW"),
            val("HW"),
            delimited(header("CONTNO"), digit1, line_ending),
        )),
        |(_csi, date, time, artno, serno, fw, hw, contno)| CSI {
            date: String::from(date),
            time: String::from(time),
            artno: String::from(artno),
            serno: String::from(serno),
            fw: String::from(fw),
            hw: String::from(hw),
            contno: contno.parse().unwrap(),
        },
    )(i)
}

fn identifier(i: &str) -> PResult<&str> {
    recognize(many1(alt((alphanumeric1, tag("_")))))(i)
}

#[derive(Debug, Clone, PartialEq)]
pub struct List3Item {
    pub busid: String,
    pub serno: String,
    pub status: Status,
    pub artno: String,
    pub name: Option<String>,
}

pub fn lst3(i: &str) -> PResult<Vec<List3Item>> {
    let (i, _) = terminated(header("LST3"), remainder)(i)?;
    many_m_n(
        1,
        30,
        map_res(
            tuple((
                preceded(tuple((tag("LST|"), contno)), alphanumeric1),
                preceded(cc('|'), alphanumeric1),
                preceded(cc('|'), identifier),
                preceded(cc('|'), alphanumeric1),
                opt(preceded(cc('|'), not_line_ending)),
                line_ending,
            )),
            |(busid, serno, status, artno, name, _nl)| -> Result<_, Error> {
                Ok(List3Item {
                    busid: String::from(busid),
                    serno: String::from(serno),
                    status: status.parse()?,
                    artno: String::from(artno),
                    name: name
                        .filter(|s| !s.trim().is_empty())
                        .map(|n| String::from(n.trim())),
                })
            },
        ),
    )(i)
}

pub fn devstatus(i: &str) -> PResult<(String, u32)> {
    map_res(
        separated_pair(
            preceded(contno, recognize(many1(alt((alphanumeric1, tag("_")))))),
            cc('|'),
            terminated(digit1, line_ending),
        ),
        |(busid, data)| -> Result<(String, u32)> {
            Ok((String::from(busid), data.parse::<u32>()?))
        },
    )(i)
}

pub fn parse(i: &str) -> PResult<Response> {
    alt((
        map(kal, |_| Response::Keepalive),
        map(evt, |v| Response::Event(v)),
        map(dataprint, |v| Response::Dataprint(v)),
        map(csi, |v| Response::CSI(v)),
        map(lst3, |v| Response::List3(v)),
        map(devstatus, |(addr, data)| Response::Devstatus { addr, data }),
    ))(i)
}

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
LST|1_OWD2|4300001982956429|S_0|DS2408|K8\n\
LST|1_OWD4|FFFFFFFFFFFFFFFF|S_10|none|             \n\
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
                    serno: "EF000019096A4026".into(),
                    status: Online,
                    artno: "11150".into(),
                    name: None
                },
                List3Item {
                    busid: "OWD2".into(),
                    serno: "4300001982956429".into(),
                    status: Online,
                    artno: "DS2408".into(),
                    name: Some("K8".into())
                },
                List3Item {
                    busid: "OWD4".into(),
                    serno: "FFFFFFFFFFFFFFFF".into(),
                    status: Unconfigured,
                    artno: "none".into(),
                    name: None
                },
            ]
        );
    }

    #[test]
    fn parse_devstatus() {
        let (rem, mtch) = devstatus("1_OWD12_3|2\n").unwrap();
        assert!(rem.is_empty());
        assert_eq!(mtch, ("OWD12_3".into(), 2));
        let (rem, mtch) = devstatus("1_OWD14_4|10000100\n").unwrap();
        assert!(rem.is_empty());
        assert_eq!(mtch, ("OWD14_4".into(), 10000100));
        let (rem, mtch) = devstatus("2_SYS3|500\n").unwrap();
        assert!(rem.is_empty());
        assert_eq!(mtch, ("SYS3".into(), 500));
    }
}
