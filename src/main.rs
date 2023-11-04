use clap::Parser;

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::str;
use std::env;
use std::path::Path;

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


fn clean_path_str(path: &str) -> String {
    // replace everything that is not letters or numbers with underscore
    let mut clean_path = String::new();
    for c in path.chars() {
        if c.is_alphanumeric() {
            clean_path.push(c);
        } else {
            clean_path.push('_');
        }
    }
    clean_path
}

fn verify_aws_cli_tool_available() {
    let mut cmd = Command::new("aws");
    cmd.arg("--version");
    let output = cmd.output().expect("failed to execute process");
    if !output.status.success() {
        panic!("aws cli tool not available");
    }
}

fn fetch_single_log_page(
    log_group: &str,
    log_stream: &str,
    fwd_token: Option<&str>,
) -> Result<EventLog, String> {
    let limit = "10000";

    let mut args = vec![
        "logs",
        "get-log-events",
        "--log-stream-name",
        log_stream,
        "--log-group-name",
        log_group,
        "--start-from-head",
        "--limit",
        limit,
    ];

    if let Some(token) = fwd_token {
        args.push("--next-token");
        args.push(token);
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
        let cache_dir = env::temp_dir().join("aws_log_cache");
        let log_group_safe = clean_path_str(log_group);
        let log_stream_safe = clean_path_str(log_stream);
        let full_path = Path::new(&cache_dir).join(log_group_safe).join(log_stream_safe);

        let contents = std::fs::read_to_string(&full_path);
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
        // now serialize it back into a string
        let serialized = serde_json::to_string(&events).unwrap();
        // write to cache file
        // create all directories in full_path if they don't exist
        std::fs::create_dir_all(&full_path.parent().unwrap()).expect("Unable to create dir for cache file");
        std::fs::write(&full_path, serialized).expect("Unable to write file");
        info!("wrote cache file: {}", &full_path.display())
    } else {
        events = fetch_entire_log(log_group, log_stream);
    }
    events
}

fn get_text_from_events(events: &[Event]) -> String {
    let text: String = events
        .iter()
        .map(|e| e.message.trim())
        .collect::<Vec<&str>>()
        .join("\n");
    text
}

fn main() {
    env_logger::init();
    verify_aws_cli_tool_available();
    let args = Args::parse();

    let log_group = args.log_group;
    let log_stream = args.log_stream;
    let output_file = args.output_file;

    let use_cached = true;
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
