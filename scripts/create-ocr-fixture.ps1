$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$destination = Join-Path $PSScriptRoot '..\crates\platform\tests\fixtures\ocr-fixture.png'
$directory = Split-Path -Parent $destination
[System.IO.Directory]::CreateDirectory($directory) | Out-Null
$bitmap = [System.Drawing.Bitmap]::new(960, 300)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$font = [System.Drawing.Font]::new('Segoe UI', 30)
try {
    $graphics.Clear([System.Drawing.Color]::White)
    $graphics.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
    $lines = @('ScreenTranslate 2', 'Read the world. Stay in the moment.', 'Local OCR. Simple translation.')
    for ($index = 0; $index -lt $lines.Length; $index++) {
        $graphics.DrawString($lines[$index], $font, [System.Drawing.Brushes]::Black, 35, (35 + $index * 78))
    }
    $bitmap.Save($destination, [System.Drawing.Imaging.ImageFormat]::Png)
} finally {
    $font.Dispose()
    $graphics.Dispose()
    $bitmap.Dispose()
}
Write-Output $destination
