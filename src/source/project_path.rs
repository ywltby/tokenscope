//! A01（跨工具项目路径统一）：项目路径身份归一化契约。
//!
//! 目标：同一规范化绝对路径跨工具得到**同一身份 key**，不可安全解释的
//! 输入返回 `None`（调用方走兜底）——绝不相对 TokenScope 的 cwd 补全，
//! 也不依赖文件系统存在性（历史已删除目录仍是合法身份）。
//!
//! 规则（docs/plans/active/2026-10-09-project-path-unification.md §2）：
//! - Windows 绝对路径：反斜杠统一为 `/`、盘符大写、去尾随分隔符；**不**整体小写；
//! - 根语义保留：`C:/` 不变成 `C:`、POSIX `/` 保持 `/`、UNC 保留主机与共享；
//! - `file://` 走 URI 解析器并**只**做一次百分号解码；普通路径的 `%20` 是字面值；
//! - POSIX 路径保留大小写与合法反斜杠字符；
//! - 相对路径、`C:foo`、非 file scheme、非法百分号编码、`.`/`..` 组件 →
//!   「不可用」（不猜测、不折叠符号链接/别名）。

/// 把日志中出现的原始路径字符串归一为项目身份 key；不可用时返回 `None`。
///
/// 纯函数：不做任何文件系统访问，也不读取环境变量。
pub fn normalize_project_path(raw: &str) -> Option<String> {
    if raw.contains('\0') {
        return None;
    }
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let b = s.as_bytes();
    // 盘符绝对路径（`C:\a` / `c:/a`）：先于 scheme 判定——`C:` 单字符 scheme
    // 的歧义输入（`C:foo`、`C:`）在此明确拒绝。
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return match b.get(2) {
            Some(b'/') | Some(b'\\') => normalize_windows(s),
            _ => None,
        };
    }
    // UNC / 网络路径：两个分隔符开头（Windows 与 POSIX 写法都接受）。
    if s.starts_with("\\\\") || s.starts_with("//") {
        return normalize_unc(s);
    }
    if s.starts_with('/') {
        return normalize_posix(s);
    }
    match uri_scheme(s) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => from_file_uri(s),
        _ => None,
    }
}

/// RFC 3986 scheme：`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )` 后接 `:`。
fn uri_scheme(s: &str) -> Option<&str> {
    let colon = s.find(':')?;
    let scheme = &s[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    Some(scheme)
}

fn from_file_uri(s: &str) -> Option<String> {
    let scheme_len = uri_scheme(s)?.len();
    // 路径必须是 URI 内的绝对路径：`file:` 与 `file:relative` 都不可用。
    let after = s.get(scheme_len + 1..)?;
    if !after.starts_with('/') {
        return None;
    }
    let url = url::Url::parse(s).ok()?;
    if !url.scheme().eq_ignore_ascii_case("file") {
        return None;
    }
    // 带 query / fragment 的 file URI 不是可解释的目录身份。
    if url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    // 仅对 URI 做一次百分号解码；非法序列（`%zz`、截断的 `%2`）判为不可用。
    let path = percent_decode_once(url.path())?;
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    if host.is_empty() || host == "localhost" {
        return local_from_uri_path(&path);
    }
    normalize_unc(&format!("//{host}{path}"))
}

/// 本地 file URI 路径：`/C:/a` → Windows 身份；其余 POSIX 绝对路径。
fn local_from_uri_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix('/')?;
    let b = rest.as_bytes();
    if b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        // `file:///C:` = 盘符根。
        return Some(format!("{}:/", (b[0] as char).to_ascii_uppercase()));
    }
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return normalize_windows(rest);
    }
    normalize_posix(path)
}

/// 一次百分号解码（`%2520` → `%20`）；非法序列或非 UTF-8 结果返回 `None`。
fn percent_decode_once(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = hex_val(*bytes.get(i + 1)?)?;
            let lo = hex_val(*bytes.get(i + 2)?)?;
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Windows 绝对路径：盘符大写 + 分隔符统一 `/` + 去尾随分隔符。
fn normalize_windows(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' {
        return None;
    }
    if b[2] != b'/' && b[2] != b'\\' {
        return None;
    }
    let drive = (b[0] as char).to_ascii_uppercase();
    let parts = split_components(&s[2..], true)?;
    let mut out = String::with_capacity(s.len() + 1);
    out.push(drive);
    out.push(':');
    if parts.is_empty() {
        out.push('/');
    } else {
        for p in parts {
            out.push('/');
            out.push_str(p);
        }
    }
    Some(out)
}

/// POSIX 绝对路径：只按 `/` 切分，反斜杠是合法文件名字符，大小写保留。
fn normalize_posix(s: &str) -> Option<String> {
    let parts = split_components(s, false)?;
    if parts.is_empty() {
        return Some("/".to_string());
    }
    let mut out = String::with_capacity(s.len());
    for p in parts {
        out.push('/');
        out.push_str(p);
    }
    Some(out)
}

/// UNC / 网络路径：保留主机与共享段；缺共享、设备路径（`\\?\`、`\\.\`）不可用。
fn normalize_unc(s: &str) -> Option<String> {
    let rest = s.strip_prefix("\\\\").or_else(|| s.strip_prefix("//"))?;
    let parts = split_components(rest, true)?;
    if parts.len() < 2 {
        return None;
    }
    let (host, share) = (parts[0], parts[1]);
    if host == "?" || host == "." {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    out.push_str("//");
    out.push_str(host);
    out.push('/');
    out.push_str(share);
    for p in &parts[2..] {
        out.push('/');
        out.push_str(p);
    }
    Some(out)
}

/// 组件切分：跳过空组件（重复/尾随分隔符），拒绝 `.` 与 `..`
///（无法在无文件系统访问的前提下安全解释——含符号链接语义）。
fn split_components(s: &str, allow_backslash: bool) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    for part in s.split(|c: char| c == '/' || (allow_backslash && c == '\\')) {
        if part.is_empty() {
            continue;
        }
        if part == "." || part == ".." {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_components_skips_empty_and_rejects_dots() {
        assert_eq!(split_components("/a//b/", false).unwrap(), ["a", "b"]);
        assert!(split_components("/a/../b", false).is_none());
        assert!(split_components(r"a\.\b", true).is_none());
    }

    #[test]
    fn scheme_detection_requires_alpha_prefix() {
        assert_eq!(uri_scheme("file:///a"), Some("file"));
        assert_eq!(uri_scheme("1http://a"), None);
        assert_eq!(uri_scheme("no-colon"), None);
    }
}
