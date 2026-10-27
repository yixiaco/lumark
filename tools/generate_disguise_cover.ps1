# 生成伪装压缩使用的内置面具资源。
#
# 面具需要是真实可播放/可查看的媒体文件：MP4 提供视频外观，JPG 提供图片外观。
# 这里用 ffmpeg 的 lavfi 渐变源生成，不依赖任何第三方素材；生成结果直接
# 编入 `lumark-archive`，运行时不访问网络。需要本机 PATH 中有 ffmpeg。
param(
    [string]$OutputDir = (Join-Path $PSScriptRoot "..\crates\lumark-archive\assets\disguise")
)

$ErrorActionPreference = "Stop"
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
    throw "找不到 ffmpeg，请先安装并把 ffmpeg 加入 PATH"
}
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

# 两秒、640×360、H.264 baseline、静音、faststart：moov 在文件前部，追加的
# 伪装载荷和尾部信息不会影响常见播放器解析。
& ffmpeg -y -hide_banner -loglevel error `
    -f lavfi -i "gradients=s=640x360:d=2:c0=0x1f2937:c1=0x2563eb:x0=0:y0=0:x1=640:y1=360" `
    -c:v libx264 -profile:v baseline -level 3.0 -pix_fmt yuv420p -movflags +faststart -an `
    (Join-Path $OutputDir "cover.mp4")

# 1280×720 静态渐变图；JPEG 对尾部附加数据最宽容。
& ffmpeg -y -hide_banner -loglevel error `
    -f lavfi -i "gradients=s=1280x720:c0=0x1f2937:c1=0x2563eb:x0=0:y0=0:x1=1280:y1=720" `
    -frames:v 1 -q:v 4 `
    (Join-Path $OutputDir "cover.jpg")

Get-ChildItem -LiteralPath $OutputDir | Select-Object Name, Length
