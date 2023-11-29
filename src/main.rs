use std::collections::HashMap;
use clap::Parser;

use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;
use std::process::Command;
use std::str;
use aws_config::BehaviorVersion;

use rayon::prelude::*;

use log::{debug, error, info};
use rayon::ThreadPoolBuilder;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// list log groups in this AWS account
    #[arg(long, action = clap::ArgAction::SetTrue)]
    describe_log_groups: bool,

    /// list log streams in this log group
    #[arg(long, action = clap::ArgAction::SetTrue)]
    describe_log_streams: bool,

    /// log stream to fetch contents of
    #[arg(short = 's', long)]
    log_stream: Option<String>,

    /// log group
    #[arg(short = 'g', long)]
    log_group: Option<String>,

    /// output file to write to
    #[arg(short, long)]
    output_file: Option<String>,

    /// get previews of the log streams when listing log groups
    #[arg(long, action = clap::ArgAction::SetTrue)]
    preview: bool,
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

#[derive(Serialize, Deserialize, Debug)]
struct LogGroupsResponse {
    #[serde(rename = "logGroups")]
    log_groups: Vec<LogGroup>,
}

#[derive(Serialize, Deserialize, Debug)]
struct LogGroup {
    #[serde(rename = "logGroupName")]
    log_group_name: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct LogStreamsResponse {
    #[serde(rename = "logStreams")]
    log_streams: Vec<LogStream>,
}

#[derive(Serialize, Deserialize, Debug)]
struct LogStream {
    #[serde(rename = "logStreamName")]
    log_stream_name: String,

