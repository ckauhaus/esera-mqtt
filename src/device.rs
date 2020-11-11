use bitflags::bitflags;
use std::fmt;
use std::sync::Mutex;
use strum_macros::{Display, EnumString};

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

#[derive(Debug)]
pub struct Device {
    pub busid: String,
    pub serno: String,
    pub status: Status,
    pub artno: String,
    pub name: Option<String>,
    model: Mutex<Box<dyn Model + Send>>,
}

impl Default for Device {
    fn default() -> Self {
        Self {
            busid: String::default(),
            serno: String::default(),
            status: Status::Unconfigured,
            artno: String::default(),
            name: None,
            model: Mutex::new(Box::new(Unknown)),
        }
    }
}

fn select_model(artno: &str) -> Box<dyn Model + Send> {
    match artno {
        "11221" => Box::new(Dimmer1::default()),
        "11228" => Box::new(Switch8_16A::default()),
        "11322" => Box::new(HubIII::default()),
        "11340" => Box::new(Controller2::default()),
        _ => {
            if artno != "none" {
                warn!("Unknown model: {}", artno);
            }
            Box::new(Unknown)
        }
    }
}

impl Device {
    pub fn new(
        busid: String,
        serno: String,
        status: Status,
        artno: String,
        name: Option<String>,
    ) -> Self {
        let model = Mutex::new(select_model(&artno));
        Self {
            busid,
            serno,
            status,
            artno,
            name,
            model,
        }
    }

    #[cfg(test)]
    pub fn with_model(busid: &str, artno: &str) -> Self {
        let model = Mutex::new(select_model(artno));
        Self {
            busid: String::from(busid),
            serno: String::default(),
            status: Status::Unconfigured,
            artno: artno.into(),
            name: None,
            model,
        }
    }

    pub fn model_name(&self) -> &'static str {
        self.model.lock().unwrap().name()
    }

    pub fn register(&self) -> Vec<String> {
        self.model.lock().unwrap().register(&self)
    }

    pub fn handle_devstatus(&self, addr: &str, data: u32) -> Vec<(String, String)> {
        self.model.lock().unwrap().handle_devstatus(addr, data)
    }
}

trait Model: fmt::Debug {
    fn name(&self) -> &'static str {
        let n = std::any::type_name::<Self>();
        if let Some(pos) = n.rfind(':') {
            &n[pos + 1..]
        } else {
            n
        }
    }

    fn register(&self, dev: &Device) -> Vec<String>;

    fn handle_devstatus(&mut self, _addr: &str, _data: u32) -> Vec<(String, String)> {
        Vec::default()
    }
}

fn boolstr<N: Into<u32>>(n: N) -> &'static str {
    if n.into() == 0 {
        "0"
    } else {
        "1"
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Controller2 {
    inputs: u8,
    outputs: u8,
    ana: f32,
}

impl Model for Controller2 {
    fn register(&self, _dev: &Device) -> Vec<String> {
        vec!["SYS1_1".into(), "SYS2_1".into(), "SYS3".into()]
    }

    fn handle_devstatus(&mut self, addr: &str, data: u32) -> Vec<(String, String)> {
        let mut res = Vec::new();
        match addr {
            "SYS1_1" => {
                self.inputs = (data & 0xff) as u8;
                for bit in 0..4 {
                    res.push((
                        format!("SYS/in/{}", bit + 1),
                        boolstr(data & 1 << bit).into(),
                    ))
                }
            }
            "SYS2_1" => {
                self.outputs = (data & 0xff) as u8;
                for bit in 0..5 {
                    res.push((
                        format!("SYS/out/{}", bit + 1),
                        boolstr(data & 1 << bit).into(),
                    ))
                }
            }
            "SYS3" => {
                let val = f32::from(data as u16) / 100.0;
                self.ana = val;
                res.push(("SYS/out/6".into(), format!("{:.2}", val)))
            }
            _ => warn!("Controller2: unknown bus addr '{}', ignoring", addr),
        }
        res
    }
}

#[cfg(test)]
mod controller2_test {
    use super::*;

    #[test]
    fn process_controller_event() {
        assert_eq!(
            Controller2::default().handle_devstatus("SYS1_1", 9),
            vec![
                ("SYS/in/1".into(), "1".into()),
                ("SYS/in/2".into(), "0".into()),
                ("SYS/in/3".into(), "0".into()),
                ("SYS/in/4".into(), "1".into())
            ]
        );
        assert_eq!(
            Controller2::default().handle_devstatus("SYS2_1", 12),
            vec![
                ("SYS/out/1".into(), "0".into()),
                ("SYS/out/2".into(), "0".into()),
                ("SYS/out/3".into(), "1".into()),
                ("SYS/out/4".into(), "1".into()),
                ("SYS/out/5".into(), "0".into())
            ]
        );
        assert_eq!(
            Controller2::default().handle_devstatus("SYS3", 526),
            vec![("SYS/out/6".into(), "5.26".into())]
        );
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct HubIII {
    voltage12: f32,
    current12: f32,
    voltage5: f32,
    current5: f32,
}

impl Model for HubIII {
    fn register(&self, d: &Device) -> Vec<String> {
        (1..=4).map(|i| format!("{}_{}", d.busid, i)).collect()
    }
}

bitflags! {
    #[derive(Default)]
    struct SwitchFlags: u8 {
        const CH1 = 1<<0;
        const CH2 = 1<<1;
        const CH3 = 1<<2;
        const CH4 = 1<<3;
        const CH5 = 1<<4;
        const CH6 = 1<<5;
        const CH7 = 1<<6;
        const CH8 = 1<<7;
    }

}

#[derive(Debug, Default, Clone, PartialEq)]
struct Switch8_16A {
    inputs: SwitchFlags,
    outputs: SwitchFlags,
}

impl Model for Switch8_16A {
    fn register(&self, d: &Device) -> Vec<String> {
        vec![format!("{}_1", d.busid), format!("{}_3", d.busid)]
    }
}

bitflags! {
    #[derive(Default)]
    struct DimmerFlags: u8 {
        const EXT_PB1 = 1<<0;
        const EXT_PB2 = 1<<1;
        const MODULE_PB1 = 1<<2;
        const MODULE_PB2 = 1<<3;
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Dimmer1 {
    inputs: DimmerFlags,
    ch1: u8,
    ch2: u8,
}

impl Model for Dimmer1 {
    fn register(&self, d: &Device) -> Vec<String> {
        vec![
            format!("{}_1", d.busid), // inputs
            format!("{}_3", d.busid), // dimmer channel 1
            format!("{}_4", d.busid), // dimmer channel 2
        ]
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Unknown;

impl Model for Unknown {
    fn register(&self, _dev: &Device) -> Vec<String> {
        vec![]
    }
}
