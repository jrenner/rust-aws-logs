# Rust AWS Logs Reader

tool for grabbing full log files from AWS written in Rust

# Compile + Run
```
cargo build --release
cp target/release/alog <place you want the bin>
```


### CLI args
```
❯ alog --help
Usage: alog [OPTIONS]

Options:
      --describe-log-groups
          list log groups in this AWS account
      --describe-log-streams
          list log streams in this log group
  -s, --log-stream <LOG_STREAM>
          log stream to fetch contents of
  -g, --log-group <LOG_GROUP>
          log group
  -o, --output-file <OUTPUT_FILE>
          output file to write to
      --preview-lines <PREVIEW_LINES>
          get previews of the log streams when listing log groups, up to N events [default: 0]
      --preview-streams <PREVIEW_STREAMS>
          get previews of the log streams when listing log groups, up to N most recent streams [default: 0]
  -t, --tail <TAIL>
          view just the last N lines
  -h, --help
          Print help
  -V, --version
          Print version
```


### Usage

Make sure you have your AWS_PROFILE set for the correct account

list log groups
```
❯ alog --describe-log-groups
Log Groups:
/aws/batch/job
/aws/containerinsights/optos-v2-k8s-cods-test/application
/aws/containerinsights/optos-v2-k8s-cods-test/dataplane
/aws/containerinsights/optos-v2-k8s-cods-test/host
...
```

list log streams in log group
```
❯ alog -g /ecs/batte-backcast-dev --describe-log-streams
Log Streams (log group: /ecs/batte-backcast-dev):
ecs/batte-backcast-dev/b741215fa98a4ea3b538c5cf6c85177c
ecs/batte-backcast-dev/4f18e6c41b064f519aa85192642d9dc0
ecs/batte-backcast-dev/50083c81a20b4b05b7429b8ee885745d
...
```


list log streams in log group, preview first X lines from last Y log stream
```
❯ alog -g /ecs/batte-backcast-dev --describe-log-streams --preview-lines 3 --preview-streams 2
...

------------------
ecs/batte-backcast-dev/38267cdab57e4bb9bf6ee57a3bc63472
PREVIEW:
BATTE_SITE: BTH1
Wed Mar 13 16:28:28 UTC 2024
BATTE_OVERRIDE_CONFIG: {"backcast":{"sites":["BTH1"]},"data":{"num_workers":20,"period":["*-3h","*-1h"]}}

------------------
ecs/batte-backcast-dev/c0d3642763b44faebbb0b53810113789
PREVIEW:
BATTE_SITE: BRE2
Wed Mar 13 16:28:36 UTC 2024
BATTE_OVERRIDE_CONFIG: {"backcast":{"sites":["BRE2"]},"data":{"num_workers":20,"period":["*-3h","*-1h"]}}
```

get the full output from a specific log stream
```
❯ alog -g /ecs/batte-backcast-dev -s ecs/batte-backcast-dev/f3564f40ea4a447da8a929a527748f72

❯ alog -g /ecs/batte-backcast-dev -s ecs/batte-backcast-dev/38267cdab57e4bb9bf6ee57a3bc63472 | head -20
FULL LOG TEXT:
BATTE_SITE: BTH1
Wed Mar 13 16:28:28 UTC 2024
BATTE_OVERRIDE_CONFIG: {"backcast":{"sites":["BTH1"]},"data":{"num_workers":20,"period":["*-3h","*-1h"]}}
[STEP 1] run backcast_db_record.py
[I 240313 16:28:29 backcast_db_record:66] created backcast record for site_id: 11, backcast id: 17544
[I 240313 16:28:29 backcast_db_record:41] persisted backcast_id: 17544 to disk
...
```

