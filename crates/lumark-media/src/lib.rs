//! 媒体探测与抽帧（M 阶段起始）：基于 ffmpeg/ffprobe 子进程。
//!
//! 不依赖 FFmpeg 开发库，只调用用户环境中的 `ffmpeg`/`ffprobe` 可执行文件；
//! 找不到工具时返回可读错误，由界面提示安装或配置路径。

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde::Deserialize;

/// ffmpeg/ffprobe 可执行文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegTools {
    /// `ffmpeg` 路径。
    pub ffmpeg: PathBuf,
    /// `ffprobe` 路径。
    pub ffprobe: PathBuf,
    /// `ffplay` 路径（可选，用于音频输出）。
    pub ffplay: Option<PathBuf>,
}

impl FfmpegTools {
    /// 从环境变量、PATH 与常见目录定位工具。
    pub fn locate() -> Result<Self, String> {
        if let Ok(value) = std::env::var("LUMARK_FFMPEG")
            && !value.trim().is_empty()
        {
            let path = PathBuf::from(value.trim());
            let directory = if path.is_file() {
                path.parent().unwrap_or(Path::new(".")).to_path_buf()
            } else {
                path
            };
            let ffmpeg = executable(&directory, "ffmpeg");
            let ffprobe = executable(&directory, "ffprobe");
            if ffmpeg.is_file() && ffprobe.is_file() {
                return Ok(Self {
                    ffmpeg,
                    ffprobe,
                    ffplay: optional_executable(&directory, "ffplay"),
                });
            }
            return Err(format!(
                "LUMARK_FFMPEG 指向的目录缺少 ffmpeg/ffprobe：{}",
                directory.display()
            ));
        }
        let path_var = std::env::var_os("PATH").unwrap_or_default();
        for directory in std::env::split_paths(&path_var) {
            let ffmpeg = executable(&directory, "ffmpeg");
            let ffprobe = executable(&directory, "ffprobe");
            if ffmpeg.is_file() && ffprobe.is_file() {
                return Ok(Self {
                    ffmpeg,
                    ffprobe,
                    ffplay: optional_executable(&directory, "ffplay"),
                });
            }
        }
        Err("未找到 ffmpeg/ffprobe；请安装 FFmpeg 或设置 LUMARK_FFMPEG".to_string())
    }

    /// 是否具备音频输出能力（同目录 ffplay）。
    pub fn has_audio_backend(&self) -> bool {
        self.ffplay.is_some()
    }
}

/// 可选工具路径。
fn optional_executable(directory: &Path, name: &str) -> Option<PathBuf> {
    let path = executable(directory, name);
    path.is_file().then_some(path)
}

/// 在目录中构造可执行文件名。
fn executable(directory: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        directory.join(format!("{name}.exe"))
    } else {
        directory.join(name)
    }
}

/// 媒体信息。
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    /// 文件路径。
    pub path: PathBuf,
    /// 容器格式。
    pub format_name: String,
    /// 时长（秒）。
    pub duration_seconds: f64,
    /// 视频宽。
    pub width: u32,
    /// 视频高。
    pub height: u32,
    /// 视频帧率。
    pub fps: f64,
    /// 视频编码。
    pub video_codec: Option<String>,
    /// 音频编码。
    pub audio_codec: Option<String>,
}

impl MediaInfo {
    /// 是否有视频轨。
    pub fn has_video(&self) -> bool {
        self.video_codec.is_some() && self.width > 0 && self.height > 0
    }

    /// 是否有音频轨。
    pub fn has_audio(&self) -> bool {
        self.audio_codec.is_some()
    }

    /// 界面摘要。
    pub fn summary(&self) -> String {
        let resolution = if self.has_video() {
            format!("{}×{} @ {:.2}fps", self.width, self.height, self.fps)
        } else {
            "无视频轨".to_string()
        };
        format!(
            "{} · {} · {:.2}s · {}{}",
            self.format_name,
            resolution,
            self.duration_seconds,
            self.video_codec.as_deref().unwrap_or("-"),
            if self.has_audio() {
                format!(" + {}", self.audio_codec.as_deref().unwrap_or("-"))
            } else {
                String::new()
            }
        )
    }
}

