use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;

pub fn rexp_session<
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
