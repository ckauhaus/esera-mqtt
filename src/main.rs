#[macro_use]
extern crate log;

use anyhow::Result;
use futures::{self, StreamExt};
use std::path::PathBuf;
use structopt::StructOpt;

use esera_mqtt::Connection;

#[derive(StructOpt, Debug)]
struct Opt {
    #[structopt(short = "c", long)]
    controller_addr: String,
    #[structopt()]
    input: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let opt = Opt::from_args();
    let mut conn = Connection::new((opt.controller_addr.as_str(), 5000)).await?;
    loop {
        futures::select_biased! {
            output = conn.next() => {
                if let Some(o) = output {
                    debug!(">>> {:?}", o)
                }
            },
        }
    }
}
