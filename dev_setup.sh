#!/usr/bin/env bash
set -euo pipefail

# dev_setup.sh
# Interactive developer environment checker/installer for Cosmic Crunchers.
# - Verifies presence of required tools
# - Optionally installs missing tools (prompts first)
# - Supports common Linux package managers and macOS (brew)
# - Installs Rust via rustup and Node via nvm by default
#
# Usage:
#   chmod +x ./dev_setup.sh
#   ./dev_setup.sh

REQUIRED_NODE_MAJOR=22

color() { printf "\033[%sm%s\033[0m" "$1" "$2"; }
info()  { echo "$(color 34 "[INFO]") $*"; }
warn()  { echo "$(color 33 "[WARN]") $*"; }
err()   { echo "$(color 31 "[ERR ]") $*" >&2; }
ok()    { echo "$(color 32 "[ OK ]") $*"; }

command_exists() { command -v "$1" >/dev/null 2>&1; }

ask_yes_no() {
  local prompt="${1:-Proceed?} [y/N]: "
  read -r -p "$prompt" ans || true
  case "${ans:-}" in
    y|Y|yes|YES) return 0 ;;
    *) return 1 ;;
  esac
}

detect_pm() {
  if [[ "$OSTYPE" == darwin* ]]; then
    echo "brew"; return
  fi
  if command_exists apt; then echo "apt"; return; fi
  if command_exists dnf; then echo "dnf"; return; fi
  if command_exists pacman; then echo "pacman"; return; fi
  echo "unknown"
}

pm_install() {
  local pm="$1"; shift
  local pkgs=("$@")
  case "$pm" in
    brew)   brew update && brew install "${pkgs[@]}";;
    apt)    sudo apt update && sudo apt install -y "${pkgs[@]}";;
    dnf)    sudo dnf install -y "${pkgs[@]}";;
    pacman) sudo pacman -Sy --noconfirm "${pkgs[@]}";;
    *)      err "Unsupported package manager for auto-install."; return 1;;
  esac
}

ensure_basic_tools() {
  local pm="$1"
  local missing=()
  for c in git curl wget jq make cmake; do
    command_exists "$c" || missing+=("$c")
  done
  if ((${#missing[@]})); then
    warn "Missing basic tools: ${missing[*]}"
    if [[ "$pm" == "unknown" ]]; then
      err "Cannot auto-install. Please install: ${missing[*]}"
      return 1
    fi
    if ask_yes_no "Install basic tools (${missing[*]}) via $pm?"; then
      pm_install "$pm" "${missing[@]}"
    else
      err "User declined installing basic tools."
      return 1
    fi
  else
    ok "Basic tools present."
  fi
}

ensure_rust() {
  if command_exists cargo && command_exists rustup; then
    ok "Rust toolchain present: $(cargo --version)"
  else
    warn "Rust toolchain (rustup/cargo) not found."
    if ask_yes_no "Install Rust via rustup now?"; then
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
      # shellcheck disable=SC1090
      if [[ -s "$HOME/.cargo/env" ]]; then
        # shellcheck disable=SC1090
        source "$HOME/.cargo/env"
      fi
    else
      err "Rust toolchain is required. Aborting."
      return 1
    fi
  fi
  rustup default stable || true
  rustup component add rustfmt clippy || true
}

node_major() {
  node -v | sed -E 's/^v([0-9]+).*/\1/' 2>/dev/null || echo 0
}

ensure_nvm() {
  if [[ -s "$HOME/.nvm/nvm.sh" ]]; then
    # shellcheck disable=SC1090
    source "$HOME/.nvm/nvm.sh"
    return 0
  fi
  warn "nvm not found."
  if ask_yes_no "Install nvm (Node Version Manager)?"; then
    curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
    if [[ -s "$HOME/.nvm/nvm.sh" ]]; then
      # shellcheck disable=SC1090
      source "$HOME/.nvm/nvm.sh"
      ok "nvm installed and sourced."
      return 0
    fi
    # try to source common profiles
    if [[ -s "$HOME/.bashrc" ]]; then source "$HOME/.bashrc" || true; fi
    if [[ -s "$HOME/.zshrc" ]]; then source "$HOME/.zshrc" || true; fi
    if [[ -s "$HOME/.nvm/nvm.sh" ]]; then
      # shellcheck disable=SC1090
      source "$HOME/.nvm/nvm.sh"
      ok "nvm sourced from profile."
      return 0
    fi
    warn "nvm install finished but could not source it automatically. Please restart your shell and re-run this script."
    return 1
  else
    return 1
  fi
}

ensure_node() {
  if command_exists node && command_exists npm; then
    local major
    major="$(node_major || echo 0)"
    if (( major >= REQUIRED_NODE_MAJOR )); then
      ok "Node present: $(node -v), npm $(npm -v)"
      return 0
    else
      warn "Node version too old: $(node -v). Need >= v${REQUIRED_NODE_MAJOR}."
    fi
  else
    warn "Node/npm not found."
  fi

  # Prefer installing via nvm
  if ensure_nvm; then
    info "Installing Node LTS via nvm..."
    nvm install --lts || true
    nvm alias default 'lts/*' || true
    ok "Installed Node $(node -v) via nvm."
    return 0
  fi

  # Fallback to system package manager if user declined nvm
  local pm="$1"
  warn "Falling back to system package manager."
  case "$pm" in
    brew)   if ask_yes_no "Install node via brew?"; then pm_install brew node; fi ;;
    apt)    if ask_yes_no "Install nodejs/npm via apt?"; then pm_install apt nodejs npm; fi ;;
    dnf)    if ask_yes_no "Install nodejs via dnf?"; then pm_install dnf nodejs; fi ;;
    pacman) if ask_yes_no "Install nodejs/npm via pacman?"; then pm_install pacman nodejs npm; fi ;;
    *)      err "Unknown package manager; cannot install Node."; return 1 ;;
  esac
}

print_summary() {
  echo
  echo "===== Summary ====="
  for c in git curl wget jq make cmake rustup cargo node npm; do
    if command_exists "$c"; then
      printf " - %-6s %s\n" "$c" "$(command -v "$c")"
    else
      printf " - %-6s MISSING\n" "$c"
    fi
  done
  echo "==================="
}

main() {
  info "Detecting environment..."
  local pm
  pm="$(detect_pm)"
  info "Package manager: $pm"

  ensure_basic_tools "$pm"
  ensure_rust
  ensure_node "$pm"

  print_summary

  ok "Development environment check complete."
  echo "If commands were installed by this script, open a new terminal or source your shell profile to pick up changes."
}

main "$@"
