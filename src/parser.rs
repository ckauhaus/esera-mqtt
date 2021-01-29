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

#[derive(Debug, Clone, PartialEq)]
pub struct OW {
    pub contno: u8,
    pub msg: Msg,
}

#[derive(Debug, Clone, PartialEq, EnumDiscriminants)]
#[strum_discriminants(name(MsgKind))]
pub enum Msg {
    Keepalive(Keepalive),
    Inf(Inf),
    Err(Err),
    Evt(Evt),
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
    OWDStatus(OWDStatus),
    Devstatus(Devstatus),
}

use nom::branch::alt;
use nom::bytes::streaming::tag;
use nom::character::streaming::{
    alphanumeric1, char as cc, digit1, line_ending, not_line_ending, one_of,
};
use nom::combinator::{map, map_res, opt, recognize};
use nom::multi::{many1, many_m_n};
use nom::sequence::{delimited, pair, preceded, terminated, tuple};

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

fn timeval(i: &str) -> PResult<&str> {
    recognize(many1(one_of("0123456789:")))(i)
}

pub type Keepalive = char;

pub fn kal(i: &str) -> PResult<OW> {
    map(
        tuple((header("KAL"), terminated(one_of("01"), line_ending))),
        |(contno, flag)| OW {
            contno,
            msg: Msg::Keepalive(flag),
        },
    )(i)
}

pub type Inf = String;

pub fn inf(i: &str) -> PResult<OW> {
    map(
        tuple((header("INF"), terminated(timeval, line_ending))),
        |(contno, dt)| OW {
            contno,
            msg: Msg::Inf(Inf::from(dt)),
        },
    )(i)
}

/// Controller error. The number denotes the erronous command component
pub type Err = u16;

pub fn err(i: &str) -> PResult<OW> {
    map(
        tuple((header("ERR"), terminated(digit1, line_ending))),
        |(contno, v)| OW {
            contno,
            msg: Msg::Err(v.parse().expect("16-bit integer")),
        },
    )(i)
}

pub type Evt = String;

pub fn evt(i: &str) -> PResult<OW> {
    map(
        tuple((header("EVT"), terminated(timeval, line_ending))),
        |(contno, dt)| OW {
            contno,
            msg: Msg::Evt(Evt::from(dt)),
        },
    )(i)
}

pub type Rst = char;

pub fn rst(i: &str) -> PResult<OW> {
    map(
        tuple((header("RST"), terminated(one_of("01"), line_ending))),
        |(contno, flag)| OW {
            contno,
            msg: Msg::Rst(flag),
        },
    )(i)
}

pub type Rdy = char;

pub fn rdy(i: &str) -> PResult<OW> {
    map(
        tuple((header("RDY"), terminated(one_of("01"), line_ending))),
        |(contno, flag)| OW {
            contno,
            msg: Msg::Rdy(flag),
        },
    )(i)
}
pub type Save = char;

pub fn save(i: &str) -> PResult<OW> {
    map(
        tuple((header("SAVE"), terminated(one_of("01"), line_ending))),
        |(contno, flag)| OW {
            contno,
            msg: Msg::Save(flag),
        },
    )(i)
}

pub type Dataprint = char;

pub fn dataprint(i: &str) -> PResult<OW> {
    map(
        tuple((header("DATAPRINT"), terminated(one_of("01"), line_ending))),
        |(contno, flag)| OW {
            contno,
            msg: Msg::Dataprint(flag),
        },
    )(i)
}

pub type Datatime = u8;

pub fn datatime(i: &str) -> PResult<OW> {
    map(
        tuple((header("DATATIME"), terminated(digit1, line_ending))),
        |(contno, s)| OW {
            contno,
            msg: Msg::Datatime(s.parse().expect("8-bit integer")),
        },
    )(i)
}

pub type Date = String;

