#!/bin/bash
# Installs everything ForgeVideo's AI mode needs on this Mac. Safe to run again: each step is
# skipped when it is already done. Run it from the AI panel ("Install AI tools…") or:
#   bash scripts/install-ai.sh
set -euo pipefail

MODEL_DIR="$HOME/Library/Application Support/ForgeVideo/models"
MODEL="$MODEL_DIR/ggml-base.en.bin"
MODEL_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"

step() { printf '\n\033[1;32m==> %s\033[0m\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

step "ForgeVideo AI setup"
echo "This installs Homebrew (if needed), ffmpeg, whisper.cpp, the English speech model (~150 MB)"
echo "and Claude Code. You may be asked for your Mac password by the Homebrew installer."

if ! have brew; then
  if [ -x /opt/homebrew/bin/brew ]; then
    eval "$(/opt/homebrew/bin/brew shellenv)"
  else
    step "Installing Homebrew"
    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    eval "$(/opt/homebrew/bin/brew shellenv)"
  fi
fi

for formula in ffmpeg whisper.cpp; do
  if brew list --formula "$formula" >/dev/null 2>&1; then
    echo "$formula: already installed"
  else
    step "Installing $formula"
    brew install "$formula"
  fi
done

if [ -s "$MODEL" ]; then
  echo "speech model: already at $MODEL"
else
  step "Downloading the speech model"
  mkdir -p "$MODEL_DIR"
  curl -L --fail --progress-bar -o "$MODEL.part" "$MODEL_URL"
  mv "$MODEL.part" "$MODEL"
fi

if have claude || [ -x "$HOME/.local/bin/claude" ] || [ -x "$HOME/.claude/local/claude" ]; then
  echo "Claude Code: already installed"
else
  step "Installing Claude Code"
  brew install --cask claude-code
fi

step "Checking"
ffmpeg -hide_banner -encoders 2>/dev/null | grep -q h264_videotoolbox && echo "ffmpeg: ok (h264_videotoolbox)" || echo "ffmpeg: WARNING, no h264_videotoolbox encoder"
have whisper-cli && echo "whisper-cli: ok" || echo "whisper-cli: WARNING, not on PATH"
[ -s "$MODEL" ] && echo "speech model: ok" || echo "speech model: WARNING, missing"
(have claude || [ -x "$HOME/.local/bin/claude" ]) && echo "Claude Code: ok" || echo "Claude Code: WARNING, not found"

step "Done"
echo "Go back to ForgeVideo and press \"Sign in to Claude\" in the AI panel. You can close this window."
