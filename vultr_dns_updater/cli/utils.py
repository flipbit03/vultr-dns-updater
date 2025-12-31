"""Shared CLI utilities."""

from __future__ import annotations

from rich.console import Console

console = Console()
error_console = Console(stderr=True)


def print_error(message: str) -> None:
    """Print an error message to stderr."""
    error_console.print(f"[bold red]Error:[/bold red] {message}")


def print_success(message: str) -> None:
    """Print a success message."""
    console.print(f"[bold green]Success:[/bold green] {message}")


def print_info(message: str) -> None:
    """Print an info message."""
    console.print(f"[bold blue]Info:[/bold blue] {message}")


def print_warning(message: str) -> None:
    """Print a warning message."""
    console.print(f"[bold yellow]Warning:[/bold yellow] {message}")
