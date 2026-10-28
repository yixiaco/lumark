//! 压缩窗口下拉框的固定标签和选项。

/// ZIP 压缩方法选项。
pub(crate) const ZIP_METHOD_LABELS: [&str; 3] = ["Deflate（推荐）", "BZip2", "LZMA"];
/// 7Z 压缩方法选项。
pub(crate) const SEVEN_METHOD_LABELS: [&str; 4] = ["LZMA2（推荐）", "LZMA", "PPMd", "BZip2"];
/// ZPAQ 压缩级别选项。
pub(crate) const ZPAQ_METHOD_LABELS: [&str; 5] = [
    "1 - 快速",
    "2 - 平衡（推荐）",
    "3 - 较高",
    "4 - 更高（较慢）",
    "5 - 极限（最慢）",
];
/// LZMA 字典大小选项。
pub(crate) const DICTIONARY_LABELS: [&str; 17] = [
    "1 MB", "2 MB", "4 MB", "8 MB", "16 MB", "24 MB", "32 MB", "48 MB", "64 MB", "96 MB", "128 MB",
    "192 MB", "256 MB", "384 MB", "512 MB", "768 MB", "1024 MB",
];
/// LZMA 字长选项。
pub(crate) const WORD_LABELS: [&str; 12] = [
    "8", "12", "16", "24", "32", "48", "64", "96", "128", "192", "256", "273",
];
/// 7Z 固实块选项。
pub(crate) const SOLID_LABELS: [&str; 17] = [
    "1 MB", "2 MB", "4 MB", "8 MB", "16 MB", "32 MB", "64 MB", "128 MB", "256 MB", "512 MB",
    "1 GB", "2 GB", "4 GB", "8 GB", "16 GB", "32 GB", "64 GB",
];
/// 压缩线程数选项。
pub(crate) const THREAD_LABELS: [&str; 32] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17",
    "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32",
];
/// 压缩过滤器和参数选项。
pub(crate) const FILTER_LABELS: [&str; 4] = ["任意时间", "早于", "晚于", "日期范围"];
/// 归档时间来源选项。
pub(crate) const ARCHIVE_TIME_LABELS: [&str; 3] = ["当前系统时间", "原始压缩时间", "最近文件时间"];
