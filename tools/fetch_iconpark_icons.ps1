$ErrorActionPreference = "Stop"

$assetRoot = Join-Path $PSScriptRoot "..\crates\lumark-ui\assets\icons\iconpark"
New-Item -ItemType Directory -Force -Path $assetRoot | Out-Null

$icons = @(
    "add",
    "arrow-left",
    "arrow-right",
    "arrow-up",
    "check",
    "close",
    "compression",
    "copy",
    "delete",
    "doc-detail",
    "down",
    "download",
    "edit",
    "file-lock",
    "file-zip",
    "folder",
    "folder-open",
    "folder-plus",
    "grid-four",
    "home",
    "key",
    "list",
    "lock",
    "menu-fold",
    "menu-unfold",
    "more",
    "music",
    "refresh",
    "right",
    "save",
    "search",
    "setting",
    "shield",
    "time",
    "tool",
    "unlock",
    "video",
    "picture",
    "zip"
)

$base = "https://cdn.jsdelivr.net/npm/@icon-park/svg@1.4.2/lib/icons"

foreach ($name in $icons) {
    $pascal = (($name -split "-") | ForEach-Object {
        if ($_.Length -gt 0) {
            $_.Substring(0, 1).ToUpperInvariant() + $_.Substring(1)
        }
    }) -join ""
    $uri = "$base/$pascal.js"
    $source = (Invoke-WebRequest -Uri $uri -TimeoutSec 30 -UseBasicParsing).Content

    $temporarySource = Join-Path ([System.IO.Path]::GetTempPath()) "lumark-iconpark-$([Guid]::NewGuid()).js"
    [System.IO.File]::WriteAllText(
        $temporarySource,
        $source,
        [System.Text.UTF8Encoding]::new($false)
    )
    $svg = (& node (Join-Path $PSScriptRoot "extract_iconpark_svg.mjs") $temporarySource) -join "`n"
    Remove-Item -LiteralPath $temporarySource

    $svg = $svg -replace '<\?xml[^>]*\?>', ""
    $svg = $svg -replace '(<svg[^>]*?)\swidth="[^"]*"', '$1'
    $svg = $svg -replace '(<svg[^>]*?)\sheight="[^"]*"', '$1'
    $svg = $svg -replace 'fill="(?!none")[^"]*"', 'fill="currentColor"'
    $svg = $svg -replace 'stroke="(?!none")[^"]*"', 'stroke="currentColor"'
    $svg = [regex]::Replace(
        $svg,
        '<(?:path|circle|rect|ellipse)\b[^>]*>',
        {
            param($match)
            $tag = $match.Value
            if ($tag.Contains('stroke="currentColor"')) {
                $tag -replace 'fill="currentColor"', 'fill="none"'
            } else {
                $tag
            }
        }
    )

    $target = Join-Path $assetRoot "$name.svg"
    [System.IO.File]::WriteAllText($target, $svg, [System.Text.UTF8Encoding]::new($false))
    Write-Host "saved $name.svg"
}

$license = (Invoke-WebRequest -Uri "https://cdn.jsdelivr.net/npm/@icon-park/svg@1.4.2/LICENSE" -TimeoutSec 30 -UseBasicParsing).Content
[System.IO.File]::WriteAllText(
    (Join-Path $assetRoot "LICENSE"),
    $license,
    [System.Text.UTF8Encoding]::new($false)
)
