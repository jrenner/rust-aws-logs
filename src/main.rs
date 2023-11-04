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
    #[serde(rename = "events")]
    events: Vec<Event>,

    #[serde(rename = "nextForwardToken")]
    next_forward_token: String,

    #[serde(rename = "nextBackwardToken")]
    next_backward_token: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Event {
    #[serde(rename = "timestamp")]
    timestamp: u64,

    #[serde(rename = "message")]
    message: String,

    #[serde(rename = "ingestionTime")]
    ingestion_time: u64,
}


fn run_command(use_cached: bool) -> EventLog {
    let args = Args::parse();
    let stage = "dev";
    let logstream_id = args.logstream_id;
    let log_group = format!("/ecs/batte-backcast-{stage}");
    let log_stream = format!("ecs/batte-backcast-{stage}/{logstream_id}");

    let limit = "100";

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

    let jdat: EventLog;
    if !output.status.success() {
        let stderr = str::from_utf8(&output.stderr).unwrap();
        eprintln!("failed to execute process: {}", stderr);
        panic!("failed to execute process: {}", output.status);
    } else {
        let stdout = str::from_utf8(&output.stdout).unwrap();
        jdat = serde_json::from_str(stdout).unwrap();
        println!("{:#?}", jdat);
    }
    // now serialize it back into a string
    let serialized = serde_json::to_string(&jdat).unwrap();
    // now write to /tmp/aws_log_cache.json
    std::fs::write("/tmp/aws_log_cache.json", serialized).expect("Unable to write file");

    jdat
}


fn main() {

    // write to file: /tmp/log.json
    std::fs::write("/tmp/log.json", serialized).expect("Unable to write file");

    let jdat: EventLog = run_command();

    // loop through events an extract all the "message" fields, and store them in a data structure
    let mut messages: Vec<String> = Vec::new();
    for event in jdat.events {
        messages.push(event.message);
    }
    println!("{:#?}", messages);


}
