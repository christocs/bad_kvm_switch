#!/usr/bin/env bash
# Installs prerequisites for this repo's MCP servers (see ../.mcp.json).
# Safe to re-run -- every step checks before installing.
set -euo pipefail

echo "Installing docsrs-mcp (Rust docs lookups)..."
cargo install docsrs-mcp

if command -v gh >/dev/null 2>&1; then
    echo "GitHub CLI already installed."
elif command -v apt >/dev/null 2>&1; then
    echo "Installing GitHub CLI (apt)..."
    # Official install steps: https://github.com/cli/cli/blob/trunk/docs/install_linux.md
    (type -p wget >/dev/null || (sudo apt update && sudo apt install wget -y)) \
        && sudo mkdir -p -m 755 /etc/apt/keyrings \
        && out=$(mktemp) && wget -nv -O "$out" https://cli.github.com/packages/githubcli-archive-keyring.gpg \
        && cat "$out" | sudo tee /etc/apt/keyrings/githubcli-archive-keyring.gpg > /dev/null \
        && sudo chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg \
        && sudo mkdir -p -m 755 /etc/apt/sources.list.d \
        && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null \
        && sudo apt update \
        && sudo apt install gh -y
else
    echo "Don't know how to install gh automatically on this distro -- see https://cli.github.com/"
    exit 1
fi

if gh auth status >/dev/null 2>&1; then
    echo "GitHub CLI already authenticated."
else
    echo ""
    echo "GitHub CLI needs authentication -- this opens a browser, so run it yourself:"
    echo "  gh auth login"
fi

echo ""
echo "Playwright MCP needs no separate install step -- npx fetches it on first use."
echo ""
echo "Done. Restart VSCode / your Claude Code session, then approve the MCP servers"
echo "when prompted (or run '/mcp' inside a session to check status)."
