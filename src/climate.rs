///! HVAC climate controller
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use strum_macros::EnumString;
use strum_macros::IntoStaticStr;
use thiserror::Error;

use crate::{bool2str, str2bool, AnnounceDevice, MqttMsg};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid numeric format: {0}: {1}")]
    FloatFormat(String, #[source] std::num::ParseFloatError),
    #[error("Cannot understand mode {0}")]
    Keyword(String),
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub static BASE: &str = "homeassistant/climate/virt";

pub type Token = u16;

const TOK_HEAT_STATE: Token = 1;
const TOK_TEMP: Token = 2;
const TOK_MODE_SET: Token = 3;
const TOK_TEMP_SET: Token = 4;
const TOK_DEW: Token = 5;
const TOK_AUX_STATE: Token = 6;

const INITIAL_TEMP: f32 = 21.0;

lazy_static! {
    static ref DEVICE: AnnounceDevice = AnnounceDevice {
        identifiers: vec![
            env!("CARGO_CRATE_NAME").into(),
            format!("{} {}", env!("CARGO_CRATE_NAME"), env!("CARGO_PKG_VERSION"))
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

#[derive(Debug, Clone, PartialEq, IntoStaticStr, strum_macros::Display, EnumString, Deserialize)]
enum Mode {
    #[strum(serialize = "off")]
    Off,
    #[strum(serialize = "heat")]
    Heat,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Climate {
    name: String,
    base: String,
    conf: Conf,
    mode: Mode,
    temp_set: f32,
    temp_cur: f32,
    heating_on: bool,
    aux_on: bool,
}

impl Climate {
    pub fn new<S: AsRef<str>>(name: S, conf: Conf) -> Self {
        Self {
            name: name.as_ref().into(),
            base: format!("{}/{}", BASE, name.as_ref()),
            conf,
            mode: Mode::Heat,
            temp_set: INITIAL_TEMP,
            temp_cur: INITIAL_TEMP,
            heating_on: false,
            aux_on: false,
        }
    }

    fn t(&self, tail: &str) -> String {
        format!("{}/{}/{}", BASE, self.name, tail)
    }

    fn discovery(&self) -> Discovery<'_> {
        let unique_id = format!(
            "{}::climate::virtual::{}",
            env!("CARGO_CRATE_NAME"),
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
            unique_id,
        }
    }

    pub fn announce(&self) -> MqttMsg {
        info!("Announcing HVAC {}", self.name);
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

    pub fn process(&mut self, token: Token, _topic: &str, payload: &str) -> Result<Vec<MqttMsg>> {
        match token {
            TOK_TEMP_SET => {
                let new = payload
                    .parse()
                    .map_err(|e| Error::FloatFormat(payload.into(), e))?;
                if self.temp_set != new {
                    info!("[{}] Setting target temperature to {} °C", self.name, new);
                    self.temp_set = new;
                    let mut res = self.eval();
                    res.push(MqttMsg::retain(self.t("target/set"), payload));
                    return Ok(res);
                }
            }
            TOK_MODE_SET => {
                let new = payload
                    .parse()
                    .map_err(|_| Error::Keyword(payload.into()))?;
                if self.mode != new {
                    debug!("[{}] Setting mode {}", self.name, new);
                    self.mode = new;
                    let mut res = self.eval();
                    res.push(MqttMsg::retain(self.t("mode/set"), payload));
                    return Ok(res);
                }
            }
            TOK_TEMP => {
                let new = payload
                    .parse::<f32>()
                    .map_err(|e| Error::FloatFormat(payload.into(), e))?;
                if self.temp_cur != new {
                    self.temp_cur = new + self.conf.offset;
                    return Ok(self.eval());
                }
            }
            TOK_HEAT_STATE => {
                let new = str2bool(payload);
                if self.heating_on != new {
                    debug!("[{}] Heating is {}", self.name, new);
                    self.heating_on = new;
                    return Ok(self.eval());
                }
            }
            TOK_AUX_STATE => {
                let new = str2bool(payload);
                if self.aux_on != new {
                    debug!("[{}] Aux heating is {}", self.name, new);
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
                res.push(MqttMsg::new(&self.conf.heat_cmnd, bool2str(false)));
            }
            if let Some(aux_cmnd) = &self.conf.aux_cmnd {
                if self.aux_on {
                    res.push(MqttMsg::new(aux_cmnd, bool2str(false)));
                }
            }
            return res;
        }
        match (self.temp_cur < self.temp_set, self.heating_on) {
            (true, false) => {
                info!("[{}] Turning on heating ({} °C)", self.name, self.temp_cur);
                res.push(MqttMsg::new(&self.conf.heat_cmnd, bool2str(true)))
            }
            (false, true) => {
                info!("[{}] Turning off heating ({} °C)", self.name, self.temp_cur);
                res.push(MqttMsg::new(&self.conf.heat_cmnd, bool2str(false)))
            }
            _ => (),
        }
        res
    }
}

#[cfg(test)]
mod test {}
