use anyhow::{anyhow, Result};
use chrono::naive::NaiveTime;
use smallstr::SmallString;

pub type SStr = SmallString<[u8; 15]>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Article {
    TempHum,
    Switch8,
    Controller2,
    Unknown,
    Other(SmallString<[u8; 16]>),
}

impl From<&str> for Article {
    fn from(s: &str) -> Self {
        match s {
            "11150" => Article::TempHum,
            "11229" => Article::Switch8,
            "11340" => Article::Controller2,
            "none" => Article::Unknown,
            other => Article::Other(other.into()),
        }
    }
}

impl Default for Article {
    fn default() -> Self {
        Article::Unknown
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevInfo {
    pub n: u8,
    pub serial: SmallString<[u8; 16]>,
    pub err: u32,
    pub art: Article,
    pub name: SmallString<[u8; 20]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevKind {
    SYS,
    OWD,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Msg {
    pub kind: DevKind,
    pub dev: u8,
    pub addr: u8,
    pub msg: SStr,
}

impl Msg {
    pub fn sys(dev: u8, addr: u8, msg: SStr) -> Self {
        Self {
            kind: DevKind::SYS,
            dev,
            addr,
            msg,
        }
    }

    pub fn owd(dev: u8, addr: u8, msg: SStr) -> Self {
        Self {
            kind: DevKind::OWD,
            dev,
            addr,
            msg,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resp {
    KAL(SStr),
    ERR(SStr),
    Timed(SStr, NaiveTime),
    // INF(NaiveTime),
    // EVT(NaiveTime),
    // LST3(NaiveTime),
    Dev(Msg),
    Info(DevInfo),
    Other(SStr, String),
}

impl Resp {
    pub fn parse(contno: u8, s: &str) -> Result<(String, Self)> {
        match parser::line(contno, s) {
            Ok((rest, resp)) => Ok((rest.to_owned(), resp)),
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
    use nom::combinator::map_res;
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

    fn time(s: &str) -> IResult<&str, NaiveTime> {
        map_res(take_while(|c: char| c.is_ascii_digit() || c == ':'), |t| {
            NaiveTime::parse_from_str(t, "%H:%M:%S")
        })(s)
    }

    fn timed(s: &str) -> IResult<&str, Resp> {
        let (s, (tag, time)) = pair(
            terminated(alt((tag("INF"), tag("EVT"), tag("LST3"))), tag("|")),
            time,
        )(s)?;
        Ok((s, Timed(tag.into(), time)))
    }

    fn sys(s: &str) -> IResult<&str, Resp> {
        let (s, dev) = map_res(preceded(tag("SYS"), digit1), str::parse)(s)?;
        let (s, addr) = map_res(preceded(tag("_"), digit1), str::parse)(s)?;
        let (s, msg) = preceded(tag("|"), not_line_ending)(s)?;
        Ok((s, Dev(Msg::sys(dev, addr, msg.into()))))
    }

    fn sys3(s: &str) -> IResult<&str, Resp> {
        let (s, val) = preceded(tag("SYS3|"), digit1)(s)?;
        Ok((s, Dev(Msg::sys(3, 0, val.into()))))
    }

    fn owd(s: &str) -> IResult<&str, Resp> {
        let (s, dev) = map_res(preceded(tag("OWD"), digit1), str::parse)(s)?;
        let (s, addr) = map_res(preceded(tag("_"), digit1), str::parse)(s)?;
        let (s, msg) = preceded(tag("|"), not_line_ending)(s)?;
        Ok((s, Dev(Msg::owd(dev, addr, msg.into()))))
    }

    fn other(s: &str) -> IResult<&str, Resp> {
        let (s, (name, msg)) =
            separated_pair(take_while1(|c| c != '|'), tag("|"), not_line_ending)(s)?;
        Ok((s, Other(name.into(), msg.into())))
    }

    fn regular(contno: u8, s: &str) -> IResult<&str, Resp> {
        let (s, _) = tag(contno.to_string().as_str())(s)?;
        let (s, _) = tag("_")(s)?;
        let (s, resp) = alt((kal, err, sys3, sys, owd, timed, other))(s)?;
        let (s, _) = line_ending(s)?;
        Ok((s, resp))
    }

    fn listall1(contno: u8, s: &str) -> IResult<&str, Resp> {
        let (s, _) = tag("LST|")(s)?;
        let (s, _) = tag(contno.to_string().as_str())(s)?;
        let (s, n) = map_res(preceded(tag("_OWD"), digit1), str::parse)(s)?;
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
        Ok((
            s,
            Info(DevInfo {
                n,
                serial,
                err,
                art: Article::from(art),
                name: name.trim().into(),
            }),
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
                ("", Dev(Msg::sys(1, 1, "6".into())))
            );
            assert_eq!(
                line(1, "1_SYS1_2|00000110\r\n").unwrap(),
                ("", Dev(Msg::sys(1, 2, "00000110".into())))
            );
            assert_eq!(
                line(1, "1_SYS3|0\r\n").unwrap(),
                ("", Dev(Msg::sys(3, 0, "0".into())))
            );
        }

        #[test]
        fn parse_dev_owd() {
            assert_eq!(
                line(1, "1_OWD2_3|130\r\n").unwrap(),
                ("", Dev(Msg::owd(2, 3, "130".into())))
            );
            assert_eq!(
                line(1, "1_OWD2_4|10000010\r\n").unwrap(),
                ("", Dev(Msg::owd(2, 4, "10000010".into())))
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
                    Info(DevInfo {
                        n: 1,
                        serial: "EF000019096A4026".into(),
                        err: 0,
                        art: Article::TempHum,
                        name: "TEMP_WZ".into()
                    })
                )
            );
            assert_eq!(
                line(1, "LST|1_OWD3|FFFFFFFFFFFFFFFF|S_10|none|             \r\n").unwrap(),
                (
                    "",
                    Info(DevInfo {
                        n: 3,
                        serial: "".into(),
                        err: 10,
                        art: Article::Unknown,
                        name: "".into()
                    })
                )
            );
        }
    }
}
