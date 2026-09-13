from __future__ import annotations

import subprocess
from pathlib import Path

from rich.console import Console


console = Console()


class CommandError(RuntimeError):
    pass


def run(
    *args: str,
    cwd: Path | None = None,
    capture: bool = True,
) -> str:
    console.print(
        "[dim]$ " + " ".join(args) + "[/dim]"
    )

    proc = subprocess.run(
        args,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )

    if proc.returncode != 0:
        stderr = proc.stderr.strip() if proc.stderr else ""

        raise CommandError(
            f"command failed ({proc.returncode}): "
            f"{' '.join(args)}\n{stderr}"
        )

    if not capture:
        return ""

    return proc.stdout.strip()
