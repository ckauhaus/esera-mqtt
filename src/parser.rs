use crate::DeviceInfo;

use strum_macros::{AsRefStr, Display, EnumDiscriminants, EnumString, IntoStaticStr};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum Error {
    #[error("Invalid UTF-8 in controller response")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Invalid UTF-8 in controller response")]
    FromUtf8(#[from] std::string::FromUtf8Error),
    #[error("Invalid status code")]
    Status(#[from] strum::ParseError),
    #[error("Cannot parse numeric argument")]
    ParseInt(#[from] std::num::ParseIntError),
}

type Result<T, E = Error> = std::result::Result<T, E>;
pub type PResult<'i, O> = nom::IResult<&'i str, O, nom::error::VerboseError<&'i str>>;

use nom::branch::alt;
use nom::bytes::streaming::tag;
use nom::character::streaming::{
    alphanumeric1, char as cc, digit1, line_ending, not_line_ending, one_of,
};
use nom::combinator::{map, map_res, opt, recognize};
use nom::multi::{many1, many_m_n};
use nom::sequence::{delimited, preceded, terminated, tuple};

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

pub type Keepalive = u8;

pub fn kal(i: &str) -> PResult<Keepalive> {
    terminated(header("KAL"), remainder)(i)
}

fn timeval(i: &str) -> PResult<&str> {
    recognize(many1(one_of("0123456789:")))(i)
}

pub type Info = String;

pub fn inf(i: &str) -> PResult<Info> {
    map(delimited(header("INF"), timeval, line_ending), String::from)(i)
}

/// Controller error. The number denotes the erronous command component
pub type Err = u8;

pub fn err(i: &str) -> PResult<Err> {
    map_res(delimited(header("ERR"), digit1, line_ending), |v| v.parse())(i)
}

pub type Event = String;

pub fn evt(i: &str) -> PResult<Event> {
    map(delimited(header("EVT"), timeval, line_ending), String::from)(i)
}

pub type Rst = char;

pub fn rst(i: &str) -> PResult<Rst> {
    delimited(header("RST"), one_of("01"), line_ending)(i)
}
pub type Rdy = char;

pub fn rdy(i: &str) -> PResult<Rdy> {
    delimited(header("RDY"), one_of("01"), line_ending)(i)
}
pub type Save = char;

pub fn save(i: &str) -> PResult<Save> {
    delimited(header("SAVE"), one_of("01"), line_ending)(i)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dataprint {
    pub contno: u8,
    pub flag: char,
}

pub fn dataprint(i: &str) -> PResult<Dataprint> {
    map(
        tuple((header("DATAPRINT"), terminated(one_of("01"), line_ending))),
        |(contno, flag)| Dataprint { contno, flag },
    )(i)
}

pub type Datatime = u8;

pub fn datatime(i: &str) -> PResult<Datatime> {
    map(delimited(header("DATATIME"), digit1, line_ending), |s| {
        s.parse().unwrap()
    })(i)
}

pub type Date = String;

pub fn date(i: &str) -> PResult<Date> {
    map(
        delimited(
            header("DATE"),
            recognize(many1(one_of("0123456789."))),
            line_ending,
        ),
        String::from,
    )(i)
}

pub type Time = String;

pub fn time(i: &str) -> PResult<Time> {
    map(
        delimited(header("TIME"), timeval, line_ending),
        String::from,
    )(i)
}

#[derive(Debug, Clone, Default, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, AsRefStr)]
pub enum Status {
    #[strum(serialize = "S_0", to_string = "Online")]
    Online,
    #[strum(serialize = "S_1")]
    Err1,
    #[strum(serialize = "S_2")]
    Err2,
    #[strum(serialize = "S_3")]
    Err3,
    #[strum(serialize = "S_5", to_string = "Offline")]
    Offline,
    #[strum(serialize = "S_10")]
    Unconfigured,
}

impl Default for Status {
    fn default() -> Self {
        Self::Unconfigured
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct List3 {
    pub contno: u8,
    pub items: Vec<DeviceInfo>,
}

pub fn lst3(i: &str) -> PResult<List3> {
    let (i, contno) = terminated(header("LST3"), remainder)(i)?;
    let head = format!("LST|{}_", contno);
    let (i, items) = many_m_n(
        1,
        30,
        map_res(
            tuple((
                preceded(tag(head.as_ref()), alphanumeric1),
                preceded(cc('|'), alphanumeric1),
                preceded(cc('|'), identifier),
                preceded(cc('|'), alphanumeric1),
                opt(preceded(cc('|'), not_line_ending)),
                line_ending,
            )),
            |(busid, serno, status, artno, name, _nl)| -> Result<_, Error> {
                Ok(DeviceInfo {
                    contno,
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
    )(i)?;
    Ok((i, List3 { contno, items }))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Devstatus {
    pub contno: u8,
    pub addr: String,
    pub val: u32,
}

pub fn devstatus(i: &str) -> PResult<Devstatus> {
    map_res(
        tuple((
            contno,
            recognize(many1(alt((alphanumeric1, tag("_"))))),
            cc('|'),
            terminated(digit1, line_ending),
        )),
        |(contno, busaddr, _, value)| -> Result<Devstatus> {
            Ok(Devstatus {
                contno,
                addr: busaddr.into(),
                val: value.parse()?,
            })
        },
    )(i)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, AsRefStr, IntoStaticStr)]
pub enum DIOStatus {
    #[strum(serialize = "0", to_string = "Independent+Level")]
    IndependentLevel,
    #[strum(serialize = "1", to_string = "Independent+Edge")]
    IndependentEdge,
    #[strum(serialize = "2", to_string = "Linked+Level")]
    LinkedLevel,
    #[strum(serialize = "3", to_string = "Linked+Edge")]
    LinkedEdge,
}

impl Default for DIOStatus {
    fn default() -> Self {
        DIOStatus::IndependentLevel
    }
}

impl Into<String> for DIOStatus {
    fn into(self) -> String {
        self.to_string()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DIO {
    pub contno: u8,
    pub status: DIOStatus,
}

pub fn dio(i: &str) -> PResult<DIO> {
    map_res(
        tuple((contno, delimited(tag("DIO|"), digit1, line_ending))),
        |(c, n)| -> Result<DIO> {
            Ok(DIO {
                contno: c,
                status: n.parse()?,
            })
        },
    )(i)
}

#[derive(Debug, Clone, PartialEq, EnumDiscriminants)]
#[strum_discriminants(name(ResponseKind))]
pub enum Response {
    Keepalive(Keepalive),
    Info(Info),
    Err(Err),
    Event(Event),
    Rst(Rst),
    Rdy(Rdy),
    Save(Save),
    Dataprint(Dataprint),
    Datatime(Datatime),
    Date(Date),
    Time(Time),
    List3(List3),
    CSI(CSI),
    DIO(DIO),
    Devstatus(Devstatus),
}

pub fn parse(i: &str) -> PResult<Response> {
    alt((
        map(kal, |v| Response::Keepalive(v)),
        map(inf, |v| Response::Info(v)),
        map(err, |v| Response::Err(v)),
        map(evt, |v| Response::Event(v)),
        map(rst, |v| Response::Rst(v)),
        map(rdy, |v| Response::Rdy(v)),
        map(save, |v| Response::Save(v)),
        map(dataprint, |v| Response::Dataprint(v)),
        map(datatime, |v| Response::Datatime(v)),
        map(date, |v| Response::Date(v)),
        map(time, |v| Response::Time(v)),
        map(lst3, |v| Response::List3(v)),
        map(csi, |v| Response::CSI(v)),
        map(dio, |v| Response::DIO(v)),
        map(devstatus, |v| Response::Devstatus(v)),
    ))(i)
}

#[cfg(test)]
mod test {
    use super::Status::*;
    use super::*;
    use assert_matches::assert_matches;
    use pretty_assertions::assert_eq;

    #[test]
    fn parse_keepalive() {
        assert_eq!(kal("1_KAL|1\n").unwrap(), ("", 1));
    }

    #[test]
    fn parse_incomplete() {
        assert_matches!(kal("1_KAL|1").unwrap_err(), nom::Err::Incomplete(_));
    }

    #[test]
    fn parse_dataprint() {
        assert_eq!(
            dataprint("1_DATAPRINT|1\n").unwrap(),
            (
                "",
                Dataprint {
                    contno: 1,
                    flag: '1'
                }
            )
        );
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
            List3 {
                contno: 1,
                items: vec![
                    DeviceInfo {
                        contno: 1,
                        busid: "OWD1".into(),
                        serno: "EF000019096A4026".into(),
                        status: Online,
                        artno: "11150".into(),
                        name: None
                    },
                    DeviceInfo {
                        contno: 1,
                        busid: "OWD2".into(),
                        serno: "4300001982956429".into(),
                        status: Online,
                        artno: "DS2408".into(),
                        name: Some("K8".into())
                    },
                    DeviceInfo {
                        contno: 1,
                        busid: "OWD4".into(),
                        serno: "FFFFFFFFFFFFFFFF".into(),
                        status: Unconfigured,
                        artno: "none".into(),
                        name: None
                    },
                ]
            }
        );
    }

    #[test]
    fn parse_devstatus() {
        let (rem, mtch) = devstatus("1_OWD12_3|2\n").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            mtch,
            Devstatus {
                contno: 1,
                addr: "OWD12_3".into(),
                val: 2
            }
        );
        let (rem, mtch) = devstatus("1_OWD14_4|10000100\n").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            mtch,
            Devstatus {
                contno: 1,
                addr: "OWD14_4".into(),
                val: 10000100
            }
        );
        let (rem, mtch) = devstatus("2_SYS3|500\n").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            mtch,
            Devstatus {
                contno: 2,
                addr: "SYS3".into(),
                val: 500
            }
        );
    }

    #[test]
    fn parse_dio() {
        let (rem, mtch) = dio("3_DIO|1\n").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            mtch,
            DIO {
                contno: 3,
                status: DIOStatus::IndependentEdge
            }
        );
        let (_, mtch) = parse("1_DIO|0\n").unwrap();
        assert_matches!(mtch, Response::DIO(_));
    }
}
