//! 应用设置：`~/.tokenscope/settings.toml`（带注释、可手编的高级配置文件）。
//!
//! 迁移（2026-10-06）：原 `settings.json` 一次性导入——toml 缺失时读取同目录
//! 遗留 json（只读），`save()` 成功写出 toml 后将其改名 `settings.json.bak`
//! 保底（改名失败只延迟迁移，toml 在位后 load 不再读 json）。
//! 容忍策略：文件缺失/坏内容 → 报错或回退默认值；未知字段忽略，向前兼容。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// 设置文件格式修订号（UP03，当前 2）。
///
/// **只在"需要声明迁移"或"改变旧设置的解释语义"时递增**，且必须同步
/// `scripts/settings_migrations.json` 的迁移规则；**新增普通设置字段不算**
/// ——未知字段本就忽略、缺失字段回默认值，向前兼容不靠这个数字。
/// 该值经 [`crate::persistent_format::persistent_format`] 进入
/// `build-metadata.json`，升级矩阵按格式分组挑起点时读的就是它。
///
/// v2（PR3，2026-10-18）：来源配置新增 `auto_discover` / `excluded_auto_paths` /
/// `additional_paths`；旧 `dir` 字段改为追加项语义并在升级时由
/// [`migrate_source_paths`] 一次性迁移——这是**读取范围的行为变更**（已启用来源
/// 的默认目录用量也会纳入），不是无影响的字段改名。
pub const SETTINGS_FORMAT_REVISION: u8 = 2;

/// 关闭窗口按钮的默认动作（None = 每次询问；托盘「退出」不受此影响）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloseAction {
    /// 最小化到托盘（窗口隐藏，进程驻留）。
    Minimize,
    /// 直接退出进程。
    Quit,
}

/// 手动追加的日志根（v2）：用户明确指定的绝对目录，带独立启停。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdditionalSourcePath {
    /// 绝对日志根；落盘保存用户输入原样，身份规范化（canonicalize 等）在发现层完成。
    pub path: String,
    /// 停用后该追加项不参与采集（历史库不删数据）。
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// 单一来源配置（C1 稳定 ID = 结构字段 claude/codex；PR3 v2 起支持自动发现与追加项）。
///
/// 解释规则（§9.2 第 2 条）：任一新格式字段出现即按新配置解释，旧 `dir` 一律
/// **忽略**、不再补入追加项；只有"有 `dir` 且无任何新字段"的未迁移旧格式仍按
/// 旧语义解释（`dir` 取代默认根）——这是迁移失败窗口期不扩大读取范围的保证。
/// 字段缺省的默认值：`auto_discover = true`、两个列表为空。
#[derive(Debug, Clone, PartialEq)]
pub struct SourceConfig {
    /// 来源总开关：停用后该来源完全不采集（优先级高于自动/追加项开关）。
    /// 反序列化缺省 true（见下方手动 impl）。
    pub enabled: bool,
    /// 旧格式（v1）遗留字段：显式目录。仅在未迁移旧格式上参与解释；迁移成功后
    /// 不再写出（迁移失败的窗口期原样保留，保证原文件字节与语义不变）。
    pub dir: Option<String>,
    /// 自动发现（默认根 + 环境变量根）；缺省 true。停用项持久化于
    /// `excluded_auto_paths`，探测/重启不能重启用。
    pub auto_discover: bool,
    /// 已停用的自动候选绝对路径（用同一规范化函数匹配，不保存探测结果列表）。
    pub excluded_auto_paths: Vec<String>,
    /// 手动追加的日志根。
    pub additional_paths: Vec<AdditionalSourcePath>,
    /// 反序列化时记录：是否为未迁移旧格式（有 `dir` 且无任何新字段）。不落盘。
    pub legacy_format: bool,
}

impl SourceConfig {
    /// 读取侧解释：未迁移旧格式（v1 dir）仍按旧语义——`dir` 取代默认根；
    /// 新格式忽略 `dir`。迁移失败或未运行时，采集不得借新字段扩大读取范围。
    pub fn is_legacy_dir_semantics(&self) -> bool {
        self.legacy_format
    }
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: None,
            auto_discover: true,
            excluded_auto_paths: Vec::new(),
            additional_paths: Vec::new(),
            legacy_format: false,
        }
    }
}

// 自定义反序列化（§9.2 第 2 条）：保留「新字段是否出现」的信息，
// 不先 #[serde(default)] 成空列表再猜新旧格式。
impl<'de> Deserialize<'de> for SourceConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            #[serde(default = "default_true")]
            enabled: bool,
            #[serde(default)]
            dir: Option<String>,
            #[serde(default)]
            auto_discover: Option<bool>,
            #[serde(default)]
            excluded_auto_paths: Option<Vec<String>>,
            #[serde(default)]
            additional_paths: Option<Vec<AdditionalSourcePath>>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let legacy_format = raw.dir.is_some()
            && raw.auto_discover.is_none()
            && raw.excluded_auto_paths.is_none()
            && raw.additional_paths.is_none();
        Ok(Self {
            enabled: raw.enabled,
            dir: raw.dir,
            auto_discover: raw.auto_discover.unwrap_or(true),
            excluded_auto_paths: raw.excluded_auto_paths.unwrap_or_default(),
            additional_paths: raw.additional_paths.unwrap_or_default(),
            legacy_format,
        })
    }
}

// 自定义序列化（与反序列化同一条形状保留规则）：仍是 v1 语义的在位配置
// （旧 dir 文件原样载入、且没有任何新格式内容）**原样写回 v1 两字段**。
// 派生序列化会带出 `auto_discover = true`，把文件变成 dir+auto_discover 的
// 混合体——下次读取按"新字段出现"解释、`dir` 被静默忽略：迁移失败窗口内
// 任何一次无关设置修改（改时区、关价格同步）都会悄悄改变读取范围。
// 一旦配置含有新格式内容（列表非空或旧 dir 已清除），按新格式写出且
// 不带 `dir`（§8.1：保留旧 dir 输入支持，新写出不带 dir）。
impl serde::Serialize for SourceConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let v1_in_place = self.legacy_format
            && self.dir.is_some()
            && self.excluded_auto_paths.is_empty()
            && self.additional_paths.is_empty();
        if v1_in_place {
            let mut s = serializer.serialize_struct("SourceConfig", 2)?;
            s.serialize_field("enabled", &self.enabled)?;
            s.serialize_field("dir", self.dir.as_deref().unwrap_or_default())?;
            return s.end();
        }
        let mut s = serializer.serialize_struct("SourceConfig", 4)?;
        s.serialize_field("enabled", &self.enabled)?;
        s.serialize_field("auto_discover", &self.auto_discover)?;
        s.serialize_field("excluded_auto_paths", &self.excluded_auto_paths)?;
        s.serialize_field("additional_paths", &self.additional_paths)?;
        s.end()
    }
}

