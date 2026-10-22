//! AC02：只编译进 acceptance 构建的受控地址与顺序取证。

use std::borrow::Cow;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

#[derive(Debug, PartialEq, Eq)]
struct Configuration {
    root: Option<PathBuf>,
    pricing_base: Option<String>,
    record_opener: bool,
}

struct Runtime {
    configuration: Configuration,
    writer: Mutex<TraceWriter>,
}

struct TraceWriter {
    file: Option<File>,
    sequence: u64,
    offset_ms: u64,
    started: Instant,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn environment(key: &str) -> Result<Option<String>> {
    match std::env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("{key} 必须是 UTF-8")),
    }
}

fn validate_base(raw: &str) -> Result<String> {
    let url = url::Url::parse(raw).context("TOKENSCOPE_ACCEPTANCE_PRICING_BASE 非法")?;
    let explicit_port = raw
        .strip_prefix("http://127.0.0.1:")
        .and_then(|text| text.trim_end_matches('/').parse::<u16>().ok());
    ensure!(
        raw.starts_with("http://127.0.0.1:")
            && url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && explicit_port.is_some_and(|port| port != 0)
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "TOKENSCOPE_ACCEPTANCE_PRICING_BASE 只接受 http://127.0.0.1:<port>"
    );
    Ok(raw.trim_end_matches('/').to_owned())
}

/// 由 acceptance::bootstrap 的互斥区调用，根与所有配置共同冻结。
pub(super) fn bootstrap(root: Option<&Path>) -> Result<()> {
    let pricing_base = environment("TOKENSCOPE_ACCEPTANCE_PRICING_BASE")?
        .map(|value| validate_base(&value))
        .transpose()?;
    let record_opener = match environment("TOKENSCOPE_ACCEPTANCE_OPENER")?.as_deref() {
        None => false,
        Some("record") => true,
        Some(_) => bail!("TOKENSCOPE_ACCEPTANCE_OPENER 只接受 record"),
    };
    let configuration = Configuration {
        root: root.map(Path::to_path_buf),
        pricing_base,
        record_opener,
    };
    if let Some(previous) = RUNTIME.get() {
        ensure!(
            previous.configuration == configuration,
            "验收根与追踪配置已冻结，拒绝运行中改写"
        );
        return Ok(());
    }
    let mut sequence = 0;
    let mut offset_ms = 0;
    if let Some(root) = root {
        let path = root.join("acceptance-trace.jsonl");
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                ensure!(
                    text.is_empty() || text.ends_with('\n'),
                    "已有验收追踪缺少末尾换行，拒绝追加"
                );
                for line in text.lines() {
                    let record: Value = serde_json::from_str(line)
                        .context("已有验收追踪不是完整 JSONL，拒绝覆盖")?;
                    let next = record["seq"].as_u64().context("验收追踪缺少 seq")?;
                    let milliseconds = record["monotonic_ms"]
                        .as_u64()
                        .context("验收追踪缺少单调时间")?;
                    ensure!(
                        next == sequence + 1 && milliseconds >= offset_ms,
                        "已有验收追踪顺序损坏，拒绝覆盖"
                    );
                    sequence = next;
                    offset_ms = milliseconds;
                }
                offset_ms = offset_ms.saturating_add(1);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("读取验收追踪失败"),
        }
    }
    RUNTIME
        .set(Runtime {
            configuration,
            writer: Mutex::new(TraceWriter {
                file: None,
                sequence,
                offset_ms,
                started: Instant::now(),
            }),
        })
        .map_err(|_| anyhow::anyhow!("验收追踪配置并发初始化"))?;
    Ok(())
}

