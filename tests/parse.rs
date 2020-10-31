use esera_mqtt::{Connection, Response};

use futures::StreamExt;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;

fn rexp_session<F: FnOnce(rexpect::session::StreamSession<TcpStream>) + Send + 'static>(
    script: F,
) -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    println!("rexpect listening on {}", addr);
    thread::spawn(move || {
        let (read, _client) = listener.accept().unwrap();
        let write = read.try_clone().unwrap();
        let session = rexpect::spawn_stream(read, write, Some(1000));
        script(session)
    });
    addr
}

#[tokio::test]
async fn read_kal() {
    let addr = rexp_session(|mut r| {
        r.send_line("1_KAL|1").unwrap();
    });
    let mut conn = Connection::new(addr).await.unwrap();
    assert_eq!(conn.next().await, Some(Response::KAL))
}
