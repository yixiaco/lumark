# 媒体播放器后端（M 阶段）

> 更新：2026-11-02。crate：`crates/lumark-media`。

## 依赖策略

不链接 FFmpeg 开发库，只调用用户环境中的 `ffmpeg`/`ffprobe` 可执行文件，
避免引入 `ffmpeg-sys` 的编译/分发负担。工具发现顺序：

1. `LUMARK_FFMPEG`（目录或 `ffmpeg.exe` 路径）；
2. `PATH` 中的 `ffmpeg` + `ffprobe`。

找不到时返回可读错误，界面可提示安装 FFmpeg 或设置环境变量。

## API

```rust
let tools = lumark_media::FfmpegTools::locate()?;
let info = lumark_media::probe_with(&tools, path)?;
info.duration_seconds; info.width; info.height; info.fps;
info.video_codec; info.audio_codec; info.has_video(); info.has_audio();
info.summary();

let frame = lumark_media::extract_frame_with(&tools, path, 1.5, Some(960))?;
frame.width; frame.height; frame.rgba; // RGBA8
```

- 探测：`ffprobe -v error -print_format json -show_format -show_streams`，解析容器时长、视频/音频流、`avg_frame_rate`/`r_frame_rate`；
- 抽帧：`ffmpeg -ss <t> -i <file> [-vf scale=W:-2] -frames:v 1 -f image2pipe -vcodec png pipe:1`，用 `image` 解码为 RGBA；
- 宽度可选，避免 4K 视频解码整帧占内存。

## 测试

- ffprobe JSON → `MediaInfo`（分辨率/帧率/编码/音轨/时长）；
- 帧率 `num/den` 解析与无视频轨回退；
- 真实 ffmpeg 集成：生成 1 秒 `testsrc` 160×90@10fps 视频 → 探测 → 抽帧 80px，断言尺寸与像素长度（未安装 ffmpeg 时跳过）。

## 播放窗口（M-4/M-5）

- 文件系统视频（mp4/m4v/mkv/webm/mov/avi/wmv/flv/ts/m2ts）打开时自动进入内置播放器；
- 打开后异步探测并加载首帧；
- 工具条：播放/暂停、±5s、±1 帧、回到开头；
- 播放循环按帧率推进时间并请求抽帧，超时/落后请求由 generation 丢弃；
- 标题与工具条右侧显示 `mm:ss.mmm / 总时长`；
- 状态栏显示容器/分辨率/帧率/编码摘要与抽帧状态；
- ffmpeg 不可用时回退系统默认播放器并提示。

限制：当前为**按帧抽图预览**，不含音频输出（M-6）；归档内媒体先解压到临时目录再打开（M-7 已接通），临时文件沿用现有 `%TEMP%/Lumark/open` 生命周期。

## 后续（M-6…M-8）

- 播放窗口：首帧 + 播放/暂停 + ±5s/帧步进；
- 时间轴与缓冲：定时按时间抽帧，避免进程堆积；
- 音频：ffplay/系统音频或 WASAPI 输出；
- 归档内媒体：先解压到临时文件再探测；
- 文档/验收：真实 MP4/MKV/WebM。