pub fn date(i: &str) -> PResult<OW> {
    map(
        tuple((
            header("DATE"),
            terminated(recognize(many1(one_of("0123456789."))), line_ending),
        )),
        |(contno, d)| OW {
            contno,
            msg: Msg::Date(Date::from(d)),
        },
    )(i)
}

pub type Time = String;

pub fn time(i: &str) -> PResult<OW> {
    map(
        tuple((header("TIME"), terminated(timeval, line_ending))),
        |(contno, t)| OW {
            contno,
            msg: Msg::Time(Time::from(t)),
        },
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
}

pub fn csi(i: &str) -> PResult<OW> {
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
        |(_csi, date, time, artno, serno, fw, hw, contno)| OW {
            contno: contno.parse().expect("8-bit integer"),
            msg: Msg::CSI(CSI {
                date: String::from(date),
                time: String::from(time),
                artno: String::from(artno),
                serno: String::from(serno),
                fw: String::from(fw),
                hw: String::from(hw),
            }),
        },
    )(i)
}

fn identifier(i: &str) -> PResult<&str> {
    recognize(many1(alt((alphanumeric1, tag("_")))))(i)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, AsRefStr)]
pub enum Status {
    #[strum(serialize = "0", to_string = "online")]
    Online,
    #[strum(serialize = "1", to_string = "error_1")]
    Err1,
    #[strum(serialize = "2", to_string = "error_2")]
    Err2,
    #[strum(serialize = "3", to_string = "error_3")]
    Err3,
    #[strum(serialize = "5", to_string = "offline")]
    Offline,
    #[strum(serialize = "10", to_string = "unconfigured")]
    Unconfigured,
}

impl Default for Status {
    fn default() -> Self {
        Self::Unconfigured
    }
}

pub type List3 = Vec<DeviceInfo>;

pub fn lst3(i: &str) -> PResult<OW> {
    let (i, contno) = terminated(header("LST3"), remainder)(i)?;
    let head = format!("LST|{}_", contno);
    let (i, items) = many_m_n(
        1,
        30,
        map_res(
            tuple((
                preceded(tag(head.as_ref()), alphanumeric1),
                preceded(cc('|'), alphanumeric1),
                preceded(tag("|S_"), identifier),
                preceded(cc('|'), alphanumeric1),
                opt(preceded(cc('|'), not_line_ending)),
                line_ending,
            )),
            |(busid, serno, status, artno, name, _nl)| -> Result<_> {
                Ok(DeviceInfo {
                    busid: String::from(busid),
                    serno: String::from(serno),
                    status: status.parse()?,
                    artno: String::from(artno),
                    name: name
                        .filter(|s| !s.trim().is_empty())
                        .map(|n| String::from(n.trim())),
                    contno,
                })
            },
        ),
    )(i)?;
    Ok((
        i,
        OW {
            contno,
            msg: Msg::List3(items),
        },
    ))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Devstatus {
    pub addr: String,
    pub val: i32,
}

pub fn devstatus(i: &str) -> PResult<OW> {
    map_res(
        tuple((
            contno,
            recognize(many1(alt((alphanumeric1, tag("_"))))),
            cc('|'),
            terminated(pair(opt(cc('-')), digit1), line_ending),
        )),
        |(contno, busaddr, _, (sign, value))| -> Result<_> {
            let val: i32 = value.parse()?;
            Ok(OW {
                contno,
                msg: Msg::Devstatus(Devstatus {
                    addr: busaddr.into(),
                    val: if sign.is_some() { -val } else { val },
                }),
            })
        },
    )(i)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, AsRefStr, IntoStaticStr)]
pub enum DIO {
    #[strum(serialize = "0", to_string = "Independent+Level")]
    IndependentLevel,
    #[strum(serialize = "1", to_string = "Independent+Edge")]
    IndependentEdge,
    #[strum(serialize = "2", to_string = "Linked+Level")]
    LinkedLevel,
    #[strum(serialize = "3", to_string = "Linked+Edge")]
    LinkedEdge,
}

