<#
.SYNOPSIS
    Runs the end to end test against a throwaway server.

.DESCRIPTION
    Starts a Game Save server in a temporary directory, creates an account and
    an API key, then runs the Rust end to end test against it. Everything is
    removed at the end.

.EXAMPLE
    ./scripts/e2e.ps1
#>
param(
    [int]$Port = 0,
    [switch]$Keep
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot

# A free port of its own: another server may well be listening on the usual
# one, and then the test would talk to that one instead.
if ($Port -eq 0) {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $Port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    $listener.Stop()
    Start-Sleep -Milliseconds 200
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("gamesave-e2e-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force -Path $work | Out-Null

$serverDir = Join-Path $root "server"
$clientDir = Join-Path $root "desktop"

function Invoke-Native {
    <#
        Runs a native command, capturing both streams, so that the output of
        a tool does not pollute the error stream of PowerShell.
    #>
    param(
        [Parameter(Mandatory = $true)][string]$Exe,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [string]$WorkingDirectory
    )
    $out = Join-Path $work ([guid]::NewGuid().ToString("N") + ".out")
    $err = $out + ".err"
    $start = @{
        FilePath               = $Exe
        ArgumentList           = $Arguments
        NoNewWindow            = $true
        Wait                   = $true
        PassThru               = $true
        RedirectStandardOutput = $out
        RedirectStandardError  = $err
    }
    if ($WorkingDirectory) { $start["WorkingDirectory"] = $WorkingDirectory }
    $process = Start-Process @start
    return [pscustomobject]@{
        Code = $process.ExitCode
        Out  = (Get-Content $out -Raw -ErrorAction SilentlyContinue)
        Err  = (Get-Content $err -Raw -ErrorAction SilentlyContinue)
    }
}

Write-Host "==> building the server" -ForegroundColor Cyan
$build = Invoke-Native -Exe "go" -Arguments @("build", "-o", "gamesave.exe", "./cmd/gamesave") -WorkingDirectory $serverDir
if ($build.Code -ne 0) {
    Write-Host $build.Out $build.Err
    throw "the server does not build"
}

$binary = if ($IsLinux -or $IsMacOS) { "gamesave" } else { "gamesave.exe" }
$exe = Join-Path $serverDir $binary

$config = @"
server:
  listen: ":$Port"
  data_dir: "$($work -replace '\\','/')"
  tls:
    mode: "selfsigned"
    san: ["localhost", "127.0.0.1"]
    validity_days: 3650
    fingerprint_file: "certs/fingerprint.txt"
auth:
  require_api_key: true
"@
Set-Content -Path (Join-Path $work "config.yaml") -Value $config -Encoding UTF8

Write-Host "==> creating the account and the key" -ForegroundColor Cyan
& $exe user add e2e --config (Join-Path $work "config.yaml") | Out-Null
$keyOutput = & $exe key create --user e2e --label e2e --config (Join-Path $work "config.yaml") 2>&1 | Out-String
$key = [regex]::Match($keyOutput, 'gsk_[A-Za-z0-9_\-]+').Value
if (-not $key) { throw "could not read the API key from the output" }

Write-Host "==> starting the server" -ForegroundColor Cyan
$server = Start-Process -FilePath $exe `
    -ArgumentList "serve", "--config", (Join-Path $work "config.yaml") `
    -RedirectStandardOutput (Join-Path $work "server.out") `
    -RedirectStandardError (Join-Path $work "server.err") `
    -PassThru -WindowStyle Hidden

try {
    # Wait for the server to answer.
    $ready = $false
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Milliseconds 250
        $null = & curl.exe -sk "https://127.0.0.1:$Port/healthz" 2>$null
        if ($LASTEXITCODE -eq 0) { $ready = $true; break }
    }
    if (-not $ready) {
        # The server may have found the port taken: say so, instead of letting
        # the test talk to somebody else's server.
        Get-Content (Join-Path $work "server.err") -ErrorAction SilentlyContinue | Select-Object -Last 5
        throw "the server did not start on port $Port"
    }

    $fingerprint = [regex]::Match(
        (Get-Content (Join-Path $work "certs\fingerprint.txt") -Raw),
        "authority\s*:\s*([0-9A-Fa-f:]+)").Groups[1].Value
    if (-not $fingerprint) { throw "could not read the fingerprint" }

    # The server that answers has to be the one that was started here.
    $served = & curl.exe -sk "https://127.0.0.1:$Port/v1/cert" 2>$null | ConvertFrom-Json
    if ($served.fingerprint -ne $fingerprint) {
        throw "another server is answering on port $Port"
    }

    Write-Host "==> running the end to end test" -ForegroundColor Cyan
    $env:GAMESAVE_E2E_URL = "https://127.0.0.1:$Port"
    $env:GAMESAVE_E2E_KEY = $key
    $env:GAMESAVE_E2E_FINGERPRINT = $fingerprint

    $test = Invoke-Native -Exe "cargo" `
        -Arguments @("test", "--test", "e2e", "--", "--nocapture", "--test-threads=1") `
        -WorkingDirectory $clientDir
    Write-Host $test.Out
    if ($test.Err) { Write-Host $test.Err }
    if ($test.Code -ne 0) { throw "the end to end test failed" }
} finally {
    if ($server -and -not $server.HasExited) { $server | Stop-Process -Force }
    Remove-Item Env:GAMESAVE_E2E_URL -ErrorAction SilentlyContinue
    Remove-Item Env:GAMESAVE_E2E_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:GAMESAVE_E2E_FINGERPRINT -ErrorAction SilentlyContinue
    if (-not $Keep) {
        Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
    } else {
        Write-Host "the server was left in $work" -ForegroundColor DarkGray
    }
}

Write-Host "==> done" -ForegroundColor Green