/// 各来源的覆盖配置；None = 全默认（启用 + 默认目录）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentSources {
    #[serde(default)]
    pub claude: Option<SourceConfig>,
    #[serde(default)]
    pub codex: Option<SourceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// P01：隐私政策同意记录。缺失/false = 未同意，启动只显示隐私政策；
    /// 仅在用户点击「同意并继续」并原子保存成功后才由程序写为 true。
    /// 引导判定用 [`read_consent`]（严格读取），默认值不是授权。
    #[serde(default)]
    pub privacy_policy_accepted: bool,
    /// 定时自动同步在线价格（默认开启，每 24h）。
    #[serde(default = "default_true")]
    pub price_auto_sync: bool,
    /// C1：来源目录与启停配置（缺省 = 全启用 + 默认目录）。
    #[serde(default)]
    pub sources: AgentSources,
    /// 关闭窗口默认动作；None = 每次询问（前端弹窗，可勾选记忆）。
    #[serde(default)]
    pub close_action: Option<CloseAction>,
    /// 统计时区：`local` 或合法 IANA 名；None = 未设置（等价默认 Asia/Shanghai）。
    ///
    /// U03：时区的**唯一权威来源**就是这里——前端不再用 localStorage 保存
    /// 时区，视图快照也不携带它。写入必须经 [`validate_timezone`] 校验，
    /// 非法值在落盘前拒绝。
    #[serde(default)]
    pub timezone: Option<String>,
    /// U06：JSONL 自动读取间隔（秒）——合法值见 [`COLLECT_INTERVAL_CHOICES`]，
    /// 缺省（或手工写入非法值）回退 [`default_collect_interval_secs`]。
    /// 写入必须经 [`validate_collect_interval_secs`] 校验：非法值在落盘前拒绝。
    #[serde(default)]
    pub collect_interval_secs: Option<u32>,
}

/// U06：JSONL 自动读取间隔的可选值（秒；界面显示为 5s、10s、30s、1m、5m、10m）。
pub const COLLECT_INTERVAL_CHOICES: [u32; 6] = [5, 10, 30, 60, 300, 600];

/// U06：自动读取间隔的默认值（秒）——与既有行为一致（5 分钟）。
pub fn default_collect_interval_secs() -> u32 {
    300
}

/// U06：间隔取值校验：只接受 [`COLLECT_INTERVAL_CHOICES`]；非法值返回中文
/// 可读错误，调用方必须拒绝保存（文件字节不动）。
pub fn validate_collect_interval_secs(value: u32) -> Result<u32, String> {
    if COLLECT_INTERVAL_CHOICES.contains(&value) {
        return Ok(value);
    }
    Err(format!(
        "自动读取间隔 {value} 秒不受支持：只接受 5 / 10 / 30 / 60 / 300 / 600 秒（界面显示为 5s、10s、30s、1m、5m、10m）"
    ))
}

/// U06：读取侧回退点（**纯函数**，可单测）——字段缺失用默认值；手工编辑写入
/// 的非法值回退默认值并标记"需要告警"，绝不因为一个间隔值阻断启动。
///
/// 返回 `(生效间隔秒数, 是否需要告警)`。
pub fn resolve_collect_interval_secs(value: Option<u32>) -> (u32, bool) {
    match value {
        None => (default_collect_interval_secs(), false),
        Some(v) => match validate_collect_interval_secs(v) {
            Ok(v) => (v, false),
            Err(_) => (default_collect_interval_secs(), true),
        },
    }
}

/// U06：从设置取生效间隔；非法值回退默认并 warn（不阻断启动）。
pub fn collect_interval_secs_from(s: &Settings) -> u32 {
    let (value, invalid) = resolve_collect_interval_secs(s.collect_interval_secs);
    if invalid {
        log::warn!(
            "设置中的自动读取间隔非法（{:?}），已回退默认 {value} 秒；可选值：5 / 10 / 30 / 60 / 300 / 600 秒",
            s.collect_interval_secs
        );
    }
    value
}

/// 统计/展示时区的默认值（字段缺失、读取失败、非法值都回落到它）。
pub fn default_timezone() -> &'static str {
    "Asia/Shanghai"
}

/// U03：时区取值校验 + 归一化（trim；`local` 归一为小写）。
///
/// 合法取值：`local`（跟随本机时区）或 jiff 能解析的 IANA 时区名。
/// 返回归一化后的字符串；非法值返回中文可读错误，调用方必须拒绝保存
/// （文件字节不动）。
pub fn validate_timezone(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!(
            "时区不能为空：请填 local 或 IANA 时区名（如 {}、UTC）",
            default_timezone()
        ));
    }
    if trimmed.eq_ignore_ascii_case("local") {
        return Ok("local".to_string());
    }
    match jiff::tz::TimeZone::get(trimmed) {
        Ok(_) => Ok(trimmed.to_string()),
        Err(e) => Err(format!(
            "未知时区 {trimmed:?}：请填 local 或 IANA 时区名（如 {}、UTC）；{e}",
            default_timezone()
        )),
    }
}

fn default_true() -> bool {
    true
}

impl Settings {
    /// 取某来源的有效配置（未配置 → 全默认）。
    pub fn source_config(&self, claude: bool) -> SourceConfig {
        let c = if claude {
            &self.sources.claude
        } else {
            &self.sources.codex
        };
        c.clone().unwrap_or_default()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            privacy_policy_accepted: false,
            price_auto_sync: true,
            sources: AgentSources::default(),
            close_action: None,
            timezone: None,
            collect_interval_secs: None,
        }
    }
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(crate::report::data_dir()?.join("settings.toml"))
}

// ── SF02：统一设置事务 ─────────────────────────────────────────
// 全体应用内写者（关闭动作、记忆关闭、自动同步开关、来源配置、
// ensure_toml 首建/迁移）共享同一进程级读改写锁：后到的写者基于先到
// 写者**提交后**的最新文件做读改写，旧副本不得覆盖无关字段。
// 应用为单实例，一把锁覆盖全部设置文件更新即可；锁只在阻塞线程内
// 等待（调用方须置于 spawn_blocking，不得跨 await 持有）。

fn write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_write_lock() -> Result<MutexGuard<'static, ()>> {
    write_lock().lock().map_err(|_| {
        anyhow::anyhow!("设置写锁已污染（先前写入线程 panic），拒绝继续写入以保护原文件")
    })
}

/// 设置更新的唯一事务入口：锁内 load 最新文件（含遗留导入）→ mutate 与
/// 校验 → 原子保存 → 返回提交后的值。mutate 返回 Err 或解析/写入失败都
/// 不落盘，原文件字节不动。并发的外部编辑器不受本进程锁约束（写入冲突
/// 由原子替换兜底为"整文件生效其一"，不做字段级合并）。
pub fn update<T>(path: &Path, mutate: impl FnOnce(&mut Settings) -> Result<T>) -> Result<T> {
    let _guard = acquire_write_lock()?;
    let mut s = load(path)?;
    let out = mutate(&mut s)?;
    save_unlocked(path, &s)?;
    Ok(out)
}

/// 遗留设置文件（迁移前格式）：与 toml 同目录；路径相同（测试直传 json 路径）时
/// 不视为遗留文件。
fn legacy_json_path(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let legacy = parent.join("settings.json");
    (legacy != path).then_some(legacy)
}

