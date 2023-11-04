use clap::Parser;

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::str;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    logstream_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct EventLog {
    #[serde(rename = "events")]
    events: Vec<Event>,
    #[serde(rename = "nextForwardToken")]
    next_forward_token: String,
    #[serde(rename = "nextBackwardToken")]
    next_backward_token: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Event {
    #[serde(rename = "timestamp")]
    timestamp: u64,
    #[serde(rename = "message")]
    message: String,
    #[serde(rename = "ingestionTime")]
    ingestion_time: u64,
}

fn fetch_single_log_page(log_group: &str, log_stream: &str, fwd_token: Option<&str>) -> EventLog {
    let limit = "10000";

    let mut args = vec![
        "logs".to_string(),
        "get-log-events".to_string(),
        "--log-stream-name".to_string(),
        log_stream.to_string(),
        "--log-group-name".to_string(),
        log_group.to_string(),
        "--start-from-head".to_string(),
        "--limit".to_string(),
        limit.to_string(),
    ];

    if let Some(token) = fwd_token {
        args.push("--next-token".to_string());
        args.push(token.to_string());
    }

    let mut cmd = Command::new("aws");
    for arg in args {
        cmd.arg(arg);
    }

    //println!("cmd: {:#?}", cmd);

    let output = cmd.output().expect("failed to execute process");

    let jdat: EventLog;

    if !output.status.success() {
        let stderr = str::from_utf8(&output.stderr).unwrap();
        eprintln!("failed to execute process: {}", stderr);
        panic!("failed to execute process: {}", output.status);
    } else {
        let stdout = str::from_utf8(&output.stdout).unwrap();
        jdat = serde_json::from_str(stdout).unwrap();
        //println!("{:#?}", jdat);
    }
    jdat
}

fn fetch_entire_log() -> Vec<Event> {
    let args = Args::parse();
    let stage = "dev";
    let logstream_id = args.logstream_id;
    let log_group = format!("/ecs/batte-backcast-{stage}");
    let log_stream = format!("ecs/batte-backcast-{stage}/{logstream_id}");
    println!("fetch entire log - log_group: {log_group}, log_stream: {log_stream}");
    let mut i = 0;
    let mut current_token: Option<String> = None;
    let mut all_events = Vec::new();
    loop {
        let jdat: EventLog =
            fetch_single_log_page(&log_group, &log_stream, current_token.as_deref());
        // append all the events to all_events
        all_events.extend(jdat.events);
        let forward_token: &str = &jdat.next_forward_token;
        // check if current token is the same as this new forward token
        let backward_token = jdat.next_backward_token;
        println!("[{i}] forward_token: {forward_token}, backward_token: {backward_token}");
        if let Some(ref ct) = current_token {
            if ct == &forward_token {
                println!("current_token == forward_token, breaking");
                break;
            }
        }
        current_token = Some(forward_token.to_string());
        i += 1;
    }
    // sort all the events based on timestamp, just in case they are out of order
    all_events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    all_events
}

fn fetch_entire_log_cached(use_cached: bool) -> Vec<Event> {
    let jdat: Vec<Event>;
    if use_cached {
        // try to read cached data from /tmp/aws_log_cache.json
        let contents = std::fs::read_to_string("/tmp/aws_log_cache.json");
        match contents {
            Ok(contents) => {
                jdat = serde_json::from_str(&contents).unwrap();
                println!("using cached data");
            }
            Err(_) => {
                println!("no cached data found, running command");
                jdat = fetch_entire_log();
            }
        }
    } else {
        jdat = fetch_entire_log();
        // now serialize it back into a string
        let serialized = serde_json::to_string(&jdat).unwrap();
        // now write to /tmp/aws_log_cache.json
        std::fs::write("/tmp/aws_log_cache.json", serialized).expect("Unable to write file");
    }
    jdat
}

fn get_text_from_events(events: &Vec<Event>) -> String {
    let text: String = events
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<String>>()
        .join("\n");
    text
}

fn main() {
    let use_cached = false;
    let events: Vec<Event> = fetch_entire_log_cached(use_cached);
    let full_log_text = get_text_from_events(&events);
    println!("FULL LOG TEXT:\n{full_log_text}")
}
