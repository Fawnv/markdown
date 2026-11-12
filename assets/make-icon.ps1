# Builds assets/app.ico (PNG-compressed entries: 16, 20, 24, 32, 40, 48, 64, 96, 128, 256 px).
#
#   With your own art:  put a square PNG (256x256 or larger, transparent background) at
#                       assets/icon.png and run this script. Optional hand-tuned small sizes:
#                       assets/icon-16.png, icon-24.png, icon-32.png are used as-is if present.
#   Ready-made .ico:    just copy it to assets/app.ico — this script is not needed.
#   -Default:           draws the original placeholder icon (blue tile + Markdown mark by Dustin Curtis, CC0).
#
#   powershell -ExecutionPolicy Bypass -File assets\make-icon.ps1
#   cargo build --release
param([switch]$Default)
Add-Type -AssemblyName System.Drawing
$ErrorActionPreference = 'Stop'

$sizes = 16, 20, 24, 32, 40, 48, 64, 96, 128, 256
$source = Join-Path $PSScriptRoot 'icon.png'

function Resize-Png([System.Drawing.Image]$img, [int]$n) {
    $bmp = New-Object System.Drawing.Bitmap $n, $n, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = 'HighQualityBicubic'
    $g.SmoothingMode = 'AntiAlias'
    $g.PixelOffsetMode = 'HighQuality'
    $g.CompositingQuality = 'HighQuality'
    $g.Clear([System.Drawing.Color]::Transparent)
    $g.DrawImage($img, 0, 0, $n, $n)
    $g.Dispose()
    return $bmp
}

function New-RoundRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    return $p
}

function Draw-Default([int]$n) {
    $m = @(@(30,98),@(30,30),@(50,30),@(70,55),@(90,30),@(110,30),@(110,98),@(90,98),@(90,59),@(70,84),@(50,59),@(50,98))
    $a = @(@(155,98),@(125,65),@(145,65),@(145,30),@(165,30),@(165,65),@(185,65))
    $bmp = New-Object System.Drawing.Bitmap $n, $n, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.PixelOffsetMode = 'HighQuality'
    $g.Clear([System.Drawing.Color]::Transparent)
    $inset = [Math]::Max(0.5, $n * 0.03)
    $rect = New-RoundRect $inset $inset ($n - 2 * $inset) ($n - 2 * $inset) ($n * 0.2)
    $top = [System.Drawing.Color]::FromArgb(255, 0x2f, 0x81, 0xf7)
    $bot = [System.Drawing.Color]::FromArgb(255, 0x09, 0x69, 0xda)
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush (New-Object System.Drawing.PointF 0, 0), (New-Object System.Drawing.PointF 0, $n), $top, $bot
    $g.FillPath($brush, $rect)
    $s = ($n * 0.72) / 155.0
    $ox = ($n - 155 * $s) / 2 - 30 * $s
    $oy = ($n - 68 * $s) / 2 - 30 * $s
    $white = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::White)
    foreach ($poly in @($m, $a)) {
        $pts = foreach ($pt in $poly) { New-Object System.Drawing.PointF ([float]($ox + $pt[0] * $s)), ([float]($oy + $pt[1] * $s)) }
        $g.FillPolygon($white, [System.Drawing.PointF[]]$pts)
    }
    $g.Dispose()
    return $bmp
}

if (-not (Test-Path $source) -and -not $Default) {
    "No assets\icon.png found; assets\app.ico left unchanged. Use -Default to draw the placeholder icon."
    return
}
$master = if (Test-Path $source) { [System.Drawing.Image]::FromFile($source) } else { $null }
if ($master) {
    if ($master.Width -ne $master.Height) { Write-Warning "icon.png is $($master.Width)x$($master.Height); it should be square." }
    if ($master.Width -lt 256) { Write-Warning "icon.png is smaller than 256x256; large sizes will look blurry." }
    "source: $source ($($master.Width)x$($master.Height))"
} else {
    "source: built-in default icon"
}

$pngs = @()
foreach ($n in $sizes) {
    $override = Join-Path $PSScriptRoot "icon-$n.png"
    if (Test-Path $override) {
        $img = [System.Drawing.Image]::FromFile($override)
        $bmp = Resize-Png $img $n
        $img.Dispose()
        "  ${n}px: $override"
    } elseif ($master) {
        $bmp = Resize-Png $master $n
    } else {
        $bmp = Draw-Default $n
    }
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $pngs += , @($n, $ms.ToArray())
}
if ($master) { $master.Dispose() }

$out = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter $out
$bw.Write([UInt16]0); $bw.Write([UInt16]1); $bw.Write([UInt16]$pngs.Count)
$offset = 6 + 16 * $pngs.Count
foreach ($e in $pngs) {
    $n = $e[0]; $bytes = $e[1]
    $dim = if ($n -ge 256) { 0 } else { $n }
    $bw.Write([byte]$dim); $bw.Write([byte]$dim); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([UInt16]1); $bw.Write([UInt16]32)
    $bw.Write([UInt32]$bytes.Length); $bw.Write([UInt32]$offset)
    $offset += $bytes.Length
}
foreach ($e in $pngs) { $bw.Write([byte[]]$e[1]) }
$bw.Flush()
$dest = Join-Path $PSScriptRoot 'app.ico'
[System.IO.File]::WriteAllBytes($dest, $out.ToArray())
"wrote $dest ($($out.Length) bytes, $($pngs.Count) sizes)"
