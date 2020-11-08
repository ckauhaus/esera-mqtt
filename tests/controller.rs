mod common;
use common::rexp_session;
use esera_mqtt::{init_controller, Connection, Status};

use chrono::Local;

#[tokio::test]
async fn init_sequence() {
    env_logger::init();
    let addr = rexp_session(|mut r| {
        r.exp_string("SET,SYS,DATAPRINT,1")?;
        r.send_line("2_DATAPRINT|1")?;
        let now = Local::now();
        r.exp_string(&format!("SET,SYS,DATE,{}", now.format("%d.%m.%y")))?;
        r.send_line(&format!("2_DATE|{}", now.format("%d.%m.%y")))?;
        r.exp_string(&format!("SET,SYS,TIME,{}", now.format("%H:%M:")))?;
        r.send_line(&format!("2_TIME|{}", now.format("%H:%M:%S")))?;
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
    let mut conn = Connection::new(addr).await.unwrap();
    let (contno, bus) = init_controller(&mut conn).await.unwrap();
    assert_eq!(contno, 2);
    let mut i = bus.iter();
    assert_eq!(i.next().unwrap().artno, "11340");
    assert!(i.all(|dev| dev.status == Status::Unconfigured))
}
