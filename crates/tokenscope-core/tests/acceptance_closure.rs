//! AC02：验收接线使用独立子进程，避免冻结状态与环境变量污染其他用例。
#![cfg(feature = "acceptance")]

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "tokenscope-ac02-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let canonical = root.canonicalize().unwrap();
    let text = canonical.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

fn child(name: &str, root: Option<&Path>) -> Output {
    configured_child(name, root, None, None)
}

fn configured_child(
    name: &str,
    root: Option<&Path>,
    base: Option<&str>,
    proxy: Option<&str>,
) -> Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", name, "--ignored", "--nocapture"]);
    for key in [
        "TOKENSCOPE_ACCEPTANCE_ROOT",
        "TOKENSCOPE_ACCEPTANCE_PRICING_BASE",
        "TOKENSCOPE_ACCEPTANCE_OPENER",
        "TOKENSCOPE_REAL_PERF",
        "TOKENSCOPE_ACCEPTANCE_COLLECT_DELAY_MS",
        "TOKENSCOPE_ACCEPTANCE_COLLECT_FAIL_ON_ROUND",
    ] {
        command.env_remove(key);
    }
    if let Some(root) = root {
        command.env("TOKENSCOPE_ACCEPTANCE_ROOT", root);
    }
    if let Some(base) = base {
        command.env("TOKENSCOPE_ACCEPTANCE_PRICING_BASE", base);
    }
    command.env("TOKENSCOPE_ACCEPTANCE_OPENER", "record");
    if let Some(proxy) = proxy {
        for key in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "http_proxy",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            command.env(key, proxy);
        }
        command.env("NO_PROXY", "127.0.0.1,localhost");
        command.env("no_proxy", "127.0.0.1,localhost");
    }
    command.output().expect("启动 AC02 隔离子进程")
}