/// 读设置：toml 缺失 → 尝试遗留 settings.json 一次性导入（只读，不落盘）；
/// 都缺失 → 默认值；坏内容 → Err（调用方警告并使用默认值/拒绝覆盖）。
pub fn load(path: &Path) -> Result<Settings> {
    if !path.exists() {
        if let Some(legacy) = legacy_json_path(path)
            && legacy.exists()
        {
            let text = std::fs::read_to_string(&legacy)
                .with_context(|| format!("读遗留设置失败: {}", legacy.display()))?;
            let s: Settings = serde_json::from_str(&text)
                .with_context(|| format!("遗留设置解析失败: {}", legacy.display()))?;
            log::info!("检测到遗留 settings.json，已导入其值（首次保存后迁移为 toml）");
            return Ok(s);
        }
        return Ok(Settings::default());
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("读设置失败: {}", path.display()))?;
    let s: Settings =
        toml::from_str(&text).with_context(|| format!("设置解析失败: {}", path.display()))?;
    Ok(s)
}

// ── P01：首次启动隐私同意（严格读取 + 事务保存）─────────────────
//
// 不变量：授权只能来自「真实存在且解析成功的 settings.toml 中该字段为 true」。
// 文件缺失、遗留 settings.json、默认值、损坏/不可读文件都**不是**同意——
// 引导判定绝不调用 load().unwrap_or_default()，也绝不用 Path::exists()
// 把权限/IO 错误吞成「新安装」。

/// [`read_consent`] 的判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsentRead {
    /// 真实 TOML 在位且同意字段为 true。
    Granted,
    /// 真实 TOML 在位，但同意字段缺失或为 false。
    NotGranted,
    /// 没有 settings.toml：首次安装、删除配置、只剩遗留 settings.json 都算这里。
    Missing,
    /// 路径无法定位/读取/解析（权限、路径被目录占用、字段类型错误等）——按封闭处理。
    Unreadable(String),
}

/// P01：引导专用的**严格**同意读取：只认磁盘上的真实 TOML。
///
/// 与 [`load`] 的区别：这里不导入遗留 json、不回退默认值，并区分
/// 「NotFound（真的没有文件）」与「其它 I/O/解析错误（有文件但读不出来）」，
/// 后者必须让引导显示「无法确认隐私设置」而不是当成新安装。
pub fn read_consent(path: &Path) -> ConsentRead {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ConsentRead::Missing,
        Err(e) => {
            return ConsentRead::Unreadable(format!("读设置失败: {}（{e}）", path.display()));
        }
    };
    match toml::from_str::<Settings>(&text) {
        Ok(s) if s.privacy_policy_accepted => ConsentRead::Granted,
        Ok(_) => ConsentRead::NotGranted,
        Err(e) => ConsentRead::Unreadable(format!("设置解析失败: {}（{e}）", path.display())),
    }
}

/// P01：用户点击「同意并继续」后的保存事务——沿用 SF02 事务锁，锁内基于
/// **最新磁盘值**只把同意字段设为 true，原子替换成功后才算同意成立。
///
/// 失败（解析/写入/锁污染）不落盘也不改内存值：调用方必须保持闸门关闭。
/// 遗留 json 的迁移只在这里发生（用户已明确同意之后）。
pub fn accept_privacy_policy(path: &Path) -> Result<()> {
    update(path, |s| {
        s.privacy_policy_accepted = true;
        Ok(())
    })
}

/// 配置文件头注释：每次保存都随内容重写（GUI 保存会丢自定义注释，见头说明）。
pub const SETTINGS_HEADER: &str = "\
# TokenScope 设置文件（GUI 设置页与手工编辑共用；可直接编辑，保存后对下一次读取生效）
# 注意：GUI 保存会重写整个文件，自定义注释会丢失；字段说明见设置页「高级配置」。
# 本文件专属于 TokenScope：GUI 保存会删除本程序不认识的字段/表，请勿把
# 其它工具的配置写在这里（读取时未知键被忽略仅是向前兼容，不承诺保留）。
";

/// 全字段注释模板：open_settings_file 首次创建用（全部键注释掉，解析即默认值）。
pub const SETTINGS_TEMPLATE: &str = "\
# TokenScope 设置文件（~/.tokenscope/settings.toml）
# ─────────────────────────────────────────────────────────
# 本文件是图形设置页的唯一事实源：页面上的修改会写回这里，
# 手动编辑保存后同样立即生效（每次读取都从磁盘加载）。
# 注意：GUI 保存时会重写整个文件，自定义注释会丢失（字段说明以本模板为准）。
#
# privacy_policy_accepted = true | false
#     隐私政策同意记录（默认 false）。缺失或 false 时，下一次启动先显示
#     隐私政策并要求明确同意，之后才读取来源日志、建立缓存、写诊断日志与
#     同步价格；不同意时应用不进入业务界面，可直接退出。
#     该项只在应用内点击「同意并继续」且保存成功后由程序写入 true。
#     删除本行或改为 false 会在下一次启动重新询问。本地文件不是防篡改的
#     法律证明：手工写成 true 同样会被接受为「用户已知情」。
#
# price_auto_sync = true | false
#     价格自动同步（默认开启，每 24h 检查一次；关闭后完全不联网）。
#
# close_action = \"minimize\" | \"quit\"
#     点击窗口关闭按钮的默认动作：minimize = 最小化到托盘；quit = 直接退出。
#     删除或注释此行 = 每次关闭弹窗询问（也可在设置页「关闭窗口时」修改）。
#
# timezone = \"Asia/Shanghai\" | \"local\" | 其它 IANA 时区名
#     统计与展示时区（设置页「通用」分区的下拉写的就是这里）。
#     local = 跟随本机时区；其余必须是 IANA 时区名（如 UTC、America/New_York）。
#     缺省/删除此行 = Asia/Shanghai。非法值会被设置页拒绝保存（不会写入本文件）；
#     手工写入的非法值在读取时回退默认值并提示。
#
# collect_interval_secs = 5 | 10 | 30 | 60 | 300 | 600
#     JSONL 自动读取间隔（秒；设置页显示为 5s、10s、30s、1m、5m、10m）。
#     后台线程按该间隔采集原生日志并写入用量历史库，确有新增或更新时通知界面；
#     间隔从上一轮采集结束开始计时，首轮延迟取该间隔与 30 秒中的较小者。
#     缺省/删除此行 = 300（5 分钟）。其它值会被设置页拒绝保存（不会写入本文件）；
#     手工写入的非法值在读取时回退 300 并记一条 warn（不阻断启动）。
#
# [sources.claude] / [sources.codex]
#     enabled = true | false    该来源总开关；停用后完全不采集（历史库不删数据）。
#     auto_discover = true | false
#         自动发现：当前用户的默认日志根与环境变量指定的根（CODEX_HOME、
#         CLAUDE_CONFIG_DIR 只在启动时读取，修改后需重启 TokenScope）。
#     excluded_auto_paths = [\"D:/logs/claude/projects\"]
#         已停用的自动候选目录（界面停用自动行时写入；重新开启由界面管理）。
#
#     [[sources.claude.additional_paths]]
#     path = \"D:/history/claude/projects\"    手动追加的日志根（绝对路径）。
#     enabled = true | false                 该追加项的启停。
#
#     注意：同一应用的目录互不相同且不嵌套（保存时校验）；旧版 v1 的 dir 字段
#     已在升级时迁移为一个追加项并开启 auto_discover——已启用来源的默认目录
#     用量也会一并纳入，统计总量可能增加（不是重复计量）；原已停用来源保持停用。
";

