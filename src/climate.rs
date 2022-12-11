///! HVAC climate controller
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use slog::{debug, error, info, o, Logger};
use strum_macros::EnumString;
use strum_macros::IntoStaticStr;
use thiserror::Error;

use crate::{bool2str, str2bool, AnnounceDevice, MqttMsg, Token};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid numeric format: {0}: {1}")]
    FloatFormat(String, #[source] std::num::ParseFloatError),
    #[error("Cannot understand mode {0}")]
    Keyword(String),
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub static BASE: &str = "homeassistant/climate/virt";
const INITIAL_TEMP: f32 = 21.0;
const EPSILON_TEMP: f32 = 0.02;
const AUX_HEAT_TRIGGER: f32 = 0.8; // offset in °C

const TOK_HEAT_STATE: Token = 1;
const TOK_TEMP: Token = 2;
const TOK_MODE_SET: Token = 3;
const TOK_TEMP_SET: Token = 4;
const TOK_DEW: Token = 5;
const TOK_AUX_STATE: Token = 6;

lazy_static! {
    static ref DEVICE: AnnounceDevice = AnnounceDevice {
        identifiers: vec![
            env!("CARGO_PKG_NAME").into(),
            format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
        ],
        manufacturer: env!("CARGO_PKG_AUTHORS").into(),
        model: "Virtual HVAC".into(),
        name: "HVAC Controller".into(),
        sw_version: Some(env!("CARGO_PKG_VERSION").into()),
        via_device: None
    };
}

#[derive(Debug, Serialize)]
struct Discovery<'a> {
    action_topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    aux_command_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aux_state_topic: Option<String>,
    availability_topic: String,
    current_temperature_topic: String,
    device: &'static AnnounceDevice,
    initial: f32,
    mode_command_topic: String,
    mode_state_topic: String,
    modes: Vec<&'static str>,
    name: &'a str,
    payload_off: &'static str,
    payload_on: &'static str,
    temperature_command_topic: String,
    temperature_state_topic: String,
    temp_step: f32,
    unique_id: String,
}

#[derive(Default, Debug, Clone, Deserialize)]
pub struct Conf {
    heat_state: String,
    heat_cmnd: String,
    aux_state: Option<String>,
    aux_cmnd: Option<String>,
    temp: String,
    dew: Option<String>,
    #[serde(default)]
    offset: f32,
}

#[derive(Debug, Clone, IntoStaticStr, strum_macros::Display, Deserialize)]
enum Action {
    #[strum(serialize = "off")]
    Off,
    #[strum(serialize = "idle")]
    Idle,
    #[strum(serialize = "heating")]
    Heating,
}

#[derive(
    Debug, Clone, PartialEq, IntoStaticStr, strum_macros::Display, EnumString, Deserialize,
)]
enum Mode {
    #[strum(serialize = "off")]
    Off,
    #[strum(serialize = "heat")]
    Heat,
}

#[derive(Debug, Clone)]
pub struct Climate {
    name: String,
    conf: Conf,
    mode: Mode,
    temp_set: f32,
    temp_cur: f32,
    heating_on: bool,
    aux_on: bool,
    log: Logger,
}

impl Climate {
    pub fn new<S: AsRef<str>>(name: S, conf: Conf, log: &Logger) -> Self {
        Self {
            name: name.as_ref().into(),
            conf,
            mode: Mode::Heat,
            temp_set: INITIAL_TEMP,
            temp_cur: INITIAL_TEMP,
            heating_on: false,
            aux_on: false,
            log: log.new(o!("HVAC" => name.as_ref().to_owned())),
        }
    }

    fn t(&self, tail: &str) -> String {
        format!("{}/{}/{}", BASE, self.name, tail)
    }

