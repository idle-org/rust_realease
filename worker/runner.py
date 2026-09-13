"""Subprocess execution without a shell; secrets are redacted even in errors."""
import os
import shlex
import subprocess
from collections import deque
from pathlib import Path
from rich.console import Console

console = Console(markup=False)
verbose = False
_secret_names = {'GITEA_TOKEN'}


def register_secret(name: str):
    _secret_names.add(name)


def redact(value: str) -> str:
    for name in _secret_names:
        secret = os.environ.get(name)
        if secret:
            value = value.replace(secret, '[REDACTED]')
    return value


class CommandError(RuntimeError):
    pass


def run(*args: str, cwd: Path | None = None, capture: bool = True,
        env: dict[str, str] | None = None, timeout: float | None = None) -> str:
    command = redact(shlex.join(args))
    if verbose:
        console.print('$ ' + command)
    environment = os.environ.copy()
    # Build scripts must not inherit publishing credentials.
    for name in _secret_names:
        environment.pop(name, None)
    environment.update(env or {})
    try:
        if capture:
            proc = subprocess.run(args, cwd=cwd, env=environment, text=True,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  timeout=timeout)
            output, error, status = proc.stdout, proc.stderr, proc.returncode
        else:
            tail = deque(maxlen=40)
            with subprocess.Popen(args, cwd=cwd, env=environment, text=True,
                                  stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as proc:
                for line in proc.stdout:
                    tail.append(line)
                    console.print(redact(line.rstrip()))
                status = proc.wait()
            output, error = '', ''.join(tail)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise CommandError(f'could not run {command}: {redact(str(exc))}') from None
    if status:
        raise CommandError(f'command failed ({status}): {command}\n{redact(error.strip())}')
    return output.rstrip('\n')
