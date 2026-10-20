# Downloads the conversion programs Kumquat bundles into resources\binaries\ (Windows x64).
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-binaries.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-binaries.ps1 -Only pdfium,pandoc
#
# Everything comes from the projects' own release pages. LibreOffice isn't
# bundled on Windows; Kumquat finds an installed copy in Program Files.
# Windows PowerShell 5.1 (built into Windows 10/11) is enough.

param(
    [string[]]$Only = @()
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is very slow with the progress bar.
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Root = Split-Path -Parent $PSScriptRoot
$Dest = Join-Path $Root 'resources\binaries'
$Work = Join-Path ([IO.Path]::GetTempPath()) ("kumquat-fetch-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $Work, (Join-Path $Dest 'lib') | Out-Null

# -Only a,b and -Only "a,b" both work.
$Only = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
function Wanted($name) { $Only.Count -eq 0 -or $Only -contains $name }

function Say($text) { Write-Host "`n==> $text" -ForegroundColor Yellow }

function Fetch($url, $file) {
    Write-Host "    $url"
    Invoke-WebRequest -Uri $url -OutFile $file -UseBasicParsing
}

# Asset URL from a GitHub release (latest, or a tag), matching a regex.
function GitHubAsset($repo, $pattern, $tag) {
    $api = if ($tag) { "https://api.github.com/repos/$repo/releases/tags/$tag" } else { "https://api.github.com/repos/$repo/releases/latest" }
    $headers = @{ 'User-Agent' = 'kumquat-fetch-binaries' }
    # Unauthenticated API calls are limited to 60 an hour, which CI runners share.
    if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
    $release = Invoke-RestMethod -Uri $api -Headers $headers
    $urls = $release.assets | ForEach-Object { $_.browser_download_url } | Where-Object { $_ -match $pattern }
    if (-not $urls) { throw "No asset in $repo matches $pattern" }
    # Highest version first when several match.
    @($urls | Sort-Object -Descending)[0]
}

# 7zr.exe unpacks .7z archives (ImageMagick) and the 7-Zip installer itself.
$SevenR = Join-Path $Work '7zr.exe'
function Get7zr {
    if (-not (Test-Path $SevenR)) {
        Fetch (GitHubAsset 'ip7z/7zip' '/7zr\.exe$') $SevenR
    }
    $SevenR
}

function Replace-Dir($from, $to) {
    if (Test-Path $to) { Remove-Item -Recurse -Force $to }
    Move-Item $from $to
}

try {
    if (Wanted 'ffmpeg') {
        Say 'ffmpeg + ffprobe (BtbN static GPL build)'
        $url = GitHubAsset 'BtbN/FFmpeg-Builds' 'ffmpeg-n[0-9.]+-latest-win64-gpl-[0-9.]+\.zip$' 'latest'
        Fetch $url "$Work\ffmpeg.zip"
        Expand-Archive "$Work\ffmpeg.zip" "$Work\ffmpeg"
        $bin = Get-ChildItem "$Work\ffmpeg" -Recurse -Filter ffmpeg.exe | Select-Object -First 1
        Copy-Item (Join-Path $bin.DirectoryName 'ffmpeg.exe'), (Join-Path $bin.DirectoryName 'ffprobe.exe') $Dest -Force
    }

    if (Wanted 'magick') {
        Say 'ImageMagick (portable Q16-HDRI build)'
        $url = GitHubAsset 'ImageMagick/ImageMagick' 'portable-Q16-HDRI-x64\.7z$'
        Fetch $url "$Work\magick.7z"
        & (Get7zr) x -y "-o$Work\magick" "$Work\magick.7z" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'could not unpack ImageMagick' }
        $exe = Get-ChildItem "$Work\magick" -Recurse -Filter magick.exe | Select-Object -First 1
        Replace-Dir $exe.DirectoryName (Join-Path $Dest 'imagemagick')
    }

    if (Wanted 'pandoc') {
        Say 'pandoc'
        $url = GitHubAsset 'jgm/pandoc' 'windows-x86_64\.zip$'
        Fetch $url "$Work\pandoc.zip"
        Expand-Archive "$Work\pandoc.zip" "$Work\pandoc"
        $exe = Get-ChildItem "$Work\pandoc" -Recurse -Filter pandoc.exe | Select-Object -First 1
        Copy-Item $exe.FullName $Dest -Force
    }

    if (Wanted '7zz') {
        Say '7-Zip (for RAR and 7z)'
        # The x64 installer is itself a 7z archive holding 7z.exe and 7z.dll (RAR support lives in the DLL).
        $url = GitHubAsset 'ip7z/7zip' '/7z[0-9]+-x64\.exe$'
        Fetch $url "$Work\7z-setup.exe"
        & (Get7zr) x -y "-o$Work\7zip" "$Work\7z-setup.exe" 7z.exe 7z.dll License.txt | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'could not unpack 7-Zip' }
        Replace-Dir "$Work\7zip" (Join-Path $Dest '7zip')
    }

    if (Wanted 'pdfium') {
        Say 'PDFium (bblanchon/pdfium-binaries)'
        $url = GitHubAsset 'bblanchon/pdfium-binaries' '/pdfium-win-x64\.tgz$'
        $tgz = Join-Path $Work 'pdfium.tgz'
        $out = Join-Path $Work 'pdfium'
        Fetch $url $tgz
        New-Item -ItemType Directory -Force -Path $out | Out-Null
        # tar.exe ships with Windows 10 1803 and later.
        tar -xzf $tgz -C $out
        if ($LASTEXITCODE -ne 0) { throw 'could not unpack PDFium' }
        Copy-Item (Join-Path $out 'bin\pdfium.dll') (Join-Path $Dest 'lib\pdfium.dll') -Force
    }

    Say 'Done. In resources\binaries:'
    Get-ChildItem $Dest | ForEach-Object {
        $size = if ($_.PSIsContainer) { (Get-ChildItem $_.FullName -Recurse -File | Measure-Object Length -Sum).Sum } else { $_.Length }
        '{0,8:N1} MB  {1}' -f ($size / 1MB), $_.Name
    }
}
finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}
