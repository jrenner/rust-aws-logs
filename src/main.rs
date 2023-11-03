use clap::Parser;

use std::process::Command;
use std::str;
use serde::{Deserialize, Serialize};


#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    logstream_id: String,
}


#[derive(Serialize, Deserialize, Debug)]
struct EventLog {
    events: Vec<Event>,
    nextForwardToken: String,
    nextBackwardToken: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Event {
    timestamp: u64,
    message: String,
    ingestionTime: u64,
}


fn main() {

    let args = Args::parse();
    let stage = "dev";
    let logstream_id = args.logstream_id;
    let log_group = format!("/ecs/batte-backcast-{stage}");
    let log_stream = format!("ecs/batte-backcast-{stage}/{logstream_id}");

    let limit = "3";

    println!("log_group: {}, log_stream: {}", log_group, log_stream);



    let output = Command::new("aws")
        .arg("logs")
        .arg("get-log-events")
        .arg("--log-stream-name")
        .arg(log_stream)
        .arg("--log-group-name")
        .arg(log_group)
        .arg("--start-from-head")
        .arg("--limit")
        .arg(limit)
        .output()
        .expect("failed to execute process");

    if !output.status.success() {
        let stderr = str::from_utf8(&output.stderr).unwrap();
        eprintln!("failed to execute process: {}", stderr);
        panic!("failed to execute process: {}", output.status);
    } else {
        let stdout = str::from_utf8(&output.stdout).unwrap();
        let jdat: EventLog = serde_json::from_str(stdout).unwrap();
        println!("{:#?}", jdat);
    }
}