/// SF09：目录冲突校验——规范化后检查**相同或嵌套**（组件前缀关系），
/// 大小写不敏感（Windows 语义）。错误列出冲突目录与恢复方法。
/// 注意：canonicalize 对存在的 symlink/junction 解析实际路径；不存在的
/// 路径走组件折叠 fallback（与采集侧一致）。
pub fn validate_dir_conflict(a: &std::path::Path, b: &std::path::Path) -> Result<(), String> {
    let norm = |p: &std::path::Path| -> std::path::PathBuf {
        std::path::PathBuf::from(
            crate::report::normalize_path(p)
                .to_string_lossy()
                .to_lowercase(),
        )
    };
    let (na, nb) = (norm(a), norm(b));
    if na == nb {
        return Err(format!(
            "来源目录冲突：{} 与 {} 指向同一位置，会导致重复统计；请为两者配置不同且互不嵌套的目录，或停用其一来源",
            a.display(),
            b.display()
        ));
    }
    if nb.starts_with(&na) || na.starts_with(&nb) {
        return Err(format!(
            "来源目录冲突：{} 与 {} 存在嵌套关系，会导致文件归属歧义或重复统计；请为两者配置不同且互不嵌套的目录，或停用其一来源",
            a.display(),
            b.display()
        ));
    }
    Ok(())
}

/// Task 2（保留入口，SF09 起委托 validate_dir_conflict）：显式目录字符串
/// 形式的重叠校验——空/None 视为未配置，跳过检查。
pub fn validate_no_overlap(
    claude_dir: Option<&str>,
    codex_dir: Option<&str>,
) -> Result<(), String> {
    let eff = |s: Option<&str>| -> Option<std::path::PathBuf> {
        s.filter(|d| !d.trim().is_empty())
            .map(std::path::PathBuf::from)
    };
    match (eff(claude_dir), eff(codex_dir)) {
        (Some(c), Some(x)) => validate_dir_conflict(&c, &x),
        _ => Ok(()),
    }
}

// ── PR3（计划 §9.2）：来源目录迁移（v1 dir → v2 追加项 + 自动发现）────────

/// 迁移判定用的原始形态：保留「哪些键出现」的信息（§9.2 第 2 条），
/// 不先 `#[serde(default)]` 成空列表再猜新旧格式。
#[derive(Debug, Default, serde::Deserialize)]
struct RawSourceConfig {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    auto_discover: Option<bool>,
    #[serde(default)]
    excluded_auto_paths: Option<Vec<String>>,
    #[serde(default)]
    additional_paths: Option<Vec<AdditionalSourcePath>>,
}

