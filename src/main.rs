use clap::Parser;

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::str;

use log::{debug, error, info};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short = 's', long)]
    log_stream: String,

    #[arg(short = 'g', long)]
    log_group: String,

    #[arg(short, long)]
    output_file: Option<String>,
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

fn fetch_single_log_page(
    log_group: &str,
    log_stream: &str,
    fwd_token: Option<&str>,
) -> Result<EventLog, String> {
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

    debug!("cmd: {:#?}", cmd);

    let output = cmd.output().expect("failed to execute process");
    let event_log: EventLog;

    if !output.status.success() {
        let stderr = str::from_utf8(&output.stderr).unwrap();
        let err_msg = "failed to fetch single page of logs:".to_string() + stderr;
        error!("{}", err_msg);
        Err(err_msg)
        //panic!("failed to execute process: {}", output.status);
    } else {
        let stdout = str::from_utf8(&output.stdout).unwrap();
        event_log = serde_json::from_str(stdout).unwrap();
        Ok(event_log)
    }
}

fn fetch_entire_log(log_group: &str, log_stream: &str) -> Vec<Event> {
    if log_stream.starts_with("/") {
        panic!("log_stream should probably not begin with / -> {log_stream}");
    }

    info!("fetch entire log - log_group: {log_group}, log_stream: {log_stream}");
    let mut i = 0;
    let mut current_token: Option<String> = None;
    let mut all_events = Vec::new();
    loop {
        let event_log: EventLog =
            fetch_single_log_page(&log_group, &log_stream, current_token.as_deref())
                .unwrap_or_else(|e| panic!("failed to fetch single log page: {}", e));
        // append all the events to all_events
        let page_size = event_log.events.len();
        if page_size == 0 {
            debug!("page size is 0, break loop");
            break;
        }
        all_events.extend(event_log.events);
        let forward_token: &str = &event_log.next_forward_token;
        // check if current token is the same as this new forward token
        let backward_token = event_log.next_backward_token;

        debug!("[{i}] forward_token: {forward_token}, backward_token: {backward_token}");
        let n = i + 1;
        info!("fetched page {n}, size: {page_size}");

        if let Some(ref ct) = current_token {
            if ct == &forward_token {
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

fn fetch_entire_log_cached(log_group: &str, log_stream: &str, use_cached: bool) -> Vec<Event> {
    let events: Vec<Event>;
    if use_cached {
        // try to read cached data from /tmp/aws_log_cache.json
        let contents = std::fs::read_to_string("/tmp/aws_log_cache.json");
        match contents {
            Ok(contents) => {
                events = serde_json::from_str(&contents).unwrap();
                info!("using cached data");
            }
            Err(_) => {
                info!("no cached data found, running command");
                events = fetch_entire_log(log_group, log_stream);
            }
        }
    } else {
        events = fetch_entire_log(log_group, log_stream);
        // now serialize it back into a string
        let serialized = serde_json::to_string(&events).unwrap();
        // now write to /tmp/aws_log_cache.json
        std::fs::write("/tmp/aws_log_cache.json", serialized).expect("Unable to write file");
    }
    events
}

fn get_text_from_events(events: &Vec<Event>) -> String {
    let text: String = events
        .iter()
        .map(|e| e.message.trim().to_string())
        .collect::<Vec<String>>()
        .join("\n");
    text
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    let log_group = args.log_group;
    let log_stream = args.log_stream;
    let output_file = args.output_file;

    let use_cached = false;
    let events: Vec<Event> = fetch_entire_log_cached(&log_group, &log_stream, use_cached);
    let full_log_text = get_text_from_events(&events);

    if let Some(fpath) = output_file {
        let error_msg = format!("Unable to write file: {fpath}");
        info!("writing to file: {fpath}");
        std::fs::write(&fpath, full_log_text).expect(&error_msg);
    } else {
        info!("FULL LOG TEXT:\n{full_log_text}");
    }
}
