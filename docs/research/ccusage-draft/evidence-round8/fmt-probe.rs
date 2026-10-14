fn main() {
    // 复现 ccusage-core/output.rs:548-550 的 format_currency
    fn format_currency(value: f64) -> String {
        format!("${value:.2}")
    }
    let probes: [f64; 12] = [
        0.005, 0.015, 0.025, 0.125, 0.135, 1.005, 1.015, 2.675, 2.685, 88.275_468_199_999_98, 0.0, 1234.5678,
    ];
    for v in probes {
        println!("{v:<24} => {}", format_currency(v));
    }
    // 也看 json_float 的行为（整数值 -> 整数 JSON）
    println!("--- json_float 语义（output.rs:452-462）---");
    for v in [42.0_f64, 42.5, 0.0, -0.0, 1e15] {
        let s = if v.fract() == 0.0 && v.abs() < 1e15 {
            format!("{}", v as i64)
        } else {
            format!("{v}")
        };
        println!("{v:<10} => {s}");
    }
}
