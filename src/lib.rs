mod bus;
pub mod controller;
mod device;
mod mqtt;
mod parser;

#[macro_use]
extern crate log;

pub use bus::Universe;
pub use controller::ControllerConnection;
pub use device::Device;
pub use mqtt::{MqttConnection, MqttMsg};
pub use parser::{DeviceInfo, Response, Status};

fn bool2str<N: Into<u32>>(n: N) -> &'static str {
    match n.into() {
        0 => "0",
        _ => "1",
    }
}

fn str2bool(s: &str) -> bool {
    matches!(s, "0")
}

fn float2centi(f: f32) -> u32 {
    (f * 100.) as u32
}

fn centi2float(c: u32) -> f32 {
    (c as f32) / 100.
}