/// 抽出的视频帧（RGBA8）。
#[derive(Debug, Clone)]
pub struct MediaFrame {
    /// 像素宽。
    pub width: u32,
    /// 像素高。
    pub height: u32,
    /// RGBA 像素。
    pub rgba: Vec<u8>,
}

/// 使用系统 ffmpeg 探测媒体。
pub fn probe(path: &Path) -> Result<MediaInfo, String> {
    let tools = FfmpegTools::locate()?;
    probe_with(&tools, path)
}

/// 使用指定工具探测媒体。
pub fn probe_with(tools: &FfmpegTools, path: &Path) -> Result<MediaInfo, String> {
    let output = Command::new(&tools.ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|error| format!("启动 ffprobe 失败：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffprobe 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_probe_json(path, &output.stdout)
}

/// ffprobe JSON 结构。
#[derive(Debug, Deserialize)]
struct ProbeJson {
    streams: Vec<StreamJson>,
    format: FormatJson,
}

/// 单条流。
#[derive(Debug, Deserialize)]
struct StreamJson {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
}

/// 容器信息。
#[derive(Debug, Deserialize)]
struct FormatJson {
    format_name: Option<String>,
    duration: Option<String>,
}

/// 解析 ffprobe JSON。
fn parse_probe_json(path: &Path, bytes: &[u8]) -> Result<MediaInfo, String> {
    let json: ProbeJson =
        serde_json::from_slice(bytes).map_err(|error| format!("解析 ffprobe 输出失败：{error}"))?;
    let video = json
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"));
    let audio = json
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"));
    let fps = video
        .and_then(|stream| {
            stream
                .avg_frame_rate
                .as_deref()
                .and_then(parse_rate)
                .or_else(|| stream.r_frame_rate.as_deref().and_then(parse_rate))
        })
        .unwrap_or(0.0);
    Ok(MediaInfo {
        path: path.to_path_buf(),
        format_name: json
            .format
            .format_name
            .unwrap_or_else(|| "unknown".to_string()),
        duration_seconds: json
            .format
            .duration
            .as_deref()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0),
        width: video.and_then(|stream| stream.width).unwrap_or(0),
        height: video.and_then(|stream| stream.height).unwrap_or(0),
        fps,
        video_codec: video.and_then(|stream| stream.codec_name.clone()),
        audio_codec: audio.and_then(|stream| stream.codec_name.clone()),
    })
}

/// 解析 `num/den` 帧率。
fn parse_rate(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator: f64 = numerator.parse().ok()?;
    let denominator: f64 = denominator.parse().ok()?;
    (denominator > 0.0).then_some(numerator / denominator)
}

/// 抽取指定时间点的一帧并解码为 RGBA。
pub fn extract_frame(
    path: &Path,
    seconds: f64,
    max_width: Option<u32>,
) -> Result<MediaFrame, String> {
    let tools = FfmpegTools::locate()?;
    extract_frame_with(&tools, path, seconds, max_width)
}

/// 使用指定工具抽帧。
pub fn extract_frame_with(
    tools: &FfmpegTools,
    path: &Path,
    seconds: f64,
    max_width: Option<u32>,
) -> Result<MediaFrame, String> {
    let mut command = Command::new(&tools.ffmpeg);
    command.args(["-v", "error"]);
    command.args(["-ss", &format!("{:.3}", seconds.max(0.0))]);
    command.arg("-i").arg(path);
    if let Some(width) = max_width.filter(|width| *width > 0) {
        command.args(["-vf", &format!("scale='min(iw,{width})':-2")]);
    }
    command.args([
        "-frames:v",
        "1",
        "-f",
        "image2pipe",
        "-vcodec",
        "png",
        "pipe:1",
    ]);
    let output = command
        .output()
        .map_err(|error| format!("启动 ffmpeg 失败：{error}"))?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err(format!(
            "ffmpeg 抽帧失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let image = image::load_from_memory(&output.stdout)
        .map_err(|error| format!("解码抽帧失败：{error}"))?;
    let rgba = image.to_rgba8();
    Ok(MediaFrame {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// 启动 `ffplay` 播放音频（不显示窗口，播完自动退出）。
pub fn spawn_audio(tools: &FfmpegTools, path: &Path, seconds: f64) -> Result<Child, String> {
    let ffplay = tools
        .ffplay
        .as_ref()
        .ok_or_else(|| "未找到 ffplay，无法输出音频".to_string())?;
    Command::new(ffplay)
        .args(["-v", "error", "-nodisp", "-autoexit"])
        .args(["-ss", &format!("{:.3}", seconds.max(0.0))])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("启动音频播放失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "streams": [
            {"codec_type": "video", "codec_name": "h264", "width": 1280, "height": 720, "avg_frame_rate": "30000/1001"},
            {"codec_type": "audio", "codec_name": "aac"}
        ],
        "format": {"format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "12.5"}
    }"#;

    /// ffprobe JSON → MediaInfo。
    #[test]
    fn parses_probe_json() {
        let info = parse_probe_json(Path::new("sample.mp4"), FIXTURE.as_bytes()).expect("解析");
        assert_eq!(info.width, 1280);
        assert_eq!(info.height, 720);
        assert!((info.fps - 29.97).abs() < 0.02);
        assert_eq!(info.video_codec.as_deref(), Some("h264"));
        assert_eq!(info.audio_codec.as_deref(), Some("aac"));
        assert!((info.duration_seconds - 12.5).abs() < f32::EPSILON as f64);
        assert!(info.has_video() && info.has_audio());
        assert!(info.summary().contains("1280×720"));
    }

    /// 帧率解析与无视频轨回退。
    #[test]
    fn parses_rates_and_missing_video() {
        assert_eq!(parse_rate("25/1"), Some(25.0));
        assert_eq!(parse_rate("0/0"), None);
        assert_eq!(parse_rate("bad"), None);
        let info = parse_probe_json(
            Path::new("audio.m4a"),
            br#"{"streams":[{"codec_type":"audio","codec_name":"aac"}],"format":{"format_name":"mp4","duration":"3.0"}}"#,
        )
        .expect("解析");
        assert!(!info.has_video());
        assert!(info.has_audio());
        assert_eq!(info.width, 0);
    }

    /// 缺少 ffplay 时音频启动返回可读错误；不实际启动进程。
    #[test]
    fn audio_requires_ffplay() {
        let tools = FfmpegTools {
            ffmpeg: PathBuf::from("ffmpeg"),
            ffprobe: PathBuf::from("ffprobe"),
            ffplay: None,
        };
        assert!(!tools.has_audio_backend());
        let error = spawn_audio(&tools, Path::new("sample.mp4"), 0.0).expect_err("应缺少 ffplay");
        assert!(error.contains("ffplay"));
    }

    /// 真实 ffmpeg（缺失时跳过）：生成 1 秒测试视频并探测/抽帧。
    #[test]
    fn ffmpeg_probe_and_frame_round_trip() {
        let Ok(tools) = FfmpegTools::locate() else {
            eprintln!("未安装 ffmpeg，跳过集成测试");
            return;
        };
        let directory = tempfile::tempdir().expect("临时目录");
        let video = directory.path().join("test.mp4");
        let status = Command::new(&tools.ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x90:rate=10",
            ])
            .args(["-t", "1", "-pix_fmt", "yuv420p"])
            .arg(&video)
            .status()
            .expect("生成测试视频");
        assert!(status.success());
        let info = probe_with(&tools, &video).expect("探测");
        assert!(info.has_video(), "{}", info.summary());
        assert!((info.fps - 10.0).abs() < 0.1);
        assert!(info.duration_seconds > 0.0);
        let frame = extract_frame_with(&tools, &video, 0.2, Some(80)).expect("抽帧");
        assert_eq!(frame.width, 80);
        assert_eq!(frame.rgba.len(), 80 * frame.height as usize * 4);
    }
}
