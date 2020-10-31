use std::net::TcpListener;

use rexpect::spawn_stream;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    println!("listening on {}", listener.local_addr().unwrap());
    let (read, _client) = listener.accept()?;
    let write = read.try_clone().unwrap();
    let mut p = spawn_stream(read, write, Some(10_000));
    let (before, after) = p.exp_regex(r"\nhello")?;
    println!("before={}, after={}", before, after);
    Ok(())
}
