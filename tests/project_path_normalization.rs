//! A01（跨工具项目路径统一）：`normalize_project_path` 的纯函数契约。
//!
//! 覆盖四类必须成立的行为：Windows 等价形式共用同一身份 key、`file://`
//! URI 只做一次百分号解码、根路径与 UNC 保留语义、POSIX 保留大小写与
//! 反斜杠字面；歧义输入一律返回「不可用」（None）由调用方走兜底。

use tokenscope::source::project_path::normalize_project_path;

fn norm(raw: &str) -> Option<String> {
    normalize_project_path(raw)
}

#[test]
fn windows_equivalent_paths_share_key() {
    for raw in [
        r"C:\a\b", "c:/a/b/", "C:/a/b", r"C:\a\b\", "C:/a//b", r"C:/a\/b",
    ] {
        assert_eq!(norm(raw).as_deref(), Some("C:/a/b"), "输入 {raw:?}");
    }
    // 只折叠分隔符与盘符大小写：不整体小写（Windows 大小写不敏感不代表身份键可猜）。
    assert_eq!(norm(r"C:\A\b").as_deref(), Some("C:/A/b"));
    assert_ne!(norm(r"C:\A\b"), norm(r"C:\a\b"));
    // 中文与空格按字符保留；普通路径里的 %20 是字面值，不做解码。
    assert_eq!(norm(r"C:\项目\a b\").as_deref(), Some("C:/项目/a b"));
    assert_eq!(
        norm(r"C:\项目\%20a").as_deref(),
        Some("C:/项目/%20a"),
        "普通路径里的 %20 必须保留字面值"
    );
}

#[test]
fn file_uri_decodes_once() {
    assert_eq!(norm("file:///C:/a%20b/c").as_deref(), Some("C:/a b/c"));
    assert_eq!(
        norm("file:///C:/a%2520b").as_deref(),
        Some("C:/a%20b"),
        "URI 只做一次百分号解码"
    );
    assert_eq!(
        norm("file:///home/user/proj/").as_deref(),
        Some("/home/user/proj")
    );
    assert_eq!(
        norm("file://server/share/dir").as_deref(),
        Some("//server/share/dir"),
        "非 localhost 主机按 UNC 保留主机与共享"
    );
    assert_eq!(
        norm("file://localhost/home/user").as_deref(),
        Some("/home/user")
    );
    assert_eq!(norm("file:///c:/a").as_deref(), Some("C:/a"));
}

#[test]
fn roots_and_unc_are_preserved() {
    assert_eq!(norm(r"C:\").as_deref(), Some("C:/"), "根目录不能变成 C:");
    assert_eq!(norm("c:/").as_deref(), Some("C:/"));
    assert_eq!(norm("/").as_deref(), Some("/"));
    assert_eq!(norm(r"\\server\share\").as_deref(), Some("//server/share"));
    assert_eq!(
        norm("//server/share/dir/").as_deref(),
        Some("//server/share/dir")
    );
    assert_eq!(
        norm(r"\\server\share\dir").as_deref(),
        Some("//server/share/dir")
    );
}

#[test]
fn posix_case_and_backslash_are_preserved() {
    assert_eq!(norm("/Home/User/Proj").as_deref(), Some("/Home/User/Proj"));
    assert_eq!(
        norm(r"/Home/User/a\b").as_deref(),
        Some(r"/Home/User/a\b"),
        "POSIX 路径里的反斜杠是合法文件名字符"
    );
    assert_eq!(norm("/a//b/").as_deref(), Some("/a/b"));
    assert_ne!(
        norm("/home/user"),
        norm(r"C:\home\user"),
        "POSIX 与 Windows 路径是两个身份"
    );
}

#[test]
fn ambiguous_paths_are_rejected() {
    for raw in [
        "",
        "   ",
        "a/b",
        r"a\b",
        r"..\a",
        "C:foo",
        "C:",
        r"\\server",
        r"\\server\",
        r"\a\b",
        "http://example.com/x",
        "mailto:a@b",
        r"\\?\C:\a",
        r"\\.\pipe\x",
        r"C:\a\..\b",
        r"C:\a\.\b",
        "/a/../b",
        "file:///C:/a%zz",
        "file:///C:/a%2",
        "file:///C:/a?q=1",
        "file:///C:/a#frag",
        "file:",
        "C:/a/\u{0}b",
    ] {
        assert!(norm(raw).is_none(), "必须判定为不可用: {raw:?}");
    }
}