    #[serde(rename = "creationTime")]
    creation_time: i64,
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

fn fetch_single_log_page(
    log_group: &str,
    log_stream: &str,
    fwd_token: Option<&str>,
    limit: Option<i32>,
) -> Result<EventLog, String> {
    println!("fetch preview for: {log_stream}");
    let limit_txt = limit.unwrap_or(10000).to_string();

    let mut args = vec![
        "logs",
        "get-log-events",
        "--log-stream-name",
        log_stream,
        "--log-group-name",
        log_group,
        "--start-from-head",
        "--limit",
        &limit_txt,
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

fn fetch_first_n_events(log_group: &str, log_stream: &str, limit: i32) -> Vec<Event> {
    if log_stream.starts_with("/") {
        panic!("log_stream should probably not begin with / -> {log_stream}");
    }
    info!("fetch first N events from log stream - log_group: {log_group}, log_stream: {log_stream}");
    let fwd_token: Option<&str> = None;
    let event_log: EventLog =
        fetch_single_log_page(&log_group, &log_stream, fwd_token, Some(limit))
            .unwrap_or_else(|e| panic!("failed to fetch single log page: {}", e));
    // append all the events to all_events
    let page_size = event_log.events.len();
    info!("fetched single page, size: {page_size}, limit was: {limit}");
    let mut all_events = event_log.events;
    all_events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    all_events
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
        let limit: Option<i32> = None;
        let event_log: EventLog =
            fetch_single_log_page(&log_group, &log_stream, current_token.as_deref(), limit)
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

fn fetch_entire_log_cached(log_group: &str, log_stream: &str, use_cache: bool) -> Vec<Event> {
    let events: Vec<Event>;
    if use_cache {
        let cache_dir = env::temp_dir().join("aws_log_cache");
        let log_group_safe = clean_path_str(log_group);
        let log_stream_safe = clean_path_str(log_stream);
        let full_path = Path::new(&cache_dir)
            .join(log_group_safe)
            .join(log_stream_safe);

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
        std::fs::create_dir_all(&full_path.parent().unwrap())
            .expect("Unable to create dir for cache file");
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


async fn get_sorted_log_stream_names(client: &aws_sdk_cloudwatchlogs::Client, log_group:&str) -> Result<Vec<String>, String> {
    let mut log_stream_names: Vec<String> = Vec::new();
    let mut next_token: Option<String> = None;
    loop {
        let mut request = client.describe_log_streams();
        request = request.log_group_name(log_group);
        if let Some(ref token) = next_token {
            request = request.next_token(token);
        }
        let response = request.send().await.unwrap();
        let log_streams_option = response.log_streams;
        // TODO could this end up abandoning a partially built result we actually would like to return?
        if log_streams_option.is_none() {
            return Err("log_streams_option is None".to_string());
        } else {
            let log_streams = log_streams_option.unwrap();
            let mut names = log_streams
                .into_iter()
                .map(|stream| stream.log_stream_name.unwrap())
                .collect::<Vec<String>>();
            log_stream_names.append(&mut names);
        }
        next_token = response.next_token;
        if next_token.is_none() {
            break;
        }
    }
    log_stream_names.sort(); // Sorts alphabetically by default
    Ok(log_stream_names)
}

fn get_sorted_log_stream_names_old(log_group: &str) -> Result<Vec<String>, String> {
    let mut cmd = Command::new("aws");
    cmd.args(&[
        "logs",
        "describe-log-streams",
        "--log-group-name",
        log_group,
    ]);

    let output = cmd
        .output()
        .expect("failed to execute aws logs describe-log-streams");
    if !output.status.success() {
        let stderr = str::from_utf8(&output.stderr).unwrap_or("Error: Unable to read stderr");
        return Err(stderr.to_string());
    }

    let stdout = str::from_utf8(&output.stdout).unwrap_or("Error: Unable to read stdout");
    let mut log_streams_response: LogStreamsResponse =
        serde_json::from_str(stdout).map_err(|e| e.to_string())?;

    log_streams_response.log_streams.sort_by_key(|log_stream| log_stream.creation_time);


    let log_stream_names: Vec<String> = log_streams_response
        .log_streams
        .into_iter()
        .map(|stream| stream.log_stream_name)
        .collect();

    Ok(log_stream_names)
}

fn init_thread_pool() {
    // it's not cpu intensive, but we want to do a ton at once
    let num_threads = 20;
    ThreadPoolBuilder::new().num_threads(num_threads).build_global().unwrap();
}

async fn get_cloudwatch_client() -> aws_sdk_cloudwatchlogs::Client {
    let config =
        aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);
    client
}

async fn get_sorted_log_group_names(client: &aws_sdk_cloudwatchlogs::Client) -> Result<Vec<String>, String> {
    let log_groups_output = client.describe_log_groups().send().await.unwrap();
    let log_groups_option = log_groups_output.log_groups;
    if log_groups_option.is_none() {
        return Err("log_groups_option is None".to_string());
    } else {
        // get all log group names sorted by alphabetical
        let mut log_group_names: Vec<String> = log_groups_option
            .unwrap()
            .into_iter()
            .map(|group| group.log_group_name.unwrap())
            .collect();
        log_group_names.sort(); // Sorts alphabetically by default
        Ok(log_group_names)
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
    init_thread_pool();
    let cwl_client = get_cloudwatch_client().await;
    let client = &cwl_client;

    if args.describe_log_groups {
        let log_group_names = get_sorted_log_group_names(client).await.unwrap();
        println!("Log Groups:");
        for name in log_group_names {
            println!("{}", name);
        }
        return;
    }
    let log_group = args.log_group.unwrap_or(String::from(""));
    if args.describe_log_streams {
        if log_group.is_empty() {
            println!("--log-group is required when using --describe-log-streams");
            return;
        }
        let log_stream_names = get_sorted_log_stream_names(client, &log_group).await.unwrap_or_else(|e| {
            println!("Error: {}", e);
            std::process::exit(1);
        });
        let mut logstream_previews: HashMap<String, String> = HashMap::new();
        if args.preview {
            // get the first 10 lines from each log stream of the last few log streams
            let preview_amount = 6;
            let preview_log_stream_names = log_stream_names
                .iter()
                .rev()
                .take(preview_amount)
                .map(|s| s.as_str())
                .collect::<Vec<&str>>();

            logstream_previews = preview_log_stream_names.par_iter().map(|log_stream| {
                let n = 10;
                let events = fetch_first_n_events(&log_group, log_stream.to_string().as_str(), n);
                let text = get_text_from_events(&events);
                (log_stream.to_string(), text)
            }).collect::<HashMap<_, _>>();
        }
        println!("Log Streams (log group: {log_group}):");
        for name in log_stream_names {
            if args.preview {
                println!("\n------------------\n{}", name);
                // check if it's in the hashmap
                let is_in_hashmap = logstream_previews.contains_key(&name);
                if is_in_hashmap {
                    let preview = logstream_previews.get(&name).unwrap();
                    println!("PREVIEW:\n{}", preview);
                }
            } else {
                println!("{}", name);
            }
        }
        return;
    }

    let log_stream = args.log_stream.expect("log-stream argument not supplied");

    let use_cache = env::var("ALOG_USE_CACHE")
        .ok()
        .and_then(|value| value.parse::<bool>().ok())
        .unwrap_or(false);

    println!("The value of use_cache is: {}", use_cache);


    let events: Vec<Event> = fetch_entire_log_cached(&log_group, &log_stream, use_cache);
    let full_log_text = get_text_from_events(&events);

    if let Some(fpath) = args.output_file {
        let error_msg = format!("Unable to write file: {fpath}");
        info!("writing to file: {fpath}");
        std::fs::write(&fpath, full_log_text).expect(&error_msg);
    } else {
        println!("FULL LOG TEXT:\n{full_log_text}");
    }
}
