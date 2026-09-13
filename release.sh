#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(
    cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
    pwd
)"

VENV_DIR="${SCRIPT_DIR}/.venv"
REQUIREMENTS="${SCRIPT_DIR}/requirements.txt"


die() {
    echo "error: $*" >&2
    exit 1
}


setup_venv() {
    command -v python3 >/dev/null 2>&1 || \
        die "python3 is required to create the virtual environment."

    echo
    echo "Creating virtual environment:"
    echo "  ${VENV_DIR}"
    echo

    python3 -m venv "${VENV_DIR}"

    echo "Installing worker dependencies..."
    "${VENV_DIR}/bin/python" -m pip install --upgrade pip
    "${VENV_DIR}/bin/python" -m pip install -r "${REQUIREMENTS}"

    echo
    echo "Virtual environment is ready."
    echo
    echo "Activate it with:"
    echo
    echo "  source .venv/bin/activate"
    echo
    echo "Then run:"
    echo
    echo "  ./release.sh $*"
    echo

    exit 0
}


# ---------------------------------------------------------------------------
# Environment guard
# ---------------------------------------------------------------------------

if [[ -z "${VIRTUAL_ENV:-}" ]]; then
    echo
    echo "rust-binstall-publisher requires an active Python virtual environment."
    echo

    if [[ -d "${VENV_DIR}" ]]; then
        echo "A project virtual environment already exists:"
        echo
        echo "  ${VENV_DIR}"
        echo
        echo "Activate it first:"
        echo
        echo "  source .venv/bin/activate"
        echo
        echo "Then rerun this command."
        echo

        exit 2
    fi

    echo "No .venv exists yet."
    echo

    if [[ -t 0 ]]; then
        read -r -p "Create and set up .venv now? [Y/n] " answer

        case "${answer:-Y}" in
            y|Y|yes|YES|"")
                setup_venv "$@"
                ;;
            *)
                echo
                echo "Environment setup cancelled."
                echo
                echo "Create it manually with:"
                echo
                echo "  python3 -m venv .venv"
                echo "  source .venv/bin/activate"
                echo "  pip install -r requirements.txt"
                echo
                exit 2
                ;;
        esac
    fi

    die "no virtual environment active and stdin is non-interactive."
fi


# ---------------------------------------------------------------------------
# Extra sanity check:
# ensure we're using the project's environment, not some random venv.
# ---------------------------------------------------------------------------

EXPECTED_VENV="$(cd "${VENV_DIR}" 2>/dev/null && pwd || true)"
ACTIVE_VENV="$(cd "${VIRTUAL_ENV}" 2>/dev/null && pwd || true)"

if [[ -d "${VENV_DIR}" && "${ACTIVE_VENV}" != "${EXPECTED_VENV}" ]]; then
    echo
    echo "warning: an environment is active, but it is not this project's .venv:"
    echo
    echo "  active:   ${VIRTUAL_ENV}"
    echo "  expected: ${VENV_DIR}"
    echo
    read -r -p "Continue anyway? [y/N] " answer

    case "${answer:-N}" in
        y|Y|yes|YES)
            ;;
        *)
            exit 2
            ;;
    esac
fi


exec python -m worker "$@"
