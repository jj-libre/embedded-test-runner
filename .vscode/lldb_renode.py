"""Relays a Renode debug session, dropping the stop replies nobody asked for.

Renode's GDB stub reports a stop twice: once accurately, then again as a bare
`S05` raised by the CPU thread as it settles into its single-step wait. LLDB
reads the first, stops, and leaves the second buffered; the next resume reads
that stale packet as its answer and the session runs away. A client that drains
is unaffected, which LLDB is not.

So the examples talk to this instead of to the stub. It forwards everything
except a stop reply arriving while nothing is outstanding, which is precisely
the extra one. Delete this file and go back to `runner-connect` once Renode
reports each stop once.

Registers `renode-connect <example-dir>` and `renode-disconnect`, standing in
for `runner-connect` / `runner-disconnect` in the Renode examples.
"""

import os
import socket
import sys
import threading

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import lldb_runner

# Only these are duplicated. `W`/`X` (the run ended) and `O` (console output)
# are always forwarded: dropping one would hide the end of the session.
DUPLICATED = ("S", "T")

_relay = None


def expects_a_stop(payload):
    """Whether the stub owes a stop reply once it has handled this packet."""
    text = payload.decode("ascii", "replace")
    if text.startswith("vCont;"):
        return True
    if text.startswith("vCont"):
        return False  # `vCont?` only asks which actions are supported
    return text[:1] in ("c", "C", "s", "S", "?")


class Session:
    """How many stop replies the client is still owed."""

    def __init__(self):
        self.owed = 0
        self.guard = threading.Lock()

    def from_client(self, payload):
        if expects_a_stop(payload):
            with self.guard:
                self.owed += 1
        return True

    def from_stub(self, payload):
        if payload[:1].decode("ascii", "replace") not in DUPLICATED:
            return True
        with self.guard:
            if self.owed:
                self.owed -= 1
                return True
        return False


def pump(source, sink, keep):
    """Forward packet by packet, dropping the ones `keep` rejects.

    `$` and `#` are escaped inside packet data, so framing on them is safe in
    both directions, binary `X` payloads included.
    """
    buffer = b""
    while True:
        try:
            chunk = source.recv(4096)
        except OSError:
            break
        if not chunk:
            break

        buffer += chunk
        forward = b""
        while True:
            start = buffer.find(b"$")
            if start < 0:
                forward += buffer
                buffer = b""
                break
            end = buffer.find(b"#", start)
            if end < 0 or len(buffer) < end + 3:
                forward += buffer[:start]
                buffer = buffer[start:]
                break
            forward += buffer[:start]
            packet, payload = buffer[start:end + 3], buffer[start + 1:end]
            buffer = buffer[end + 3:]
            if keep(payload):
                forward += packet

        if forward:
            try:
                sink.sendall(forward)
            except OSError:
                break

    for side in (source, sink):
        try:
            side.close()
        except OSError:
            pass


def serve(listener, stub_port):
    session = Session()
    client, _ = listener.accept()
    stub = socket.create_connection((lldb_runner.HOST, stub_port))
    for source, sink, keep in (
        (client, stub, session.from_client),
        (stub, client, session.from_stub),
    ):
        threading.Thread(target=pump, args=(source, sink, keep), daemon=True).start()


def through_relay(debugger, stub_port):
    global _relay

    listener = socket.socket()
    listener.bind((lldb_runner.HOST, 0))
    listener.listen(1)
    _relay = listener

    threading.Thread(target=serve, args=(listener, stub_port), daemon=True).start()
    return lldb_runner.connect(debugger, listener.getsockname()[1])


def renode_attach(debugger, command, result, internal_dict):
    """Connect through the relay to a stub already serving on a known port.

    For the launch config, where a task starts the runner rather than us.
    """
    port = command.strip()
    if not port.isdigit():
        result.SetError("renode-attach takes the port the stub serves on")
        return

    failure = through_relay(debugger, int(port))
    if failure is not None:
        result.SetError(failure)


def renode_connect(debugger, command, result, internal_dict):
    lldb_runner.start_session(debugger, command, result, through_relay)


def renode_disconnect(debugger, command, result, internal_dict):
    global _relay

    lldb_runner.runner_disconnect(debugger, command, result, internal_dict)
    if _relay is not None:
        _relay.close()
        _relay = None


def __lldb_init_module(debugger, internal_dict):
    for name, function in (
        ("connect", "renode_connect"),
        ("disconnect", "renode_disconnect"),
        ("attach", "renode_attach"),
    ):
        debugger.HandleCommand(
            "command script add --overwrite --function lldb_renode.%s renode-%s"
            % (function, name)
        )