pub(super) fn pricing_url(endpoint: &'static str) -> Result<Cow<'static, str>> {
    let state = RUNTIME.get().context("价格验收必须先 bootstrap")?;
    ensure!(state.configuration.root.is_some(), "价格验收缺少隔离根");
    ensure!(
        matches!(
            endpoint,
            "/modelsdev/api.json" | "/openrouter/api/v1/models"
        ),
        "未知验收价格端点"
    );
    let base = state
        .configuration
        .pricing_base
        .as_deref()
        .context("验收构建同步价格必须设置 TOKENSCOPE_ACCEPTANCE_PRICING_BASE；拒绝回退外网")?;
    Ok(Cow::Owned(format!("{base}{endpoint}")))
}

pub(super) fn record(event: &str, fields: Value) -> Result<()> {
    let Some(state) = RUNTIME
        .get()
        .filter(|state| state.configuration.root.is_some())
    else {
        return Ok(());
    };
    let root = state.configuration.root.as_ref().unwrap();
    let mut writer = state
        .writer
        .lock()
        .map_err(|_| anyhow::anyhow!("验收追踪写锁已损坏"))?;
    let mut record = fields.as_object().cloned().unwrap_or_default();
    let sequence = writer.sequence + 1;
    let milliseconds = writer
        .offset_ms
        .saturating_add(u64::try_from(writer.started.elapsed().as_millis()).unwrap_or(u64::MAX));
    record.insert("seq".to_owned(), json!(sequence));
    record.insert("monotonic_ms".to_owned(), json!(milliseconds));
    record.insert("utc".to_owned(), json!(jiff::Timestamp::now().to_string()));
    record.insert("event".to_owned(), json!(event));
    let mut line = serde_json::to_vec(&record)?;
    line.push(b'\n');
    if writer.file.is_none() {
        writer.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join("acceptance-trace.jsonl"))
                .context("打开验收追踪失败")?,
        );
    }
    let file = writer.file.as_mut().unwrap();
    file.write_all(&line).context("写入验收追踪失败")?;
    file.flush().context("flush 验收追踪失败")?;
    writer.sequence = sequence;
    Ok(())
}

fn normalized_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    loop {
        if let Ok(canonical) = ancestor.canonicalize() {
            let text = canonical.to_string_lossy();
            let mut result = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
                PathBuf::from(format!(r"\\{rest}"))
            } else {
                PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
            };
            for part in suffix.into_iter().rev() {
                result.push(part);
            }
            return Ok(result);
        }
        suffix.push(
            ancestor
                .file_name()
                .context("验收路径无法解析")?
                .to_os_string(),
        );
        ancestor = ancestor.parent().context("验收路径没有可解析的父目录")?;
    }
}

pub(super) fn record_path(event: &str, path: &Path, fields: Value) -> Result<()> {
    let Some(root) = RUNTIME
        .get()
        .and_then(|state| state.configuration.root.as_ref())
    else {
        return Ok(());
    };
    let path = normalized_path(path)?;
    let mut detail = fields.as_object().cloned().unwrap_or_default();
    detail.insert("path".to_owned(), json!(path));
    record(event, Value::Object(detail))?;
    ensure!(
        path.starts_with(root),
        "验收追踪发现隔离根外路径：{}",
        path.display()
    );
    Ok(())
}

/// FNV-1a 64 只用作落盘字节的变更指纹；算法随证据写明。
/// 发布资产及二进制身份仍由发布工具的 SHA256 校验。
fn settings_hash(bytes: &[u8]) -> String {
    let value = bytes.iter().fold(0xcbf29ce484222325_u64, |value, byte| {
        (value ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{value:016x}")
}

pub(super) fn consent_saved(path: &Path) -> Result<()> {
    if RUNTIME
        .get()
        .is_none_or(|state| state.configuration.root.is_none())
    {
        return Ok(());
    }
    let bytes = std::fs::read(path).context("读取已落盘同意记录的指纹失败")?;
    record_path(
        "consent_saved",
        path,
        json!({"settings_hash": {
            "algorithm":"fnv1a64", "value":settings_hash(&bytes)
        }}),
    )
}

pub(super) fn record_opener(path: &Path) -> Result<bool> {
    let state = RUNTIME.get().context("验收 opener 必须先 bootstrap")?;
    ensure!(state.configuration.root.is_some(), "验收 opener 缺少隔离根");
    if !state.configuration.record_opener {
        return Ok(false);
    }
    ensure!(path.is_file(), "验收 opener 必须在文件创建或确认之后记录");
    record_path("opener", path, json!({}))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn consent_file_fingerprint_has_an_explicit_stable_algorithm() {
        assert_eq!(super::settings_hash(b""), "cbf29ce484222325");
        assert_eq!(super::settings_hash(b"hello"), "a430d84680aabd0b");
    }
}
