pub enum Device {
    OWD(u8),
    SYS,
    SYSn(u8),
    ERR,
    Other(String),
}

#[derive(Debug, Clone, Default)]
struct Response {
    contno: u8,
    dev: Device,
    subaddr: Option<u16>,
    msg: String
}