impl RawSourceConfig {
    fn has_new_format_fields(&self) -> bool {
        self.auto_discover.is_some()
            || self.excluded_auto_paths.is_some()
            || self.additional_paths.is_some()
    }
    /// 是否需要迁移：旧 dir 存在且无任何新格式字段。新格式下旧 dir 必须忽略
    /// （§9.2 第 2 条），因此不迁移、不改文件。
    fn needs_migration(&self) -> bool {
        self.dir.is_some() && !self.has_new_format_fields()
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct RawAgentSources {
    #[serde(default)]
    claude: Option<RawSourceConfig>,
    #[serde(default)]
    codex: Option<RawSourceConfig>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct RawSettings {
    #[serde(default)]
    sources: RawAgentSources,
}

fn read_raw_settings(path: &Path) -> Result<RawSettings> {
    if path.exists() {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("读设置失败: {}", path.display()))?;
        return toml::from_str(&text).with_context(|| format!("设置解析失败: {}", path.display()));
    }
    // toml 缺失但遗留 settings.json 在位：迁移同样覆盖它（转换后 save 会落 toml
    // 并把 json 改名 .bak，与 ensure_toml 的遗留导入收敛到同一形态）。
    if let Some(legacy) = legacy_json_path(path)
        && legacy.exists()
    {
        let text = std::fs::read_to_string(&legacy)
            .with_context(|| format!("读遗留设置失败: {}", legacy.display()))?;
        return serde_json::from_str(&text)
            .with_context(|| format!("遗留设置解析失败: {}", legacy.display()));
    }
    Ok(RawSettings::default())
}

/// 迁移上下文：用户主目录与旧相对路径的解释基准（旧入口按 cwd 解释相对目录）。
/// 生产用真实值；测试注入隔离值，绝不触真实主目录（计划 §8）。
#[derive(Debug, Clone)]
pub struct SourceMigrationContext {
    pub home: PathBuf,
    pub cwd: PathBuf,
}

impl SourceMigrationContext {
    /// 生产上下文：真实主目录 + 当前工作目录。
    pub fn real() -> Result<Self> {
        Ok(Self {
            home: dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?,
            cwd: std::env::current_dir().context("无法定位当前工作目录")?,
        })
    }
    /// 某应用自动发现的默认根（与采集侧同源；v2 起还会并入环境根，迁移提示
    /// 按主目录默认根计算）。
    fn auto_roots(&self, agent: &str) -> Vec<PathBuf> {
        if agent == "claude" {
            vec![self.home.join(".claude").join("projects")]
        } else {
            vec![
                self.home.join(".codex").join("sessions"),
                self.home.join(".codex").join("archived_sessions"),
            ]
        }
    }
}

/// 一次成功迁移里单个来源的转换结果（设置页「用量数据」分区的一次性提示用）。
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertedSourcePath {
    /// `"claude"` 或 `"codex"`。
    pub agent: String,
    /// 旧 `dir` 原始值。
    pub old_dir: String,
    /// 追加项路径（相对路径已按旧入口语义转为绝对）。
    pub path: String,
    /// 追加项启用值（= 旧来源 enabled：整体停用的来源迁移后仍保持停用）。
    pub enabled: bool,
}

/// 来源目录迁移结果（§9.2 第 5 条：迁移提示由本次迁移结果返回，不逐轮重复）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourcePathMigration {
    /// 本次真正从旧 dir 转换出的追加项；空串旧 dir 只做格式归一，不产生条目。
    pub converted: Vec<ConvertedSourcePath>,
    /// 旧根与自动根相同或嵌套的一次性提示（重叠文件由采集层保证只解析一次）。
    pub overlap_notes: Vec<String>,
    /// 本次是否改写了文件（false = 已是新格式 / 无需迁移，幂等）。
    pub changed: bool,
}

/// 归一化比较键：组件折叠 + Windows 大小写折叠（与 [`validate_dir_conflict`] 同源）。
/// 对外发布：发现层与界面层的排除项匹配必须用这**同一个**规范化函数
/// （计划 §9.3 第 5 条），禁止第三套前缀判重。
pub fn normalized_key(p: &Path) -> String {
    crate::report::normalize_path(p)
        .to_string_lossy()
        .to_lowercase()
}

/// 判断迁移后的追加目录与自动根的重叠关系，生成一次性提示。
fn overlap_note(ctx: &SourceMigrationContext, agent: &str, resolved: &Path) -> Option<String> {
    let key = normalized_key(resolved);
    for root in ctx.auto_roots(agent) {
        let root_key = normalized_key(&root);
        let relation = if key == root_key {
            "相同"
        } else if root_key.starts_with(&key) || key.starts_with(&root_key) {
            "嵌套"
        } else {
            continue;
        };
        return Some(format!(
            "迁移后的追加目录 {} 与 {agent} 默认根 {} {}：自动发现已开启，重叠部分每轮只会解析一次；如不需要可移除该追加目录或关闭自动发现",
            resolved.display(),
            root.display(),
            relation
        ));
    }
    None
}

/// 把一个旧 `dir` 转换为 v2 配置（新 SourceConfig + 转换结果 + 重叠提示）。
/// 相对路径按旧入口语义（cwd）解释；不存在则报可操作错误，保留旧文件。
fn convert_legacy_source(
    agent: &str,
    raw: &RawSourceConfig,
    ctx: &SourceMigrationContext,
) -> Result<(SourceConfig, Option<ConvertedSourcePath>, Vec<String>)> {
    let enabled = raw.enabled.unwrap_or(true);
    let raw_dir = raw.dir.as_deref().unwrap_or_default();
    let mut notes = Vec::new();
    if raw_dir.trim().is_empty() {
        // 旧空 dir = 未配置：无追加项，仅归一为新格式（§9.2 第 4 条：不产生空路径行）。
        return Ok((
            SourceConfig {
                enabled,
                dir: None,
                auto_discover: true,
                excluded_auto_paths: Vec::new(),
                additional_paths: Vec::new(),
                legacy_format: false,
            },
            None,
            notes,
        ));
    }
    let configured = Path::new(raw_dir);
    let resolved = if configured.is_relative() {
        let abs = ctx.cwd.join(configured);
        if !abs.exists() {
            return Err(anyhow::anyhow!(
                "旧配置 sources.{agent}.dir = {raw_dir:?} 是相对路径，按当前工作目录 {} 解释为 {}，该目录不存在，无法保持原读取语义；请把该配置改为已存在的绝对路径后重试，或删除该行（本次迁移未改写设置文件）",
                ctx.cwd.display(),
                abs.display()
            ));
        }
        // 组件折叠 + 本地分隔符（join 保留输入里的 `/`，落盘前归一）
        crate::report::normalize_path(&abs)
    } else {
        configured.to_path_buf()
    };
    let path = resolved.to_string_lossy().into_owned();
    if let Some(note) = overlap_note(ctx, agent, &resolved) {
        notes.push(note);
    }
    Ok((
        SourceConfig {
            enabled,
            dir: None,
            auto_discover: true,
            excluded_auto_paths: Vec::new(),
            additional_paths: vec![AdditionalSourcePath {
                path: path.clone(),
                enabled,
            }],
            legacy_format: false,
        },
        Some(ConvertedSourcePath {
            agent: agent.to_string(),
            old_dir: raw_dir.to_string(),
            path,
            enabled,
        }),
        notes,
    ))
}

/// PR3：来源目录迁移——事务写锁内读最新文件，把旧 `dir` 转为一个追加项并开启
/// 自动发现，原子保存（§9.2 第 3 条）。刻意**不**调用 [`update`]（那会在同一把
/// 锁内递归 load）；迁移保存失败返回明确错误，原文件字节不变，不回退成"全部
/// 默认启用"。幂等：已迁移 / 显式新格式直接返回 `changed = false`。
pub fn migrate_source_paths(path: &Path) -> Result<SourcePathMigration> {
    migrate_source_paths_with(path, &SourceMigrationContext::real()?)
}

/// [`migrate_source_paths`] 的注入形态（测试与验收用，隔离主目录与 cwd）。
pub fn migrate_source_paths_with(
    path: &Path,
    ctx: &SourceMigrationContext,
) -> Result<SourcePathMigration> {
    let _guard = acquire_write_lock()?;
    let raw = read_raw_settings(path)?;
    let needs = [
        ("claude", raw.sources.claude.as_ref()),
        ("codex", raw.sources.codex.as_ref()),
    ]
    .into_iter()
    .filter(|(_, cfg)| cfg.is_some_and(RawSourceConfig::needs_migration))
    .collect::<Vec<_>>();
    if needs.is_empty() {
        return Ok(SourcePathMigration::default());
    }
    // 先完成全部转换计算，再统一落盘：任一来源无法保持原语义 → 整体失败、
    // 原文件字节不变（§9.2 第 3 条：失败不扩大读取范围）。
    let mut converted = Vec::new();
    let mut overlap_notes = Vec::new();
    let mut new_claude: Option<SourceConfig> = None;
    let mut new_codex: Option<SourceConfig> = None;
    for (agent, raw_cfg) in needs {
        let raw_cfg = raw_cfg.expect("needs 过滤后必为 Some");
        let (cfg, entry, notes) = convert_legacy_source(agent, raw_cfg, ctx)?;
        overlap_notes.extend(notes);
        if let Some(entry) = entry {
            converted.push(entry);
        }
        if agent == "claude" {
            new_claude = Some(cfg);
        } else {
            new_codex = Some(cfg);
        }
    }
    let mut settings = load(path)?;
    if let Some(cfg) = new_claude {
        settings.sources.claude = Some(cfg);
    }
    if let Some(cfg) = new_codex {
        settings.sources.codex = Some(cfg);
    }
    save_unlocked(path, &settings)?;
    log::info!(
        "来源目录迁移完成：{} 个旧 dir 转为追加项并开启自动发现",
        converted.len()
    );
    Ok(SourcePathMigration {
        converted,
        overlap_notes,
        changed: true,
    })
}

/// 手动追加目录校验失败的原因（§4.1 / §5）。
#[derive(Debug, Clone, PartialEq)]
pub enum AdditionalPathReject {
    /// 与已有行指向同一位置：提示「目录已在列表中」，不新增。
    AlreadyExists { existing: String },
    /// 与已有行存在父/子嵌套：直接拒绝，错误指出冲突行。
    Nested { existing: String },
}

/// PR3（§4.1）：手动追加目录与同应用**全部已有行**（默认、环境、手动、停用、
/// 缺失均包含）的逻辑比较——相同 → [`AdditionalPathReject::AlreadyExists`]；
/// 父/子嵌套 → [`AdditionalPathReject::Nested`]。只做规范化后的路径分量比较
/// （`C:/a` 与 `C:/ab` 不算嵌套），不触文件系统；联接别名的物理目标比较由
/// 发现层 canonicalize 后传入已解析路径。
pub fn check_additional_path(
    existing: &[std::path::PathBuf],
    new_path: &Path,
) -> Result<(), AdditionalPathReject> {
    let new_key = normalized_key(new_path);
    let new_norm = crate::report::normalize_path(new_path);
    for other in existing {
        if normalized_key(other) == new_key {
            return Err(AdditionalPathReject::AlreadyExists {
                existing: other.display().to_string(),
            });
        }
        let other_norm = crate::report::normalize_path(other);
        if other_norm.starts_with(&new_norm) || new_norm.starts_with(&other_norm) {
            return Err(AdditionalPathReject::Nested {
                existing: other.display().to_string(),
            });
        }
    }
    Ok(())
}

/// PR3（§5）：手动追加路径的形态校验——拒绝空值、相对路径、明显整盘与整个
/// 用户主目录；合法缺失的绝对根可保存（缺失与否由发现层显示，不在这里挡）。
/// 通过时返回 trim 后的路径。
pub fn validate_additional_path_shape(path: &str, home: &Path) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(
            "目录不能为空：请填写日志根的绝对路径（如 D:\\history\\claude\\projects）".to_string(),
        );
    }
    let p = Path::new(trimmed);
    if p.parent().is_none() {
        return Err(format!(
            "目录不能是整盘根：{trimmed}；请填写具体的日志目录（如 D:\\history\\claude\\projects）"
        ));
    }
    if p.is_relative() {
        return Err(format!(
            "目录必须是绝对路径：{trimmed} 是相对路径；请填写完整路径（如 D:\\history\\claude\\projects）"
        ));
    }
    if normalized_key(p) == normalized_key(home) {
        return Err(format!(
            "目录不能是整个用户主目录 {}：请填写其中的日志子目录（如 .claude\\projects）",
            home.display()
        ));
    }
    Ok(PathBuf::from(trimmed))
}

