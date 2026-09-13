from __future__ import annotations

from pathlib import Path

import typer

from rich.console import Console
from rich.table import Table

from . import __version__
from .project import (
    acquire_project,
    cleanup_project,
)
from .manifest import inspect_project


app = typer.Typer(
    no_args_is_help=True,
    pretty_exceptions_enable=False,
)

console = Console()


DEFAULT_TARGETS = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-pc-windows-gnu",
]


@app.command()
def release(
    source: str = typer.Argument(
        ...,
        help="Git repository URL or local project path.",
    ),
    tag: str | None = typer.Option(
        None,
        "--tag",
        "-t",
        help="Git tag to release.",
    ),
    target: list[str] | None = typer.Option(
        None,
        "--target",
        help="Build target. May be repeated.",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Inspect and plan without changing anything.",
    ),
    keep_workdir: bool = typer.Option(
        False,
        "--keep-workdir",
        help="Keep temporary cloned repositories.",
    ),
):
    targets = target or DEFAULT_TARGETS

    console.print(
        f"[bold]rust-binstall-publisher[/bold] "
        f"[dim]v{__version__}[/dim]"
    )

    console.print()

    project_source = acquire_project(source)

    try:
        project = inspect_project(
            project_source.path
        )

        table = Table(
            title="Project",
            show_header=False,
        )

        table.add_column("Property")
        table.add_column("Value")

        table.add_row(
            "Path",
            str(project.root),
        )

        table.add_row(
            "Package",
            project.package_name,
        )

        table.add_row(
            "Version",
            project.version,
        )

        table.add_row(
            "Repository",
            project.repository or "[red]missing[/red]",
        )

        table.add_row(
            "Binary",
            ", ".join(project.binaries),
        )

        table.add_row(
            "Commit",
            project.commit or "unknown",
        )

        if tag:
            table.add_row(
                "Requested tag",
                tag,
            )

        console.print(table)
        console.print()

        target_table = Table(
            title="Planned build matrix"
        )

        target_table.add_column("Target")
        target_table.add_column("Artifact")

        for triple in targets:
            suffix = ".exe" if "windows" in triple else ""

            binary = project.binaries[0]

            target_table.add_row(
                triple,
                f"{binary}{suffix}",
            )

        console.print(target_table)

        console.print()

        if len(project.binaries) != 1:
            console.print(
                "[yellow]warning:[/yellow] "
                "multiple binaries are not fully supported yet."
            )

        if project.repository is None:
            console.print(
                "[red]error:[/red] "
                "package.repository is required for release publishing."
            )

            raise typer.Exit(1)

        if tag:
            expected = f"v{project.version}"

            if tag != expected:
                console.print(
                    f"[red]error:[/red] "
                    f"tag {tag!r} does not match "
                    f"package version {project.version!r} "
                    f"(expected {expected!r})."
                )

                raise typer.Exit(1)

        console.print()

        if dry_run:
            console.print(
                "[cyan]DRY RUN[/cyan] — "
                "no files changed, built, or uploaded."
            )
        else:
            console.print(
                "[yellow]Release execution is not implemented yet.[/yellow]"
            )

    finally:
        if project_source.temporary:
            if keep_workdir:
                console.print(
                    f"\nTemporary workdir kept at "
                    f"[bold]{project_source.path}[/bold]"
                )
            else:
                cleanup_project(project_source)
