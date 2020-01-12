use smallstr::SmallString;
use std::collections::HashMap;

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

pub type BusID = SmallString<[u8; 5]>;
pub type NameStr = SmallString<[u8; 20]>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevInfo {
    busid: BusID,
    serial: SmallString<[u8; 16]>,
    err: u32,
    art: Article,
    name: NameStr,
}

impl DevInfo {
    pub fn new<S: Into<BusID>, A: Into<Article>>(
        busid: S,
        serial: &str,
        err: u32,
        art: A,
        name: &str,
    ) -> Self {
        Self {
            busid: busid.into(),
            serial: serial.into(),
            err,
            art: art.into(),
            name: name.into(),
        }
    }

    pub fn friendly_name(&self) -> &str {
        if self.name.is_empty() {
            self.busid.as_str()
        } else {
            self.name.as_str()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Devices {
    by_busid: HashMap<BusID, DevInfo>,
    by_name: HashMap<NameStr, BusID>,
}

impl Devices {
    pub fn new() -> Self {
        let mut s = Self {
            by_busid: HashMap::new(),
            by_name: HashMap::new(),
        };
        s.by_busid.insert("".into(), DevInfo::default());
        s
    }

    pub fn has<S: AsRef<str>>(&self, busid: S) -> bool {
        self.by_busid.contains_key(busid.as_ref())
    }

    pub fn by_busid<S: AsRef<str>>(&self, id: S) -> &DevInfo {
        self.by_busid
            .get(id.as_ref())
            .unwrap_or_else(|| self.by_busid.get("").unwrap())
    }

    pub fn insert(&mut self, info: DevInfo) {
        info!("insert({:?})", info);
        let id = info.busid.clone();
        if !info.name.is_empty() {
            self.by_name.insert(info.name.clone(), id.clone());
        }
        self.by_busid.insert(id, info);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn friendly_name() {
        assert_eq!(
            DevInfo::new("OWD1", "", 0, Article::Unknown, "").friendly_name(),
            "OWD1"
        );
        assert_eq!(
            DevInfo::new("OWD1", "", 0, Article::Unknown, "TEMP2").friendly_name(),
            "TEMP2"
        );
    }
}