/// 只监听回环地址。代理模式永远拒绝请求，不向任何目标转发。
struct LocalHttp {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl LocalHttp {
    fn start(proxy: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            for incoming in listener.incoming() {
                let mut stream = incoming.unwrap();
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                observed.lock().unwrap().push(request.trim().to_owned());
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap() == 0 || header == "\r\n" {
                        break;
                    }
                }
                let endpoint = request.split_whitespace().nth(1).unwrap_or("");
                let (status, body) = if proxy {
                    ("403 Forbidden", serde_json::json!({"blocked": true}))
                } else if endpoint == "/modelsdev/api.json" {
                    (
                        "200 OK",
                        serde_json::json!({"anthropic":{"models":{"claude-opus-5.5":{
                            "name":"Synthetic Opus", "cost":{"input":5.0,"output":25.0,"cache_read":0.5,"cache_write":6.25}
                        }}}}),
                    )
                } else if endpoint == "/openrouter/api/v1/models" {
                    (
                        "200 OK",
                        serde_json::json!({"data":[{"id":"anthropic/claude-opus-5.5",
                        "name":"Synthetic Opus", "pricing":{"prompt":"0.000005","completion":"0.000025",
                        "input_cache_read":"0.0000005","input_cache_write":"0.00000625"}}]}),
                    )
                } else {
                    ("404 Not Found", serde_json::json!({"missing":endpoint}))
                };
                let body = serde_json::to_vec(&body).unwrap();
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        Self {
            address,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    fn base(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for LocalHttp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.address, std::time::Duration::from_secs(2));
        if let Some(worker) = self.worker.take()
            && let Err(error) = worker.join()
        {
            if std::thread::panicking() {
                eprintln!("本地 HTTP fixture 线程失败：{error:?}");
            } else {
                panic!("本地 HTTP fixture 线程失败：{error:?}");
            }
        }
    }
}

#[test]
fn acceptance_prices_and_io_trace_are_local_and_ordered() {
    let root = isolated_root("trace");
    let prices = LocalHttp::start(false);
    let proxy = LocalHttp::start(true);
    let output = configured_child(
        "closure_child_prices_and_trace",
        Some(&root),
        Some(&prices.base()),
        Some(&proxy.base()),
    );
    let external = proxy.requests.lock().unwrap().clone();
    assert!(external.is_empty(), "外部连接尝试必须为零：{external:?}");
    assert_child_passed(output);
    let hits = prices.requests.lock().unwrap().clone();
    assert_eq!(
        hits.iter()
            .filter(|line| line.contains("/modelsdev/api.json "))
            .count(),
        1
    );
    assert_eq!(
        hits.iter()
            .filter(|line| line.contains("/openrouter/api/v1/models "))
            .count(),
        1
    );
    assert_eq!(hits.len(), 2);
    let records: Vec<serde_json::Value> =
        std::fs::read_to_string(root.join("acceptance-trace.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert!(!records.is_empty(), "实际 I/O 必须有追踪记录");
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record["seq"].as_u64(), Some(index as u64 + 1));
        assert!(
            record["utc"]
                .as_str()
                .is_some_and(|value| value.ends_with('Z'))
        );
        let monotonic = record["monotonic_ms"]
            .as_u64()
            .expect("单调时间必须是实测整数");
        if index > 0 {
            assert!(monotonic >= records[index - 1]["monotonic_ms"].as_u64().unwrap());
        }
        if let Some(path) = record["path"].as_str() {
            assert!(Path::new(path).starts_with(&root), "隔离根外追踪：{record}");
        }
    }
    for event in [
        "collect_round_start",
        "source_root_read",
        "history_open",
        "ccs_open",
        "pricing_fetch",
        "settings_write",
    ] {
        assert!(
            records.iter().any(|record| record["event"] == event),
            "缺少真实接线事件 {event}: {records:?}"
        );
    }
    println!(
        "AC02_TRACE_OBSERVED {}",
        serde_json::json!({"records":records,"price_hits":hits,"external_attempts":0})
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_acceptance_price_addresses_reject_bootstrap_without_network() {
    for base in [
        "https://127.0.0.1:1234",
        "http://example.invalid:1234",
        "http://127.0.0.1:0",
        "http://127.0.0.1:1234/path",
        "not-a-url",
    ] {
        let root = isolated_root("invalid-base");
        assert_child_passed(configured_child(
            "closure_child_invalid_price_base",
            Some(&root),
            Some(base),
            None,
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
#[ignore = "由父用例以独立进程执行"]
fn closure_child_invalid_price_base() {
    assert!(
        tokenscope::acceptance::bootstrap().is_err(),
        "非法价格地址必须在启动期拒绝"
    );
}

#[test]
#[ignore = "由父用例以独立进程执行"]
fn closure_child_prices_and_trace() {
    tokenscope::acceptance::bootstrap().unwrap();
    let root = tokenscope::acceptance::root().unwrap();
    assert_eq!(
        tokenscope::import::ccs::default_source_path().unwrap(),
        root.join("sources/cc-switch/cc-switch.db")
    );
    let base = std::env::var("TOKENSCOPE_ACCEPTANCE_PRICING_BASE").unwrap();
    // 在实际请求之前核对纯地址 getter，接线错误也不能尝试外网。
    assert_eq!(
        tokenscope::modelsdev::api_url().unwrap(),
        format!("{base}/modelsdev/api.json")
    );
    assert_eq!(
        tokenscope::openrouter::api_url().unwrap(),
        format!("{base}/openrouter/api/v1/models")
    );
    let data = root.join("tokenscope");
    assert_eq!(
        tokenscope::modelsdev::sync(&data.join("pricing-modelsdev.json"))
            .unwrap()
            .count,
        1
    );
    assert_eq!(
        tokenscope::openrouter::sync(&data.join("pricing-openrouter.json"))
            .unwrap()
            .count,
        1
    );
    let source = root.join("sources/claude/projects/synthetic");
    std::fs::create_dir_all(&source).unwrap();
    let event = serde_json::json!({"type":"assistant","timestamp":"2026-10-21T10:00:00Z","sessionId":"ac02",
        "cwd":"C:/acceptance/workspace/test", "message":{"id":"ac02-1","model":"claude-opus-5.5",
        "usage":{"input_tokens":10,"output_tokens":3,"cache_creation_input_tokens":2,"cache_read_input_tokens":1}}});
    std::fs::write(
        source.join("synthetic.jsonl"),
        serde_json::to_vec(&event).unwrap(),
    )
    .unwrap();
    let options = tokenscope::report::SummaryOptions {
        cache_dir: Some(data.clone()),
        history_path: Some(data.join("history.db")),
        pricing_index: Some(data.join("pricing-index.json")),
        pricing_path: Some(data.join("pricing.toml")),
        modelsdev_path: Some(data.join("pricing-modelsdev.json")),
        openrouter_path: Some(data.join("pricing-openrouter.json")),
        source_plans: Some(vec![tokenscope::report::SourcePlan {
            agent: tokenscope::model::AgentKind::ClaudeCode,
            root: root.join("sources/claude/projects"),
            claude_mapping: None,
        }]),
        ..Default::default()
    };
    let report = tokenscope::report::summary(&options).unwrap();
    assert_eq!(report.totals.requests, 1);
    tokenscope::settings::ensure_toml(&data.join("settings.toml")).unwrap();
    let before_ccs: Vec<serde_json::Value> =
        std::fs::read_to_string(root.join("acceptance-trace.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert!(
        !before_ccs
            .iter()
            .any(|record| record["event"] == "ccs_open"),
        "主动打开之前不得读取 CCS"
    );
    let ccs = tokenscope::import::ccs::default_source_path().unwrap();
    std::fs::create_dir_all(ccs.parent().unwrap()).unwrap();
    drop(rusqlite::Connection::open(&ccs).unwrap());
    drop(tokenscope::import::ccs::CcsSource::open(&ccs).unwrap());
}

fn assert_child_passed(output: Output) {
    assert!(
        output.status.success(),
        "AC02 子进程失败 {:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn acceptance_ccs_default_stays_inside_the_bootstrapped_root() {
    let root = isolated_root("ccs-default");
    assert_child_passed(child("closure_child_ccs_default", Some(&root)));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn acceptance_ccs_default_refuses_before_bootstrap() {
    assert_child_passed(child("closure_child_ccs_without_bootstrap", None));
}

#[test]
#[ignore = "由父用例以独立进程执行"]
fn closure_child_ccs_default() {
    tokenscope::acceptance::bootstrap().unwrap();
    let root = tokenscope::acceptance::root().expect("验收根已解析");
    let expected = root.join("sources/cc-switch/cc-switch.db");
    let actual = tokenscope::import::ccs::default_source_path().unwrap();
    // 只比路径，不调用 source_exists 或打开任何 CCS 库。
    assert_eq!(actual, expected, "默认 CCS 路径必须隔离");
    println!(
        "AC02_CCS_DEFAULT {}",
        serde_json::json!({"path": actual, "inside_root": actual.starts_with(&root)})
    );
}

#[test]
#[ignore = "由父用例以独立进程执行"]
fn closure_child_ccs_without_bootstrap() {
    let result = tokenscope::import::ccs::default_source_path();
    assert!(
        result.is_err(),
        "未 bootstrap 的验收构建必须拒绝 CCS 默认路径，不能回退真实目录: {result:?}"
    );
}

#[test]
fn acceptance_rejects_unterminated_trace_before_append() {
    let root = isolated_root("unterminated-trace");
    let previous = serde_json::json!({"seq":1,"monotonic_ms":0,"utc":"2026-10-22T00:00:00Z","event":"business_ready"});
    let bytes = serde_json::to_vec(&previous).unwrap();
    std::fs::write(root.join("acceptance-trace.jsonl"), &bytes).unwrap();
    assert_child_passed(child("closure_child_unterminated_trace", Some(&root)));
    assert_eq!(
        std::fs::read(root.join("acceptance-trace.jsonl")).unwrap(),
        bytes
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "由父用例以独立进程执行"]
fn closure_child_unterminated_trace() {
    let error = tokenscope::acceptance::bootstrap().expect_err("没有换行的旧追踪不得继续追加");
    assert!(error.contains("换行"), "必须明确说明不完整行边界：{error}");
}

#[test]
fn acceptance_trace_continues_across_process_restarts() {
    let root = isolated_root("trace-restart");
    for _ in 0..2 {
        assert_child_passed(child("closure_child_append_trace", Some(&root)));
    }
    let records: Vec<serde_json::Value> =
        std::fs::read_to_string(root.join("acceptance-trace.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["seq"], 1);
    assert_eq!(records[1]["seq"], 2);
    assert!(
        records[1]["monotonic_ms"].as_u64().unwrap()
            >= records[0]["monotonic_ms"].as_u64().unwrap()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "由父用例以独立进程执行"]
fn closure_child_append_trace() {
    tokenscope::acceptance::bootstrap().unwrap();
    tokenscope::acceptance::record(
        "business_ready",
        serde_json::json!({"scope":"unit_trace_restart"}),
    )
    .unwrap();
}
