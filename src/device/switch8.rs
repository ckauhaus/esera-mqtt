use super::{digital_io, str2bool, Result, Token};
use crate::{Device, DeviceInfo, MqttMsg, Response, TwoWay};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Switch8 {
    info: DeviceInfo,
}

impl Switch8 {
    new!(Switch8);
}

impl Device for Switch8 {
    std_methods!(Switch8);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 3])
    }

    fn handle_1wire(&mut self, resp: Response) -> Result<TwoWay> {
        Ok(match resp {
            Response::Devstatus(s) => {
                debug!("[{}] Switch8 {} is {:b}", s.contno, s.addr, s.val);
                match s.addr.rsplit('_').nth(0).unwrap() {
                    "1" => digital_io(&self.info, 8, "in", s.val),
                    "3" => digital_io(&self.info, 8, "out", s.val),
                    other => panic!("BUG: Unknown busaddr {}", other),
                }
            }
            Response::OWDStatus(os) => self.handle_status(os.status),
            _ => {
                warn!("[{}] Switch8: no handler for {:?}", self.info.contno, resp);
                TwoWay::default()
            }
        })
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        let mut t = Vec::with_capacity(24);
        for i in 1..=8 {
            t.push((self.info.fmt(format_args!("/set/ch{}", i)), i - 1));
            t.push((self.info.fmt(format_args!("/out/ch{}", i)), -1));
            t.push((self.info.fmt(format_args!("/in/ch{}", i)), -1));
        }
        t
    }

    fn handle_mqtt(&self, msg: MqttMsg, token: Token) -> Result<TwoWay> {
        let pl = msg.payload();
        Ok(match token {
            i if i >= 0 && i < 8 => TwoWay::from_1wire(format!(
                "SET,OWD,OUT,{},{},{}",
                self.info.devno(),
                i,
                str2bool(pl) as u8
            )),
            _ => TwoWay::default(),
        })
    }
}
