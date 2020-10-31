use esera_mqtt::{Connection, Response};

use futures::StreamExt;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;

fn rexp_session<
    F: FnOnce(rexpect::session::StreamSession<TcpStream>) -> rexpect::errors::Result<()>
        + Send
        + 'static,
>(
    script: F,
) -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    println!("rexpect listening on {}", addr);
    thread::spawn(move || {
        let (read, _client) = listener.accept().unwrap();
        let write = read.try_clone().unwrap();
        let session = rexpect::spawn_stream(read, write, Some(1000));
        script(session).expect("rexpect script failed")
    });
    addr
}

// #[tokio::test]
// async fn read_kal() {
//     let addr = rexp_session(|mut r| {
//         r.send_line("1_KAL|1")?;
//         Ok(())
//     });
//     let mut conn = Connection::new(addr).await.unwrap();
//     assert_eq!(conn.next().await, Some(Response::KAL))
// }

// #[tokio::test]
// async fn set_datetime() {
//     let addr = rexp_session(|mut r| {
//         r.exp_string("SET,SYS,DATE,25.10.20")?;
//         r.send_line("1_DATE|25.10.20")?;
//         r.exp_string("SET,SYS,TIME,14:44:14")?;
//         r.send_line("1_TIME|14:44:14")?;
//         Ok(())
//     });
//     let mut conn = Connection::new(addr).await.unwrap();
//     conn.send("SET,SYS,DATE,25.10.20").await.unwrap();
//     assert_eq!(conn.next().await, Some(Response::Date("25.10.20".into())));
// }
