#![allow(unused)]

#[macro_use]
extern crate log;

use anyhow::Result;
use futures::{self, StreamExt};
use std::path::PathBuf;
use structopt::StructOpt;

use esera_mqtt::Connection;

#[derive(StructOpt, Debug)]
struct Opt {
    /// Host name or IP address of the ESERA controller
    #[structopt(short = "c", long)]
    controller_addr: String,
    /// Port number
    #[structopt(short, long, default_value = "5000")]
    port: u16,
    #[structopt()]
    input: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let opt = Opt::from_args();
    let mut conn = Connection::new((&*opt.controller_addr, opt.port)).await?;
    // loop {
    //     futures::select_biased! {
    //     output = conn.poll() => {
    //     if let Some(o) = output {
    //         debug!(">>> {:?}", o)
    //     }
    // },
    // }
    // }
    Ok(())
}