impl Default for DIO {
    fn default() -> Self {
        DIO::IndependentLevel
    }
}

impl Into<String> for DIO {
    fn into(self) -> String {
        self.to_string()
    }
}

pub fn dio(i: &str) -> PResult<OW> {
    map_res(
        tuple((contno, delimited(tag("DIO|"), digit1, line_ending))),
        |(contno, n)| -> Result<_> {
            Ok(OW {
                contno,
                msg: Msg::DIO(n.parse()?),
            })
        },
    )(i)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OWDStatus {
    pub owd: u8,
    pub status: Status,
}

pub fn owdstatus(i: &str) -> PResult<OW> {
    map_res(
        tuple((
            contno,
            delimited(tag("OWD_"), digit1, cc('|')),
            terminated(digit1, line_ending),
        )),
        |(contno, n, s)| -> Result<_> {
            Ok(OW {
                contno,
                msg: Msg::OWDStatus(OWDStatus {
                    owd: n.parse()?,
                    status: s.parse()?,
                }),
            })
        },
    )(i)
}

pub fn parse(i: &str) -> PResult<OW> {
    alt((
        kal, inf, err, evt, rst, rdy, save, dataprint, datatime, date, time, lst3, csi, dio,
        owdstatus, devstatus,
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
        assert_matches!(
            kal("1_KAL|1\n").unwrap(),
            (
                "",
                OW {
                    msg: Msg::Keepalive('1'),
                    contno: 1
                },
            )
        );
    }

    #[test]
    fn parse_incomplete() {
        assert_matches!(kal("1_KAL|1").unwrap_err(), nom::Err::Incomplete(_));
    }

    #[test]
    fn parse_dataprint() {
        assert_eq!(
            dataprint("1_DATAPRINT|1\n").unwrap().1.msg,
            Msg::Dataprint('1')
        );
    }

    #[test]
    fn parse_date() {
        assert_eq!(
            date("2_DATE|03.11.20\n").unwrap().1.msg,
            Msg::Date("03.11.20".to_owned())
        );
    }

    #[test]
    fn parse_time() {
        assert_eq!(
            time("3_TIME|0:00:52\n").unwrap().1.msg,
            Msg::Time("0:00:52".to_owned())
        );
    }

    #[test]
    fn incomplete_list() {
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
            OW {
                contno: 1,
                msg: Msg::List3(vec![
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
                ])
            }
        );
    }

    #[test]
    fn parse_devstatus_numeric() {
        assert_eq!(
            devstatus("1_OWD12_3|2\n").unwrap(),
            (
                "",
                OW {
                    contno: 1,
                    msg: Msg::Devstatus(Devstatus {
                        addr: "OWD12_3".into(),
                        val: 2
                    })
                }
            )
        );
    }

    #[test]
    fn parse_devstatus_sys() {
        assert_eq!(
            devstatus("2_SYS3|500\n").unwrap().1.msg,
            Msg::Devstatus(Devstatus {
                addr: "SYS3".into(),
                val: 500
            })
        );
    }

    #[test]
    fn parse_devstatus_neg() {
        assert_eq!(
            devstatus("3_OWD16_1|-847\n").unwrap().1.msg,
            Msg::Devstatus(Devstatus {
                addr: "OWD16_1".into(),
                val: -847
            })
        );
    }

    #[test]
    fn parse_dio() {
        assert_eq!(
            dio("3_DIO|1\n").unwrap().1,
            OW {
                contno: 3,
                msg: Msg::DIO(DIO::IndependentEdge)
            }
        );
    }

    #[test]
    fn parse_status() {
        assert_eq!(
            parse("4_OWD_2|5\n").unwrap().1,
            OW {
                contno: 4,
                msg: Msg::OWDStatus(OWDStatus {
                    owd: 2,
                    status: Status::Offline
                })
            }
        )
    }
}
