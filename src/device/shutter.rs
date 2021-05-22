use super::{digital_io, Device, DeviceInfo, MqttMsg, Result, Token, TwoWay};
use crate::parser::{Msg, OW};

use serde_json::json;
use std::time::Instant;

const DEF_TIME: f32 = 60.0;

#[derive(Debug, Eq, PartialEq, Clone, Copy, strum_macros::IntoStaticStr, strum_macros::Display)]
enum Direction {
    #[strum(serialize = "STOP")]
    Stop = 0,
    #[strum(serialize = "CLOSE")]
    Close = 1,
    #[strum(serialize = "OPEN")]
    Open = 2,
}

use Direction::*;

impl Default for Direction {
    fn default() -> Self {
        Stop
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shutter {
    info: DeviceInfo,
    buttons: i32,
    direction: Direction,
    start: Option<Instant>,
    initial_pos: f32,
    position: f32,
}

fn clamp(val: f32, min: f32, max: f32) -> f32 {
    f32::max(f32::min(val, max), min)
}

impl Shutter {
    pub fn new(info: DeviceInfo) -> Self {
        Self {
            info,
            position: 100.0,
            initial_pos: 100.0,
            ..Default::default()
        }
    }

    fn time_to(&self, what: Direction) -> f32 {
        match std::env::var(format!(
            "SHUTTER_{}_{}_{}_TIME",
            self.info.contno,
            self.name(),
            what
        )) {
            Ok(v) => v.trim().parse::<f32>().unwrap_or(DEF_TIME),
            Err(_) => DEF_TIME,
        }
    }

    fn calc(&mut self) {
        match self.direction {
            Close => {
                self.position = self.initial_pos
                    - clamp(
                        (Instant::now() - self.start.unwrap()).as_secs_f32() - 1.0,
                        0.0,
                        self.time_to(Close),
                    ) * 100.0
                        / self.time_to(Close)
            }
            Open => {
                self.position = self.initial_pos
                    + clamp(
                        (Instant::now() - self.start.unwrap()).as_secs_f32() - 1.0,
                        0.0,
                        self.time_to(Open),
                    ) * 100.0
                        / self.time_to(Open)
            }
            _ => (),
        }
        self.position = clamp(self.position, 0.0, 100.0);
    }

    fn stop(&mut self) {
        self.calc();
        debug!(
            "[{}] Shutter {} stopping at {}",
            self.info.contno,
            self.name(),
            self.position
        );
        self.direction = Stop;
        self.start = None;
        self.initial_pos = self.position;
    }

    fn close(&mut self) {
        debug!("[{}] Shutter {} closing", self.info.contno, self.name());
        self.direction = Close;
        self.start = Some(Instant::now());
    }

    fn open(&mut self) {
        debug!("[{}] Shutter {} opening", self.info.contno, self.name());
        self.direction = Open;
        self.start = Some(Instant::now());
    }

    fn state(&self) -> &'static str {
        match (self.direction, self.position.round() as i32) {
            (Stop, 100) => "open",
            (Stop, 0) => "closed",
            (Stop, _) => "stopped",
            (Close, _) => "closing",
            (Open, _) => "opening",
        }
    }
}

impl Device for Shutter {
    std_methods!(Shutter);

    fn register_1wire(&self) -> Vec<String> {
        self.info.mkbusaddrs(&[1, 3])
    }

    fn handle_1wire(&mut self, ow: OW) -> Result<TwoWay> {
        Ok(match ow.msg {
            Msg::Devstatus(s) => match s.subaddr() {
                Some(1) => {
                    debug!(
                        "[{}] Shutter {} buttons={:02b}",
                        ow.contno,
                        self.name(),
                        s.val
                    );
                    let res = digital_io(&self.info, 2, "in", s.val, None)
                        + digital_io(&self.info, 2, "button", s.val, Some(self.buttons));
                    self.buttons = s.val;
                    res
                }
                Some(3) => {
                    debug!(
                        "[{}] Shutter {} state={:02b}",
                        ow.contno,
                        self.name(),
                        s.val
                    );
                    let mut res = TwoWay::default();
                    match s.val & 0b11 {
                        0b01 if self.direction != Close => self.close(),
                        0b10 if self.direction != Open => self.open(),
                        0b11 => self.stop(),
                        _ => {
                            self.calc();
                            if let Some(start) = self.start {
                                match self.direction {
                                    Close
                                        if (Instant::now() - start).as_secs_f32()
                                            > self.time_to(Close) =>
                                    {
                                        res +=
                                            self.handle_mqtt(&MqttMsg::new("", "STOP"), 0).unwrap()
                                    }
                                    Open if (Instant::now() - start).as_secs_f32()
                                        > self.time_to(Open) =>
                                    {
                                        res +=
                                            self.handle_mqtt(&MqttMsg::new("", "STOP"), 0).unwrap()
                                    }
                                    _ => (),
                                }
                            }
                        }
                    }
                    res += TwoWay::new(
                        vec![
                            self.info
                                .mqtt_msg("position", format!("{:1.0}", self.position.round())),
                            self.info.mqtt_msg("state", self.state()),
                        ],
                        vec![],
                    );
                    res
                }
                _ => panic!("BUG: Unknown busaddr {}", s.addr),
            },
            _ => {
                warn!(
                    "[{}] Shutter {}: no handler for {:?}",
                    ow.contno,
                    self.name(),
                    ow
                );
                TwoWay::default()
            }
        })
    }

    /// Channel 1: down/close
    /// Channel 2: up/open
    fn announce(&self) -> Vec<MqttMsg> {
        let mut res = Vec::with_capacity(10);
        let dev = self.announce_device();
        let i = &self.info;
        for button in &[1, 2] {
            res.push(self.announce_trigger(&dev, *button, "short", "0"));
            res.push(self.announce_trigger(&dev, *button, "short", "1"));
        }
        res.push(MqttMsg::retain(
            format!(
                "homeassistant/cover/{}/{}/config",
                i.contno,
                i.serno.replace(
                    |c: char| { !c.is_ascii_alphanumeric() && c != '_' && c != '-' },
                    ""
                )
            ),
            serde_json::to_string(&json!({
                "availability_topic": i.status_topic(),
                "command_topic": i.topic("set"),
                "device": dev,
                "name": format!("Shutter {}/{}", i.contno, self.name()),
                "state_topic": i.topic("state"),
                "unique_id": i.serno,
                "optimistic": false,
                "position_topic": i.topic("position"),
                "state_opening": "opening",
                "state_open": "open",
                "state_closing": "closing",
                "state_closed": "closed",
                "state_stopped": "stopped",
            }))
            .unwrap(),
        ));
        res
    }

    fn register_mqtt(&self) -> Vec<(String, Token)> {
        vec![(self.info.topic("set"), 0)]
    }

    fn handle_mqtt(&self, msg: &MqttMsg, _: Token) -> Result<TwoWay> {
        let pl = msg.payload();
        debug!(
            "[{}] Shutter {}: MQTT: set {}",
            self.info.contno,
            self.name(),
            pl
        );
        let d = self.info.devno();
        Ok(match pl {
            "CLOSE" => TwoWay::from_1wire(format!("SET,OWD,SHT,{},1", d)),
            "OPEN" => TwoWay::from_1wire(format!("SET,OWD,SHT,{},2", d)),
            "STOP" => TwoWay::from_1wire(format!("SET,OWD,SHT,{},3", d)),
            _ => {
                error!(
                    "[{}] Shutter {}: unrecognized MQTT command {}",
                    self.info.contno,
                    self.name(),
                    pl
                );
                TwoWay::default()
            }
        })
    }
}
