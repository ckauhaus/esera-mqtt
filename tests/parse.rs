use esera_mqtt::{Connection, Response};
use futures::{SinkExt, StreamExt};

mod common;
use common::rexp_session;

type Result<T = (), E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

#[tokio::test]
async fn read_kal() -> Result {
    let addr = rexp_session(|mut r| {
        r.send_line("1_KAL|1")?;
        Ok(())
    });
    let mut conn = Connection::new(addr).await?;
    assert_eq!(conn.next().await.unwrap()?, Response::KAL);
    Ok(())
}

#[tokio::test]
async fn set_datetime() -> Result {
    let addr = rexp_session(|mut r| {
        r.exp_string("SET,SYS,DATE,25.10.20")?;
        r.send_line("1_DATE|25.10.20")?;
        r.exp_string("SET,SYS,TIME,14:44:14")?;
        r.send_line("1_TIME|14:44:14")?;
        Ok(())
    });
    let mut conn = Connection::new(addr).await?;
    conn.send("SET,SYS,DATE,25.10.20").await?;
    assert_eq!(
        conn.next().await.unwrap()?,
        Response::Date("25.10.20".into())
    );
    Ok(())
}
