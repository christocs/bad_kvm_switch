#!/usr/bin/env pwsh
# Installs prerequisites for this repo's MCP servers (see ../.mcp.json).
# Safe to re-run -- every step checks before installing.

$ErrorActionPreference = "Stop"

Write-Host "Installing docsrs-mcp (Rust docs lookups)..."
cargo install docsrs-mcp

if (Get-Command gh -ErrorAction SilentlyContinue) {
    Write-Host "GitHub CLI already installed."
} else {
    Write-Host "Installing GitHub CLI..."
    winget install --id GitHub.cli -e --accept-package-agreements --accept-source-agreements
    # winget updates the registry-persisted PATH, but this session's PATH is
    # a stale copy from before the install -- refresh it so `gh` resolves
    # below without needing a new shell.
    $machinePath = [System.Environment]::GetEnvironmentVariable('Path', 'Machine')
    $userPath = [System.Environment]::GetEnvironmentVariable('Path', 'User')
    $env:PATH = "$machinePath;$userPath"
}

gh auth status 2>&1 | Out-Null
if ($LASTEXITCODE -eq 0) {
    Write-Host "GitHub CLI already authenticated."
} else {
    Write-Host ""
    Write-Host "GitHub CLI needs authentication -- this opens a browser, so run it yourself:"
    Write-Host "  gh auth login"
}

Write-Host ""
Write-Host "Playwright MCP needs no separate install step -- npx fetches it on first use."
Write-Host ""
Write-Host "Done. Restart VSCode / your Claude Code session, then approve the MCP servers"
Write-Host "when prompted (or run '/mcp' inside a session to check status)."
exit 0
