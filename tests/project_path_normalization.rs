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
fn file_uri_rejects_dot_segments_and_nul() {
    // A01 修订（审查）：URI 路径同样受"拒绝 . / .. 组件、不做折叠"的约束——
    // URL 解析器会静默折叠点段，所以必须在交给解析器之前自行检查。
    assert!(norm("file:///C:/a/../b").is_none(), "不得折叠 .. 后接受");
    assert!(norm("file:///C:/a/./b").is_none(), "不得折叠 . 后接受");
    assert!(
        norm("file:///C:/a/%2e%2e/b").is_none(),
        "编码后的点段同样拒绝"
    );
    assert!(norm("file:///server/share/../x").is_none(), "UNC 形态同理");
    // 解码后含 NUL（空字符）不是可安全解释的路径。
    assert!(norm("file:///C:/a%00b").is_none(), "解码后含 NUL 必须拒绝");
    assert!(norm("file:///C:/a%00").is_none());
    assert!(norm("file:///C:/%00").is_none());
}

#[test]
fn file_uri_and_plain_path_share_identity_for_spaces() {
    // 合法空格（%20）必须可用：URI 解码一次，普通路径按字面——两者同一身份。
    assert_eq!(
        norm("file:///C:/我的项目/demo%20app").as_deref(),
        Some("C:/我的项目/demo app")
    );
    assert_eq!(
        norm(r"C:\我的项目\demo app").as_deref(),
        Some("C:/我的项目/demo app")
    );
    assert_eq!(
        norm("file:///C:/我的项目/demo%20app"),
        norm(r"C:\我的项目\demo app")
    );
    // 普通路径里的 `%20` 是字面值（不解码）；URI 里的 `%2520` 只解码一次。
    assert_eq!(
        norm(r"C:\我的项目\demo%20app").as_deref(),
        Some("C:/我的项目/demo%20app")
    );
    assert_eq!(
        norm("file:///C:/我的项目/demo%2520app").as_deref(),
        Some("C:/我的项目/demo%20app")
    );
    assert_ne!(
        norm(r"C:\我的项目\demo%20app"),
        norm(r"C:\我的项目\demo app"),
        "字面 %20 与真实空格不是同一路径"
    );
}

#[test]
fn posix_trailing_space_is_preserved() {
    // 第三轮审查：尾空格是合法文件名字符——`/tmp/alpha ` 与 `/tmp/alpha`
    // 是两个不同目录，不得用整体 trim 误合并。
    assert_eq!(norm("/tmp/alpha ").as_deref(), Some("/tmp/alpha "));
    assert_ne!(norm("/tmp/alpha "), norm("/tmp/alpha"));
    assert_eq!(norm("/tmp/al pha").as_deref(), Some("/tmp/al pha"));
    // 全空白仍不可用。
    assert!(norm("   ").is_none());
    assert!(norm("").is_none());
}

#[test]
fn unc_and_file_uri_agree_on_host_case() {
    // 第三轮审查：URI 主机解析恒为小写；直接 UNC 路径必须同口径，
    // 否则同一台机器上的同一路径被拆成两个身份。
    assert_eq!(
        norm(r"\\Server\Share\Dir").as_deref(),
        Some("//server/Share/Dir")
    );
    assert_eq!(
        norm("file://Server/Share/Dir").as_deref(),
        Some("//server/Share/Dir")
    );
    assert_eq!(norm(r"\\Server\Share\Dir"), norm("file://Server/Share/Dir"));
}

#[test]
fn uri_backslash_handling_matches_path_semantics() {
    // 第三轮审查：字面反斜杠在 file URI 里是分隔符（WHATWG 归一）——其中的
    // `..` 必须拒绝（原实现在没有 `/` 的 authority 后返回空路径，绕过检查）。
    assert!(norm(r"file://server\share\a\..\b").is_none());
    assert!(norm(r"file:///C:\a\..\b").is_none());
    // 编码的反斜杠（%5C）是**字面字符**：与普通 POSIX 路径同一身份，
    // 不得被当作分隔符而误拒绝。
    assert_eq!(
        norm("file:///tmp/a%5C..%5Cb").as_deref(),
        Some(r"/tmp/a\..\b")
    );
    assert_eq!(
        norm("file:///tmp/a%5C..%5Cb"),
        norm(r"/tmp/a\..\b"),
        "编码反斜杠与普通路径必须同身份"
    );
    // 编码点段仍然拒绝（大小写两种写法）。
    assert!(norm("file:///C:/a/%2e%2e/b").is_none());
    assert!(norm("file:///C:/a/%2E%2E/b").is_none());
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
