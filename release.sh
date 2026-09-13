#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
VENV_DIR="${SCRIPT_DIR}/.venv"

die() {
    echo "error: $*" >&2
    exit 2
}

activation_guidance() {
    if [[ "$PWD" == "$SCRIPT_DIR" ]]; then
        echo '  source .venv/bin/activate'
    else
        printf '  source %q\n' "${VENV_DIR}/bin/activate"
    fi
}

if [[ -z "${VIRTUAL_ENV:-}" ]]; then
    echo 'rust-binstall-publisher requires an active Python virtual environment.'
    if [[ -d "$VENV_DIR" ]]; then
        echo 'Activate the project environment first:'
        activation_guidance
        exit 2
    fi
    [[ -t 0 ]] || die 'no virtual environment active and stdin is non-interactive.'
    read -r -p 'No .venv exists. Create and install dependencies now? [Y/n] ' answer
    case "${answer:-Y}" in
        y|Y|yes|YES)
            command -v python3 >/dev/null || die 'python3 is required.'
            python3 -m venv "$VENV_DIR"
            "$VENV_DIR/bin/python" -m pip install -r "$SCRIPT_DIR/requirements.txt"
            echo 'Virtual environment ready. Activate it, then rerun your command:'
            activation_guidance
            exit 0
            ;;
        *) die 'environment setup cancelled.' ;;
    esac
fi

EXPECTED_VENV="$(cd "$VENV_DIR" 2>/dev/null && pwd -P || true)"
ACTIVE_VENV="$(cd "$VIRTUAL_ENV" 2>/dev/null && pwd -P || true)"
if [[ ! -d "$VENV_DIR" || -z "$ACTIVE_VENV" || "$ACTIVE_VENV" != "$EXPECTED_VENV" ]]; then
    echo "warning: another environment is active: $VIRTUAL_ENV" >&2
    echo "Expected: $VENV_DIR" >&2
    [[ -t 0 ]] || die 'another virtual environment is active and stdin is non-interactive.'
    read -r -p 'Continue anyway? [y/N] ' answer
    case "${answer:-N}" in
        y|Y|yes|YES) ;;
        *) exit 2 ;;
    esac
fi

[[ -x "$VIRTUAL_ENV/bin/python" ]] || die 'active environment has no Python interpreter.'
# Keep SOURCE relative to the caller, but import only this tool's worker package.
export PYTHONPATH="$SCRIPT_DIR"
exec "$VIRTUAL_ENV/bin/python" -P -m worker "$@"
