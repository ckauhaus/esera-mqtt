#[macro_use]
extern crate log;
use anyhow::Result;
use futures::future::{ready, FutureExt};
use futures::sink::SinkExt;
use futures::stream::{self, FusedStream, StreamExt};
use regex::Regex;
use std::path::PathBuf;
use std::time::Duration;
use structopt::StructOpt;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_util::codec::{Framed, LinesCodec};

// #[derive(Debug, Clone)]
// enum Command {
//     Set(SetCommand),
//     Get(GetCommand),
// }
// use Command::*;

// #[derive(Debug, Clone)]
// enum SetCommand {
//     Sys(SetSys),
//     Owb(SetOwb),
//     Owd(SetOwd),
// }
// use SetCommand::*;

// #[derive(Debug, Clone)]
// enum SetSys {
//     Date(NaiveDate),
//     Time(NaiveTime),
// }

// #[derive(Debug, Clone)]
// enum SetOwb {}

// #[derive(Debug, Clone)]
// enum SetOwd {}

// #[derive(Debug, Clone)]
// enum GetCommand {}

// struct Controller {
//     artno: u32,
//     serno: String,
//     fw: String,
//     hw: String,
//     contno: u8,
// }

// struct Response {
//     contno: u8,
//     entity: String,
//     field: Option<u8>,
//     value: String,
// }

#[derive(StructOpt, Debug)]
struct Opt {
    #[structopt(short = "H", long)]
    host: String,
    #[structopt()]
    input: PathBuf,
}

lazy_static::lazy_static! {
    static ref R_SLEEP: Regex = Regex::new(r"^sleep (\d+)$").unwrap();
}

fn get_lines(input: &str) -> impl FusedStream<Item = &str> {
    tokio::stream::StreamExt::throttle(
        stream::iter(input.lines().fuse()).filter(|line| {
            let l = line.trim();
            if let Some(m) = R_SLEEP.captures(l) {
                let seconds = m[1].parse().unwrap();
                debug!("sleeping {}s", seconds);
                sleep(Duration::new(seconds, 0))
                    .map(|_| false)
                    .left_future()
            } else {
                ready(!l.is_empty() && !l.starts_with('#')).right_future()
            }
        }),
        Duration::new(0, 500_000_000),
    )
    .fuse()
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let opt = Opt::from_args();
    let input = std::fs::read_to_string(opt.input)?;
    debug!("Connecting to {}:5000", opt.host);
    let s = TcpStream::connect(format!("{}:5000", opt.host)).await?;
    let mut f = Framed::new(s, LinesCodec::new()).fuse();
    let mut i = get_lines(&input);
    loop {
        futures::select_biased! {
            output = f.next() => {
                if let Some(o) = output {
                    info!(">>> {}", o?.trim())
                }
            },
            input = i.next() => {
                if let Some(s) = input {
                    info!("<<< {}", s.trim());
                    f.send(s).await?;
                } else {
                    sleep(Duration::new(5, 0)).await;
                    break
                }
            }
        }
    }
    Ok(())
}
