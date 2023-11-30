use std::collections::HashMap;
use clap::Parser;

use serde::{Deserialize, Serialize};
use std::str;
use aws_config::BehaviorVersion;

use log::{debug, info};

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

    /// get previews of the log streams when listing log groups, up to N events
    #[arg(long, default_value_t = 0)]
    preview_lines: u32,

    /// get previews of the log streams when listing log groups, up to N most recent streams
    #[arg(long, default_value_t = 0)]
    preview_streams: u32,
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
    timestamp: i64,

    #[serde(rename = "message")]
    message: String,

    #[serde(rename = "ingestionTime")]
    ingestion_time: i64,
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

async fn fetch_single_log_page(
    client: &aws_sdk_cloudwatchlogs::Client,
    log_group: &str,
    log_stream: &str,
    fwd_token: Option<&str>,
    limit: Option<i32>,
) -> Result<EventLog, String> {
    let token_disp = fwd_token.unwrap_or("None");
    let limit_disp = limit.unwrap_or(-1);
    debug!("fetch single log page for: {log_stream}, token: {}, limit: {}", token_disp, limit_disp);
    let mut bld = client.get_log_events()
        .log_stream_name(log_stream)
        .log_group_name(log_group)
        .start_from_head(true);
    // determine which page to get
    if let Some(token) = fwd_token {
        bld = bld.next_token(token);
    }
    if let Some(lmt) = limit {
        bld = bld.limit(lmt);
    }
    let response = bld.send().await.unwrap();
    let events = response.events.unwrap();
    let my_events = events.into_iter().map(|event| {
        let timestamp = event.timestamp.unwrap();
        let message = event.message.unwrap();
        let ingestion_time = event.ingestion_time.unwrap();
        Event {
            timestamp,
            message,
            ingestion_time,
        }
    }).collect::<Vec<Event>>();
    let eventlog: EventLog = EventLog {
        events: my_events,
        next_forward_token: response.next_forward_token.unwrap(),
        next_backward_token: response.next_backward_token.unwrap(),
    };
    Ok(eventlog)
}


async fn fetch_first_n_events(client: &aws_sdk_cloudwatchlogs::Client, log_group: &str, log_stream: &str, limit: i32) -> Vec<Event> {
    if log_stream.starts_with("/") {
        panic!("log_stream should probably not begin with / -> {log_stream}");
    }
    info!("fetch first N events from log stream - log_group: {log_group}, log_stream: {log_stream}, limit: {limit}");
    let fwd_token: Option<&str> = None;
    let event_log: EventLog =
        fetch_single_log_page(client, &log_group, &log_stream, fwd_token, Some(limit))
            .await
            .unwrap_or_else(|e| panic!("failed to fetch single log page: {}", e));
    // append all the events to all_events
    let page_size = event_log.events.len();
    info!("fetched single page, size: {page_size}, limit was: {limit}");
    let mut all_events = event_log.events;
    all_events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    all_events
}

async fn fetch_entire_log(client: &aws_sdk_cloudwatchlogs::Client, log_group: &str, log_stream: &str) -> Vec<Event> {
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
            fetch_single_log_page(client, &log_group, &log_stream, current_token.as_deref(), limit)
                .await
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

async fn get_cloudwatch_client() -> aws_sdk_cloudwatchlogs::Client {
    let config =
        aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);
    client
}

async fn get_sorted_log_group_names(client: &aws_sdk_cloudwatchlogs::Client) -> Result<Vec<String>, String> {
    let mut all_group_names: Vec<String> = vec![];
    let mut next_token: Option<String> = None;
    let max_iters = 100;
    let mut i = 0;
    loop {
        debug!("fetch log groups, iter: {i}");
        //let log_groups_output = client.describe_log_groups().send().await.unwrap();
        let mut bld = client.describe_log_groups();
        if next_token.is_some() {
            bld = bld.next_token(next_token.unwrap());
        }
        let log_groups_output = bld.send().await.unwrap();
        next_token = log_groups_output.next_token;
        // get all log group names sorted by alphabetical
        let mut log_group_names: Vec<String> = log_groups_output.log_groups
            .unwrap()
            .into_iter()
            .map(|group| group.log_group_name.unwrap())
            .collect();
        all_group_names.append(&mut log_group_names);
        if next_token.is_none() {
            break;
        }
        i += 1;
        if i > max_iters {
            return Err("max iterations exceeded".to_string());
        }
    }
    all_group_names.sort();
    Ok(all_group_names)
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
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
        let preview_requested = args.preview_lines > 0;
        if preview_requested {
            // get the first N lines of the last 20 log streams
            let preview_streams = args.preview_streams;
            if preview_streams == 0 {
                println!("--preview-streams must be greater than 0");
                return;
            }
            let preview_event_count = args.preview_lines;
            let max_preview_events = 200;
            if preview_event_count > max_preview_events {
                println!("Preview amount cannot be greater than {max_preview_events}");
                return;
            }
            let preview_log_stream_names = log_stream_names
                .iter()
                .rev()
                .take(preview_streams as usize)
                .map(|s| s.as_str())
                .collect::<Vec<&str>>();
            let mut preview_futures = vec![];
            for log_stream_name in preview_log_stream_names.clone() {
                let future = fetch_first_n_events(client, &log_group, log_stream_name, preview_event_count as i32);
                preview_futures.push(future);
            }
            let fut_results = futures::future::join_all(preview_futures).await;

            for (i, fut_result) in fut_results.into_iter().enumerate() {
                let log_stream_name = preview_log_stream_names[i];
                let events = fut_result;
                let text = get_text_from_events(&events);
                logstream_previews.insert(log_stream_name.to_string(), text);
            }

        }
        println!("Log Streams (log group: {log_group}):");
        for name in log_stream_names {
            if preview_requested {
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
    let events: Vec<Event> = fetch_entire_log(client, &log_group, &log_stream).await;
    let full_log_text = get_text_from_events(&events);

    if let Some(fpath) = args.output_file {
        let error_msg = format!("Unable to write file: {fpath}");
        info!("writing to file: {fpath}");
        std::fs::write(&fpath, full_log_text).expect(&error_msg);
    } else {
        println!("FULL LOG TEXT:\n{full_log_text}");
    }
}