/// D3/R06：自动同步的允许判定——设置读取失败时返回 false（默认离线）。
/// 关闭自动同步的用户意图绝不能因设置损坏被悄悄重置成联网。
pub fn auto_sync_allowed(s: &Result<Settings, anyhow::Error>) -> bool {
    match s {
        Ok(s) => s.price_auto_sync,
        Err(_) => false,
    }
}

/// 确保设置文件以 toml 形态在位（open_settings_file 首建用）：
/// 已存在 → 原样不动；缺失但有遗留 json → load 导入 + save 迁移（值保留、
/// json 改名 .bak）；都缺失 → 写入全字段注释模板（解析即默认值）。
/// 首建/迁移也是写者：与 [`update`] 共享同一把写锁，不与并发更新互踩。
pub fn ensure_toml(path: &Path) -> Result<()> {
    let _guard = acquire_write_lock()?;
    if path.exists() {
        return Ok(());
    }
    let had_legacy = legacy_json_path(path).is_some_and(|p| p.exists());
    if had_legacy {
        let s = load(path)?;
        return save_unlocked(path, &s);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    }
    crate::fsutil::atomic_write(path, SETTINGS_TEMPLATE.as_bytes())
        .with_context(|| format!("写设置模板失败: {}", path.display()))?;
    Ok(())
}

