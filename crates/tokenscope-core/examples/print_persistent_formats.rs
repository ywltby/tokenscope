//! 构建期指纹打印（UP03）：只做一件事——把核心库的真实持久格式常量以一行
//! JSON 打到标准输出，供生成 `build-metadata.json` 的作业原样采集：
//!
//! ```text
//! cargo run --locked --release -p tokenscope --example print_persistent_formats
//! ```
//!
//! 本示例不进安装包（Tauri 只打包 `tokenscope.exe`）；输出内容与键顺序由
//! [`tokenscope::persistent_format::persistent_format`] 定义，此处不复制任何常量数字。

fn main() {
    println!(
        "{}",
        tokenscope::persistent_format::persistent_format().to_json()
    );
}
