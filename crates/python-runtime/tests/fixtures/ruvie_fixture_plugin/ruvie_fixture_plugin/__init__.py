"""Pure-Python fixture package for the embedded CPython runtime."""


def curve(time: float) -> float:
    return time * time + 1.0


def register(host) -> None:
    """Future plugin contract fixture; host registration is not in slice one."""
    host.register_command("fixture.curve", curve)


def unregister(host) -> None:
    host.unregister_command("fixture.curve")