/// 私有底层写入：调用方必须已持有写锁（[`update`] / [`ensure_toml`]）。
/// 不设公有 save——防止绕过事务读改写或递归加锁。
fn save_unlocked(path: &Path, s: &Settings) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    }
    let body = format!("{SETTINGS_HEADER}{}", toml::to_string_pretty(s)?);
    // 先落 toml 再处理遗留文件：toml 在位后 load 即不再读 json（改名失败只是延迟迁移）。
    crate::fsutil::atomic_write(path, body.as_bytes())
        .with_context(|| format!("写设置失败: {}", path.display()))?;
    if let Some(legacy) = legacy_json_path(path)
        && legacy.exists()
    {
        let bak = legacy.with_extension("json.bak");
        match std::fs::rename(&legacy, &bak) {
            Ok(()) => log::info!("遗留 settings.json 已迁移，改名保留为 {}", bak.display()),
            Err(e) => log::warn!("遗留 settings.json 改名失败（不影响使用）: {e}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tokenscope-m11-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("settings.toml")
    }

    #[test]
    fn test_settings_roundtrip_with_close_action() {
        let path = tmp("roundtrip");
        let s = Settings {
            privacy_policy_accepted: false,
            price_auto_sync: false,
            close_action: Some(CloseAction::Minimize),
            timezone: None,
            collect_interval_secs: None,
            sources: crate::settings::AgentSources {
                claude: Some(SourceConfig {
                    enabled: false,
                    dir: Some("D:/logs/claude".into()),
                    // dir 在位的真实文件只会以 v1 形态载入（legacy_format=true，
                    // 与 load 的产出一致）；序列化按同一条形状保留规则写回。
                    legacy_format: true,
                    ..Default::default()
                }),
                codex: None,
            },
        };
        save_unlocked(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap(), s);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("close_action = \"minimize\""),
            "关闭动作必须落盘: {text}"
        );
        // quit 同样往返；None 序列化为缺省（省略字段）
        let s2 = Settings {
            close_action: Some(CloseAction::Quit),
            ..s.clone()
        };
        save_unlocked(&path, &s2).unwrap();
        assert_eq!(load(&path).unwrap(), s2);
        let s3 = Settings {
            close_action: None,
            ..s2.clone()
        };
        save_unlocked(&path, &s3).unwrap();
        assert_eq!(load(&path).unwrap(), s3);
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("close_action")
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_save_writes_comment_header_and_template_parses() {
        let path = tmp("header");
        save_unlocked(&path, &Settings::default()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with('#'),
            "配置文件必须以注释头开头（可手编自解释）"
        );
        assert!(text.contains("直接编辑"), "注释头必须说明可手编: {text}");
        assert!(
            SETTINGS_TEMPLATE.contains("timezone"),
            "模板必须说明 timezone 的合法取值与缺省语义"
        );
        // 模板全部键注释化：解析即默认值（open_settings_file 首建用）
        let parsed: Settings = toml::from_str(SETTINGS_TEMPLATE).unwrap();
        assert_eq!(parsed, Settings::default());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_legacy_json_import() {
        // 迁移：settings.toml 缺失但遗留 settings.json 存在 → 导入其值；
        // load 保持只读（不落盘、不改名）。
        let dir = tmp("legacy-import");
        let dir = dir.parent().unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"price_auto_sync": false, "sources": {"claude": {"enabled": true, "dir": "D:/x"}}}"#,
        )
        .unwrap();
        let toml_path = dir.join("settings.toml");
        let s = load(&toml_path).unwrap();
        assert!(!s.price_auto_sync);
        assert_eq!(
            s.sources.claude.as_ref().unwrap().dir.as_deref(),
            Some("D:/x")
        );
        assert!(s.close_action.is_none());
        assert!(!toml_path.exists(), "load 不得落盘");
        assert!(dir.join("settings.json").exists(), "load 不得改名遗留文件");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_legacy_json_renamed_to_bak_on_save() {
        // 迁移完成时点 = 首次成功保存：json 改名 .bak（内容保留），防"改了没生效"。
        let dir = tmp("legacy-bak");
        let dir = dir.parent().unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"price_auto_sync": false}"#).unwrap();
        let toml_path = dir.join("settings.toml");
        let s = load(&toml_path).unwrap();
        save_unlocked(&toml_path, &s).unwrap();
        assert!(toml_path.exists());
        assert!(
            !dir.join("settings.json").exists(),
            "迁移后遗留文件必须改名"
        );
        let bak = std::fs::read_to_string(dir.join("settings.json.bak")).unwrap();
        assert!(bak.contains("price_auto_sync"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_toml_takes_precedence_over_legacy() {
        // toml 在位后遗留 json 被忽略（即使改名失败也不回读旧值）。
        let dir = tmp("precedence");
        let dir = dir.parent().unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"price_auto_sync": false}"#).unwrap();
        let toml_path = dir.join("settings.toml");
        std::fs::write(&toml_path, "price_auto_sync = true\n").unwrap();
        assert!(load(&toml_path).unwrap().price_auto_sync);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_legacy_json_bad_content_is_err() {
        // 坏 JSON：报错（绝不静默回默认后覆盖用户文件）；load 只读，文件原样保留。
        let dir = tmp("legacy-bad");
        let dir = dir.parent().unwrap();
        let corrupt = r#"{"price_auto_sync": true, "broken""#;
        std::fs::write(dir.join("settings.json"), corrupt).unwrap();
        assert!(load(&dir.join("settings.toml")).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.json")).unwrap(),
            corrupt
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_validate_no_overlap_parent_and_case_aliases() {
        // Task 2（审阅）：`..` 组件折叠 + 大小写不敏感——等价目录必须识别。
        // 注意：validate 不触达文件系统（保存前校验），走 fallback 规范化。
        assert!(validate_no_overlap(Some(r"a\b\..\shared"), Some(r"a\shared")).is_err());
        assert!(validate_no_overlap(Some("C:/Shared/Logs"), Some("c:/shared/logs")).is_err());
        assert!(validate_no_overlap(Some("a/shared/"), Some("a/shared")).is_err());
        // 不同目录不误报。
        assert!(validate_no_overlap(Some("a/one"), Some("a/two")).is_ok());
    }

    #[test]
    fn test_sync_disabled_on_invalid_settings() {
        // D3：设置损坏 → 自动同步按离线处理，不悄悄恢复联网。
        assert!(auto_sync_allowed(&Ok(Settings::default())));
        assert!(!auto_sync_allowed(&Err(anyhow::anyhow!("bad json"))));
        let off = Settings {
            price_auto_sync: false,
            ..Default::default()
        };
        assert!(!auto_sync_allowed(&Ok(off)));
    }

    #[test]
    fn test_source_config_defaults_and_lookup() {
        // C1（test_source_config_roundtrip 的默认侧）：未配置 → 启用 + 默认目录。
        let s = Settings::default();
        let c = s.source_config(true);
        assert!(c.enabled);
        assert!(c.dir.is_none());
        // 旧版 settings.json（无 sources 字段）→ 经遗留导入向前兼容。
        let dir = tmp("legacy-c1");
        let dir = dir.parent().unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"price_auto_sync": true}"#).unwrap();
        let s = load(&dir.join("settings.toml")).unwrap();
        assert!(s.source_config(false).enabled);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_settings_missing_defaults() {
        assert_eq!(
            load(Path::new("Z:/no-such/settings.toml")).unwrap(),
            Settings::default()
        );
    }

    #[test]
    fn test_settings_corrupt_is_err() {
        let path = tmp("corrupt");
        std::fs::write(&path, "not [valid toml").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_settings_unknown_fields_ignored() {
        let path = tmp("unknown");
        std::fs::write(&path, "price_auto_sync = false\nfuture_key = 123\n").unwrap();
        let s = load(&path).unwrap();
        assert!(!s.price_auto_sync);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    // ── P01：首次启动隐私同意（严格读取）─────────────────────

    /// 没有 TOML（首次安装、删除配置）→ Missing；默认值本身不是授权。
    #[test]
    fn privacy_missing_toml_requires_consent() {
        let path = tmp("privacy-missing");
        assert_eq!(read_consent(&path), ConsentRead::Missing);
        assert!(
            !Settings::default().privacy_policy_accepted,
            "默认值必须为未同意"
        );
        // load()（业务路径）与 read_consent（引导路径）对缺失文件都不产生授权
        assert!(!load(&path).unwrap().privacy_policy_accepted);
    }

    /// 有效 TOML：字段缺失或显式 false 都要求同意。
    #[test]
    fn privacy_missing_field_and_false_require_consent() {
        let path = tmp("privacy-field");
        std::fs::write(&path, "price_auto_sync = false\n").unwrap();
        assert_eq!(read_consent(&path), ConsentRead::NotGranted);
        std::fs::write(&path, "privacy_policy_accepted = false\n").unwrap();
        assert_eq!(read_consent(&path), ConsentRead::NotGranted);
    }

    /// 有效 TOML + true → 允许进入同意后的启动流程（重启不再询问）。
    #[test]
    fn privacy_valid_true_allows_bootstrap() {
        let path = tmp("privacy-true");
        std::fs::write(
            &path,
            "privacy_policy_accepted = true\nprice_auto_sync = false\n",
        )
        .unwrap();
        assert_eq!(read_consent(&path), ConsentRead::Granted);
        assert!(!load(&path).unwrap().price_auto_sync, "其余字段照常读取");
    }

    /// 遗留 json（即使其中写了 true）不构成授权：引导不读它，也不迁移。
    #[test]
    fn privacy_legacy_json_never_grants_bootstrap() {
        let dir = tmp("privacy-legacy");
        let dir = dir.parent().unwrap();
        let legacy = dir.join("settings.json");
        std::fs::write(&legacy, r#"{"privacy_policy_accepted": true}"#).unwrap();
        assert_eq!(
            read_consent(&dir.join("settings.toml")),
            ConsentRead::Missing
        );
        assert!(legacy.exists(), "引导读取不得改动遗留文件");
        std::fs::remove_dir_all(dir).ok();
    }

    /// 损坏 / 类型错误 / 路径被目录占用 → Unreadable（封闭），原文件字节不动。
    #[test]
    fn privacy_corrupt_or_unreadable_settings_fail_closed() {
        let path = tmp("privacy-corrupt");
        let corrupt = "privacy_policy_accepted = \"yes\"\nnot [valid";
        std::fs::write(&path, corrupt).unwrap();
        assert!(matches!(read_consent(&path), ConsentRead::Unreadable(_)));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            corrupt,
            "引导读取不得改写原文件"
        );

        // 类型错误：字符串写进 bool 字段
        std::fs::write(&path, "privacy_policy_accepted = \"true\"\n").unwrap();
        let typed = read_consent(&path);
        assert!(
            matches!(typed, ConsentRead::Unreadable(_)),
            "字段类型错误必须封闭而不是当作未同意/默认值: {typed:?}"
        );

        // 路径被目录占用：exists() = true 但读不出来 → Unreadable，不是 Missing
        let as_dir = path.parent().unwrap().join("settings-as-dir.toml");
        std::fs::create_dir_all(&as_dir).unwrap();
        let blocked = read_consent(&as_dir);
        assert!(
            matches!(&blocked, ConsentRead::Unreadable(m) if m.contains("读设置失败")),
            "I/O 错误不得被吞成「新安装」: {blocked:?}"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    // ── U03：统计时区（settings.toml 为唯一事实源）─────────────────

    /// 事务内设置时区（与命令层同一条读改写路径：先校验后落盘）。
    fn set_timezone(path: &Path, value: Option<&str>) -> Result<Option<String>> {
        let normalized = match value {
            None => None,
            Some(v) => Some(validate_timezone(v).map_err(anyhow::Error::msg)?),
        };
        update(path, |s| {
            s.timezone = normalized.clone();
            Ok(normalized)
        })
    }

    /// 缺省 = 未设置（等价默认 Asia/Shanghai）；旧 TOML 无该字段可正常读取。
    #[test]
    fn timezone_default_unset_and_legacy_toml_reads() {
        assert_eq!(default_timezone(), "Asia/Shanghai");
        assert!(Settings::default().timezone.is_none());
        let path = tmp("u03-tz-legacy");
        std::fs::write(&path, "price_auto_sync = false\n").unwrap();
        let s = load(&path).unwrap();
        assert!(s.timezone.is_none(), "旧文件无该字段 → None（等价默认值）");
        assert!(!s.price_auto_sync, "其余字段照常读取");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// 合法值：`local` 与 IANA 名；返回 trim 归一化后的字符串。
    #[test]
    fn timezone_validate_accepts_local_and_iana() {
        assert_eq!(validate_timezone("Asia/Shanghai").unwrap(), "Asia/Shanghai");
        assert_eq!(validate_timezone("  UTC\n").unwrap(), "UTC");
        assert_eq!(
            validate_timezone("America/New_York").unwrap(),
            "America/New_York"
        );
        assert_eq!(validate_timezone("local").unwrap(), "local");
    }

    /// 非法值：空串/纯空白/未知时区名——中文可读错误。
    #[test]
    fn timezone_validate_rejects_empty_and_unknown() {
        for bad in ["", "   ", "\t", "Not/AZone", "Mars/Olympus"] {
            let err = validate_timezone(bad).unwrap_err();
            assert!(err.contains("时区"), "错误必须中文可读: {err}");
        }
    }

    /// 非法值拒绝保存：文件字节不动（校验失败不得落盘）。
    #[test]
    fn timezone_invalid_rejected_without_touching_file() {
        let path = tmp("u03-tz-reject");
        set_timezone(&path, Some("UTC")).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(set_timezone(&path, Some("Not/AZone")).is_err());
        assert!(set_timezone(&path, Some("   ")).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "拒绝保存时文件不得改动"
        );
        assert_eq!(load(&path).unwrap().timezone.as_deref(), Some("UTC"));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// 连续两次 update：重新读取等于后一次；清除后字段从文件消失。
    #[test]
    fn timezone_two_updates_last_one_wins() {
        let path = tmp("u03-tz-sequence");
        set_timezone(&path, Some("UTC")).unwrap();
        set_timezone(&path, Some(" America/New_York ")).unwrap();
        assert_eq!(
            load(&path).unwrap().timezone.as_deref(),
            Some("America/New_York"),
            "连续两次写入后读到的必须是后一次"
        );
        set_timezone(&path, None).unwrap();
        assert!(load(&path).unwrap().timezone.is_none());
        assert!(
            !std::fs::read_to_string(&path).unwrap().contains("timezone"),
            "None 序列化为缺省（省略字段）"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    // ── U06：JSONL 自动读取间隔（settings.toml 单一来源）──────────

    /// 读取回退点：缺省 → 默认值；非法值 → 回退默认并标记需要告警。
    #[test]
    fn collect_interval_resolution_falls_back_on_invalid() {
        assert_eq!(
            resolve_collect_interval_secs(None),
            (default_collect_interval_secs(), false)
        );
        for ok in COLLECT_INTERVAL_CHOICES {
            assert_eq!(resolve_collect_interval_secs(Some(ok)), (ok, false));
        }
        for bad in [0u32, 1, 7, 45, 601, 3600] {
            assert_eq!(
                resolve_collect_interval_secs(Some(bad)),
                (default_collect_interval_secs(), true),
                "{bad} 必须回退默认值并标记需要告警"
            );
        }
    }

    /// 校验函数：只接受 5 / 10 / 30 / 60 / 300 / 600，错误中文可读。
    #[test]
    fn collect_interval_validation_rejects_unsupported_values() {
        for ok in [5u32, 10, 30, 60, 300, 600] {
            assert_eq!(validate_collect_interval_secs(ok).unwrap(), ok);
        }
        for bad in [0u32, 2, 7, 59, 61, 299, 301, 6000, u32::MAX] {
            let err = validate_collect_interval_secs(bad).unwrap_err();
            assert!(
                err.contains("自动读取间隔") && err.contains("600"),
                "错误必须中文可读且列出可选值: {err}"
            );
        }
    }

    /// 模板必须说明该字段，且模板本身解析 = `Settings::default()`。
    #[test]
    fn collect_interval_template_documents_choices_and_parses_to_default() {
        assert!(
            SETTINGS_TEMPLATE.contains("collect_interval_secs"),
            "模板必须说明自动读取间隔"
        );
        for choice in ["5", "10", "30", "60", "300", "600"] {
            assert!(
                SETTINGS_TEMPLATE.contains(choice),
                "模板必须列出可选值 {choice}"
            );
        }
        let parsed: Settings = toml::from_str(SETTINGS_TEMPLATE).unwrap();
        assert_eq!(parsed, Settings::default());
    }

    /// 落盘往返：显式值可读回；None 序列化为缺省（省略字段）。
    #[test]
    fn collect_interval_roundtrip_and_omitted_when_unset() {
        let path = tmp("u06-interval");
        let s = Settings {
            collect_interval_secs: Some(30),
            ..Default::default()
        };
        save_unlocked(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap().collect_interval_secs, Some(30));
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("collect_interval_secs = 30"),
            "显式值必须落盘"
        );
        save_unlocked(&path, &Settings::default()).unwrap();
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("collect_interval_secs"),
            "None 序列化为缺省（省略字段）"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// 手工写入的非法值在读取时回退默认并告警，不阻断启动。
    #[test]
    fn collect_interval_manual_invalid_value_falls_back_with_warning() {
        let path = tmp("u06-interval-invalid");
        std::fs::write(&path, "collect_interval_secs = 45\n").unwrap();
        let s = load(&path).unwrap();
        assert_eq!(s.collect_interval_secs, Some(45), "字段本身照常解析");
        assert_eq!(collect_interval_secs_from(&s), 300, "非法值必须回退默认值");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "collect_interval_secs = 45\n",
            "读取不得改写用户文件"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
