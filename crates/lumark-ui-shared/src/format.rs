//! 主程序与自解压运行时共用的尺寸和时间格式化。

/// 使用二进制单位格式化字节数，供紧凑界面标签显示。
pub fn format_size(bytes: u64) -> String {
    /// 字节格式化使用的二进制单位。
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// 把秒数格式化为 HH:MM:SS，供进度面板显示。
pub fn format_duration(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::{format_duration, format_size};

    /// 验证二进制单位和时长格式保持稳定。
    #[test]
    fn sizes_and_durations_are_formatted() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_duration(3661), "01:01:01");
    }
}