    fn discovery(&self) -> Discovery<'_> {
        let unique_id = format!(
            "{}::climate::virtual::{}",
            env!("CARGO_PKG_NAME"),
            self.name
        );
        Discovery {
            action_topic: self.t("action"),
            aux_command_topic: self.conf.aux_cmnd.clone(),
            aux_state_topic: self.conf.aux_state.clone(),
            availability_topic: format!("{}/status", BASE), // global status
            current_temperature_topic: self.t("current"),
            device: &DEVICE,
            initial: INITIAL_TEMP,
            payload_on: "1",
            payload_off: "0",
            mode_command_topic: self.t("mode/set"),
            mode_state_topic: self.t("mode"),
            modes: vec!["off", "heat"],
            name: &self.name,
            temperature_command_topic: self.t("target/set"),
            temperature_state_topic: self.t("target"),
            temp_step: 0.5,
            unique_id,
        }
    }

    pub fn announce(&self) -> MqttMsg {
        debug!(self.log, "Announcing");
        MqttMsg::retain(
            self.t("config"),
            serde_json::to_string(&self.discovery()).unwrap(),
        )
    }

    /// Return topics which this HVAC controller should be subscribed to.
    pub fn subscribe(&self) -> impl Iterator<Item = (Token, String)> {
        let mut t = vec![
            (TOK_HEAT_STATE, self.conf.heat_state.clone()),
            (TOK_TEMP, self.conf.temp.clone()),
            (TOK_MODE_SET, self.t("mode/set")),
            (TOK_TEMP_SET, self.t("target/set")),
        ];
        if let Some(dew) = &self.conf.dew {
            t.push((TOK_DEW, dew.clone()));
        }
        if let Some(aux_state) = &self.conf.aux_state {
            t.push((TOK_AUX_STATE, aux_state.clone()))
        }
        t.into_iter()
    }

    /// Determins the curent run mode of this virtual unit
    fn action(&self) -> Action {
        if self.mode == Mode::Off {
            return Action::Off;
        }
        if self.heating_on {
            return Action::Heating;
        }
        Action::Idle
    }

    /// Turns auxiliary heating on or off if present
    fn set_aux(&self, on: bool) -> Vec<MqttMsg> {
        if let Some(aux_cmnd) = &self.conf.aux_cmnd {
            if self.aux_on != on {
                info!(self.log, "Setting auxiliary heating to {}", on);
                return vec![MqttMsg::new(aux_cmnd.to_string(), bool2str(on))];
            }
        }
        Vec::new()
    }

    pub fn process(&mut self, token: Token, _topic: &str, payload: &str) -> Result<Vec<MqttMsg>> {
        match token {
            TOK_TEMP_SET => {
                let new = payload
                    .parse::<f32>()
                    .map_err(|e| Error::FloatFormat(payload.into(), e))?;
                if (self.temp_set - new).abs() > EPSILON_TEMP {
                    info!(
                        self.log,
                        "Setting {} target temperature to {} °C", self.name, new
                    );
                    self.temp_set = new;
                    let mut res = self.eval();
                    res.push(MqttMsg::retain(self.t("target/set"), payload));
                    return Ok(res);
                }
            }
            TOK_TEMP => {
                let new = payload
                    .parse::<f32>()
                    .map_err(|e| Error::FloatFormat(payload.into(), e))?
                    + self.conf.offset;
                if (self.temp_cur - new).abs() > EPSILON_TEMP {
                    self.temp_cur = new;
                    return Ok(self.eval());
                }
            }
            TOK_MODE_SET => {
                let new = payload
                    .parse()
                    .map_err(|_| Error::Keyword(payload.into()))?;
                if self.mode != new {
                    debug!(self.log, "Setting mode {}", new);
                    self.mode = new;
                    let mut res = self.eval();
                    res.push(MqttMsg::retain(self.t("mode/set"), payload));
                    return Ok(res);
                }
            }
            TOK_HEAT_STATE => {
                let new = str2bool(payload);
                if self.heating_on != new {
                    debug!(self.log, "Heating is {}", new);
                    self.heating_on = new;
                    return Ok(self.eval());
                }
            }
            TOK_AUX_STATE => {
                let new = str2bool(payload);
                if self.aux_on != new {
                    debug!(self.log, "Aux heating is {}", new);
                    self.aux_on = new;
                }
            }
            _ => (),
        }
        Ok(Vec::new())
    }

    pub fn eval(&self) -> Vec<MqttMsg> {
        let mut res = vec![
            MqttMsg::new(self.t("action"), self.action()),
            MqttMsg::new(self.t("mode"), &self.mode),
            MqttMsg::new(self.t("current"), self.temp_cur),
            MqttMsg::new(self.t("target"), self.temp_set),
        ];
        if self.mode == Mode::Off {
            if self.heating_on {
                info!(self.log, "Turning heating off ({} disabled)", self.name);
                res.push(MqttMsg::new(&self.conf.heat_cmnd, bool2str(false)));
            }
            res.extend(self.set_aux(false));
            return res;
        }
        if self.temp_cur >= self.temp_set - 0.1 && self.aux_on {
            info!(
                self.log,
                "Turning aux heating off ({}={:.2} °C)", self.name, self.temp_cur
            );
            res.extend(self.set_aux(false));
        }
        match (self.temp_cur < self.temp_set, self.heating_on) {
            (true, false) => {
                info!(
                    self.log,
                    "Turning heating on ({}={:.2} °C)", self.name, self.temp_cur
                );
                res.push(MqttMsg::new(&self.conf.heat_cmnd, bool2str(true)));
                // Use auxiliary heating to bridge larger temperature gaps
                if self.temp_cur < self.temp_set - AUX_HEAT_TRIGGER {
                    res.extend(self.set_aux(true));
                }
            }
            (false, true) => {
                info!(
                    self.log,
                    "Turning heating off ({}={:.2} °C)", self.name, self.temp_cur
                );
                res.push(MqttMsg::new(&self.conf.heat_cmnd, bool2str(false)));
            }
            _ => (),
        }
        res
    }
}

#[cfg(test)]
mod test {}
