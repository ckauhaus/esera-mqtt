use crate::parser::Status;
use std::fmt;

#[derive(Debug)]
pub struct Device {
    pub serial: String,
    pub status: Status,
    pub artno: String,
    pub name: Option<String>,
    model: Box<dyn Model>,
}

impl Default for Device {
    fn default() -> Self {
        Self {
            serial: String::default(),
            status: Status::Unconfigured,
            artno: String::default(),
            name: None,
            model: Box::new(Unknown),
        }
    }
}

impl Device {
    pub fn new(serial: String, status: Status, artno: String, name: Option<String>) -> Self {
        Self {
            serial,
            status,
            artno,
            name,
            model: Box::new(Unknown),
        }
    }
}

pub trait Model: fmt::Debug {}

#[derive(Debug, Default, Clone)]
pub struct Controller2;

impl Model for Controller2 {}

#[derive(Debug, Default, Clone)]
pub struct Unknown;

impl Model for Unknown {}
