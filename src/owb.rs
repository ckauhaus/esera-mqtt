use anyhow::{anyhow, Result};
use chrono::naive::{NaiveDate, NaiveTime};
use smallstr::SmallString;

use crate::device::{Article, BusID, DevInfo};

pub type SStr = SmallString<[u8; 15]>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evt {
    pub busid: BusID,
    pub sub: u8,
    pub msg: SStr,
}

impl Evt {
    pub fn new<S: Into<BusID>>(busid: S, sub: u8, msg: SStr) -> Self {
        Evt {
            busid: busid.into(),
            sub,
            msg,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resp {
    Event(Evt),
    ERR(SStr),
    Info(DevInfo),
    KAL(SStr),
    Timed(SStr, NaiveTime),
    Date(NaiveDate),
    Artno(SStr),
    Serno(SStr),
    Fw(SStr),
    Hw(SStr),
    Dataprint(u8),
    Other(SStr, String),
}

impl Resp {
    pub fn parse(contno: u8, s: &str) -> Result<Self> {
        match parser::line(contno, s) {
            Ok((rest, resp)) => {
                if rest.is_empty() {
                    Ok(resp)
                } else {
                    Err(anyhow!("Incompletely parsed line: {}", rest))
                }
            }
            Err(e) => Err(anyhow!("Parse error: {}", e)),
        }
    }
}

mod parser {
    use super::Resp::*;
    use super::*;

    use nom::branch::alt;
    use nom::bytes::complete::{tag, take_while, take_while1};
    use nom::character::complete::{
        alphanumeric1, digit1, hex_digit1, line_ending, not_line_ending,
    };
    use nom::combinator::{map, map_res};
    use nom::sequence::{pair, preceded, separated_pair, terminated};
    use nom::IResult;

    fn kal(s: &str) -> IResult<&str, Resp> {
        let (s, msg) = preceded(tag("KAL|"), not_line_ending)(s)?;
        Ok((s, KAL(msg.into())))
    }

    fn err(s: &str) -> IResult<&str, Resp> {
        let (s, msg) = preceded(tag("ERR|"), not_line_ending)(s)?;
        Ok((s, ERR(msg.into())))
    }

    fn sys3(s: &str) -> IResult<&str, Resp> {
        let (s, val) = preceded(tag("SYS3|"), digit1)(s)?;
        Ok((s, Event(Evt::new("SYS3", 0, val.into()))))
    }

    fn dev(s: &str) -> IResult<&str, Resp> {
        let (s, dev) = alt((tag("OWD"), tag("SYS")))(s)?;
        let (s, id) = digit1(s)?;
        let (s, addr) = map_res(preceded(tag("_"), digit1), str::parse)(s)?;
        let (s, msg) = preceded(tag("|"), not_line_ending)(s)?;
        let mut busid = BusID::from(dev);
        busid.push_str(id);
        Ok((s, Event(Evt::new(busid, addr, msg.into()))))
    }

    fn time(s: &str) -> IResult<&str, NaiveTime> {
        map_res(take_while(|c: char| c.is_ascii_digit() || c == ':'), |t| {
            NaiveTime::parse_from_str(t, "%H:%M:%S")
        })(s)
    }

    fn timed(s: &str) -> IResult<&str, Resp> {
        let (s, (tag, time)) = pair(
            terminated(
                alt((tag("INF"), tag("EVT"), tag("LST3"), tag("CSI"), tag("TIME"))),
                tag("|"),
            ),
            time,
        )(s)?;
        Ok((s, Timed(tag.into(), time)))
    }

    fn date_(s: &str) -> IResult<&str, NaiveDate> {
        map_res(take_while(|c: char| c.is_ascii_digit() || c == '.'), |t| {
            NaiveDate::parse_from_str(t, "%d.%m.%y")
        })(s)
    }

    fn date(s: &str) -> IResult<&str, Resp> {
        map(preceded(tag("DATE|"), date_), Date)(s)
    }

    fn artno(s: &str) -> IResult<&str, Resp> {
        let (s, artno) = preceded(tag("ARTNO|"), not_line_ending)(s)?;
        Ok((s, Artno(artno.into())))
    }

    fn serno(s: &str) -> IResult<&str, Resp> {
        let (s, serno) = preceded(tag("SERNO|"), not_line_ending)(s)?;
        Ok((s, Serno(serno.into())))
    }

    fn fw(s: &str) -> IResult<&str, Resp> {
        let (s, fw) = preceded(tag("FW|"), not_line_ending)(s)?;
        Ok((s, Fw(fw.into())))
    }

    fn hw(s: &str) -> IResult<&str, Resp> {
        let (s, hw) = preceded(tag("HW|"), not_line_ending)(s)?;
        Ok((s, Hw(hw.into())))
    }

    fn dataprint(s: &str) -> IResult<&str, Resp> {
        let (s, i) = map_res(preceded(tag("DATAPRINT|"), digit1), str::parse)(s)?;
        Ok((s, Dataprint(i)))
    }

    fn other(s: &str) -> IResult<&str, Resp> {
        let (s, (name, msg)) =
            separated_pair(take_while1(|c| c != '|'), tag("|"), not_line_ending)(s)?;
        Ok((s, Other(name.into(), msg.into())))
    }

    fn regular(contno: u8, s: &str) -> IResult<&str, Resp> {
        let (s, _) = tag(contno.to_string().as_str())(s)?;
        let (s, _) = tag("_")(s)?;
        let (s, resp) = alt((
            kal, err, sys3, dev, timed, date, artno, serno, fw, hw, dataprint, other,
        ))(s)?;
        let (s, _) = line_ending(s)?;
        Ok((s, resp))
    }

    fn listall1(contno: u8, s: &str) -> IResult<&str, Resp> {
        let (s, _) = tag("LST|")(s)?;
        let (s, _) = pair(tag(contno.to_string().as_str()), tag("_"))(s)?;
        let (s, dev) = tag("OWD")(s)?;
        let (s, id) = digit1(s)?;
        let (s, serial) = preceded(tag("|"), hex_digit1)(s)?;
        let (s, err) = map_res(preceded(tag("|S_"), digit1), str::parse)(s)?;
        let (s, art) = preceded(tag("|"), alphanumeric1)(s)?;
        let (s, name) = preceded(tag("|"), not_line_ending)(s)?;
        let (s, _) = line_ending(s)?;
        let serial = match serial {
            "FFFFFFFFFFFFFFFF" => "",
            s => s,
        }
        .into();
        let mut busid = BusID::from(dev);
        busid.push_str(id);
        Ok((
            s,
            Info(DevInfo::new(
                busid,
                serial,
                err,
                Article::from(art),
                name.trim(),
            )),
        ))
    }

    pub fn line(contno: u8, s: &str) -> IResult<&str, Resp> {
        let reg = |s| regular(contno, s);
        let lst = |s| listall1(contno, s);
        alt((reg, lst))(s)
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        fn parse_timed() {
            assert_eq!(
                line(1, "1_INF|16:07:01\r\n").unwrap(),
                ("", Timed("INF".into(), NaiveTime::from_hms(16, 7, 1)))
            );
            assert_eq!(
                line(1, "1_EVT|17:02:11\r\n").unwrap(),
                ("", Timed("EVT".into(), NaiveTime::from_hms(17, 2, 11)))
            );
        }

        #[test]
        fn parse_err() {
            assert_eq!(line(2, "2_ERR|3\r\n").unwrap(), ("", ERR("3".into())));
        }

        #[test]
        fn parse_run() {
            assert_eq!(
                line(1, "1_RUN|0\r\n").unwrap(),
                ("", Other("RUN".into(), "0".into()))
            );
        }

        #[test]
        fn parse_wrong_contno() {
            assert!(line(2, "1_INF|15:49:23\r\n").is_err());
        }

        #[test]
        fn parse_dev_sys() {
            assert_eq!(
                line(1, "1_SYS1_1|6\r\n").unwrap(),
                ("", Event(Evt::new("SYS1", 1, "6".into())))
            );
            assert_eq!(
                line(1, "1_SYS1_2|00000110\r\n").unwrap(),
                ("", Event(Evt::new("SYS1", 2, "00000110".into())))
            );
            assert_eq!(
                line(1, "1_SYS3|0\r\n").unwrap(),
                ("", Event(Evt::new("SYS3", 0, "0".into())))
            );
        }

        #[test]
        fn parse_dev_owd() {
            assert_eq!(
                line(1, "1_OWD2_3|130\r\n").unwrap(),
                ("", Event(Evt::new("OWD2", 3, "130".into())))
            );
            assert_eq!(
                line(1, "1_OWD2_4|10000010\r\n").unwrap(),
                ("", Event(Evt::new("OWD2", 4, "10000010".into())))
            );
        }

        #[test]
        fn parse_listall1() {
            assert_eq!(
                line(1, "1_LST3|16:21:02\r\n").unwrap(),
                ("", Timed("LST3".into(), NaiveTime::from_hms(16, 21, 2)))
            );
            assert_eq!(
                line(1, "LST|1_OWD1|EF000019096A4026|S_0|11150|TEMP_WZ\r\n").unwrap(),
                (
                    "",
                    Info(DevInfo::new(
                        "OWD1",
                        "EF000019096A4026",
                        0,
                        Article::TempHum,
                        "TEMP_WZ"
                    ))
                )
            );
            assert_eq!(
                line(1, "LST|1_OWD3|FFFFFFFFFFFFFFFF|S_10|none|             \r\n").unwrap(),
                ("", Info(DevInfo::new("OWD3", "", 10, Article::Unknown, "")))
            );
        }
    }
}
