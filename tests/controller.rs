mod common;
use common::rexp_session;
use esera_mqtt::{Bus, Connection, Status};

use chrono::Local;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn init_sequence() {
    let endpoint = rexp_session(|mut r| {
        r.exp_string("SET,SYS,DATAPRINT,1")?;
        r.send_line("2_DATAPRINT|1")?;
        let now = Local::now();
        r.exp_string("GET,SYS,INFO")?;
        r.send_line(&format!(
            "\
            2_HW|20\n\
            2_CSI|{time}\n\
            2_DATE|{date}\n\
            2_TIME|{time}\n\
            2_ARTNO|11340\n\
            2_SERNO|113402019V2.0-243\n\
            2_FW|V1.20_29b\n\
            2_HW|V2.0\n\
            2_CONTNO|2",
            date = now.format("%d.%m.%y"),
            time = now.format("%H:%M:%S")
        ))?;
        Ok(())
    });
    let mut conn = Connection::new(endpoint).await.unwrap();
    let bus = Bus::init(&mut conn).await.unwrap();
    assert_eq!(bus.contno, 2);
    dbg!(&bus);

    let mut i = bus.iter();
    assert_eq!(i.next().unwrap().artno, "11340");
    assert!(i.all(|dev| dev.status == Status::Unconfigured));

    let mut addrs: Vec<_> = bus.handlers().collect();
    addrs.sort();
    assert_eq!(&addrs, &["SYS1_1", "SYS2_1", "SYS3"]);
}

#[tokio::test]
async fn scan_bus() {
    // env_logger::init();
    let endpoint = rexp_session(|mut r| {
        r.exp_string("GET,OWB,LISTALL1")?;
        r.send_line(
            "1_LST3|15:53:02\n\
             LST|1_OWD1|9700001B47945E20|S_0|11322|HUB               \n\
             LST|1_OWD2|5E00001B46F81429|S_0|11228|K1                \n\
             LST|1_OWD3|5D00001BA2F4CC29|S_5|DS2408|                  \n\
             LST|1_OWD4|FFFFFFFFFFFFFFFF|S_10|none|                  \n\
             LST|1_OWD5|8D00001982810629|S_0|11221|K9                \n\
             1_EVT|14:45:00",
        )?;
        Ok(())
    });
    let mut conn = Connection::new(endpoint).await.unwrap();
    let mut bus = Bus::default();
    bus.scan(&mut conn).await.unwrap();

    use esera_mqtt::Status::*;
    assert_eq!(
        bus.iter()
            .skip(1)
            .take(6)
            .map(|d| (
                d.busid.as_str(),
                d.serno.as_str(),
                d.status,
                d.name.as_deref().unwrap_or_default(),
                d.model_name()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("OWD1", "9700001B47945E20", Online, "HUB", "HubIII"),
            ("OWD2", "5E00001B46F81429", Online, "K1", "Switch8_16A"),
            ("OWD3", "5D00001BA2F4CC29", Offline, "", "Unknown"),
            ("OWD4", "FFFFFFFFFFFFFFFF", Unconfigured, "", "Unknown"),
            ("OWD5", "8D00001982810629", Online, "K9", "Dimmer1"),
            ("", "", Unconfigured, "", "Unknown"),
        ]
    );

    let mut addrs: Vec<_> = bus.handlers().collect();
    addrs.sort();
    assert_eq!(
        &addrs,
        &[
            "OWD1_1", "OWD1_2", "OWD1_3", "OWD1_4", "OWD2_1", "OWD2_3", "OWD5_1", "OWD5_3",
            "OWD5_4"
        ]
    );
}
