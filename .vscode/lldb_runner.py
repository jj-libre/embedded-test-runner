"""Runs one test in an emulator and connects LLDB to its GDB stub.

Shared by every example, whichever venue it runs in. Registers
`runner-connect <example-dir>` for a launch config's processCreateCommands, and
`runner-disconnect` for its exitCommands. The test comes from the debuggee's
argv, which is what rust-analyzer's Debug lens fills in, so the lens picks the
test.

Cargo is invoked rather than the runner directly, so the example's own
.cargo/config.toml supplies the target, the runner and its board.
"""

import os
import re
import shlex
import socket
import subprocess
import tempfile
import time

import lldb

# Cargo names a test binary `<target>-<hash>`.
ARTIFACT = re.compile(r"^(?P<target>.+)-[0-9a-f]{8,}$")

# Printed by the runner once it has a port to serve on.
READY = "waiting for a debugger on port"

# Printed by a runner that hands the test over instead of running it.
NAMED = "waiting for a debugger to run"
CONNECT_TIMEOUT = 20.0
POLL_INTERVAL = 0.05

# The stub is served on the loopback address, spelled numerically: `localhost`
# can resolve to ::1 first and miss a stub listening only on IPv4.
HOST = "127.0.0.1"

# Every address, which is what a venue binds when it takes no address at all.
WILDCARD = "0.0.0.0"

# The runner of the session in progress, so it can be stopped with the session.
_runner = None


def free_port():
    with socket.socket() as sock:
        sock.bind((HOST, 0))
        return sock.getsockname()[1]


def port_taken(port):
    """Whether a stub is serving the port, whichever address it chose.

    The address is the venue's: QEMU binds the loopback one the runner names,
    Renode's StartGdbServer takes no address and binds the wildcard. Neither
    probe alone sees both, because a specific bind does not collide with
    another process's wildcard bind, nor a wildcard bind with a specific one.
    """
    for address in (HOST, WILDCARD):
        with socket.socket() as sock:
            try:
                sock.bind((address, port))
            except OSError:
                return True
    return False


def test_target(elf):
    """The `[[test]]` name behind a built artifact, for `cargo test --test`.

    Only test targets are supported; a `--lib` or `--bin` target would need a
    different cargo flag.
    """
    stem = os.path.splitext(os.path.basename(elf))[0]
    matched = ARTIFACT.match(stem)
    return matched.group("target") if matched else stem


def debuggee_args(target):
    """The arguments the editor chose, passed on to libtest untouched so that
    flags like --include-ignored survive."""
    info = target.GetLaunchInfo()
    return [info.GetArgumentAtIndex(i) for i in range(info.GetNumArguments())]


def stop(process):
    """Stop the runner and the emulator under it: the emulator is its child, and
    outlives a runner killed on its own."""
    if process is None or process.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/PID", str(process.pid), "/T", "/F"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    else:
        os.killpg(os.getpgid(process.pid), 15)


def spawn(directory, elf, arguments, port, log):
    command = ["cargo", "test", "--test", test_target(elf), "--"] + arguments
    environment = dict(
        os.environ, EMBEDDED_TEST_DEBUG="true", EMBEDDED_TEST_GDB=str(port)
    )
    print("runner-connect: %s" % shlex.join(command))

    # A file rather than a pipe: nothing here drains it, and a full pipe would
    # block the runner mid-session.
    # Its own process group, so the emulator under it can be stopped too.
    return subprocess.Popen(
        command,
        cwd=directory,
        env=environment,
        stderr=log,
        start_new_session=os.name != "nt",
    )


def await_announcement(process, log):
    """Wait for the runner to announce its stub, or report why it never did.

    Connecting blind is not an option: a `gdb-remote` to a port nobody is
    listening on blocks for about two minutes before failing, so one wrong
    guess costs more than the whole session.
    """
    deadline = time.time() + CONNECT_TIMEOUT

    while True:
        log.seek(0)
        output = log.read().decode("utf-8", "replace")
        if READY in output:
            return None
        if process.poll() is not None:
            return output.strip() or "the runner exited with code %d" % process.returncode
        if time.time() >= deadline:
            return "the runner never announced a stub"
        time.sleep(POLL_INTERVAL)


def await_listening(process, port):
    """Wait for the emulator to bind the announced port, which the runner says
    nothing about: it announces before starting the emulator.

    Probed by binding rather than by connecting, because the stub serves a
    single client and a dropped probe would read as a debugger detaching.
    """
    deadline = time.time() + CONNECT_TIMEOUT

    while True:
        if port_taken(port):
            return None
        if process.poll() is not None:
            return "the runner exited before the emulator served port %d" % port
        if time.time() >= deadline:
            return "the emulator never served port %d" % port
        time.sleep(POLL_INTERVAL)


def connect(debugger, port):
    result = lldb.SBCommandReturnObject()
    debugger.GetCommandInterpreter().HandleCommand(
        "gdb-remote %s:%d" % (HOST, port), result
    )
    return None if result.Succeeded() else result.GetError()


def start_session(debugger, command, result, attach):
    """Run the test the editor chose, then attach to the stub it serves.

    `attach(debugger, port)` returns a failure message or None. It is the one
    step that differs between connecting straight to a venue and connecting
    through something that sits in between.
    """
    global _runner

    directory = command.strip().strip('"')
    if not directory:
        result.SetError("connect takes the example directory to build in")
        return

    target = debugger.GetSelectedTarget()
    arguments = debuggee_args(target)
    if not [argument for argument in arguments if not argument.startswith("-")]:
        result.SetError(
            "no test in the debuggee's arguments: start this from the Debug "
            "lens above a #[test], which is what supplies the test name"
        )
        return

    stop(_runner)
    port = free_port()
    log = tempfile.TemporaryFile()
    _runner = spawn(directory, target.GetExecutable().fullpath, arguments, port, log)

    failure = await_announcement(_runner, log)
    if failure is None:
        failure = await_listening(_runner, port)
    if failure is None:
        failure = attach(debugger, port)

    if failure is not None:
        stop(_runner)
        result.SetError(failure)
        return

    # The stub reports no load address, so the module has to be placed at the
    # addresses it was linked for, or no breakpoint ever resolves.
    _ = target.SetModuleLoadAddress(target.GetModuleAtIndex(0), 0)


def named_test(directory, elf, arguments):
    """Ask the runner which test the editor's selection names.

    It answers and exits rather than running anything, so this waits for the
    whole command instead of polling a log the way the stub venues do.
    """
    command = ["cargo", "test", "--test", test_target(elf), "--"] + arguments
    environment = dict(os.environ, EMBEDDED_TEST_DEBUG="true")
    print("host-launch: %s" % shlex.join(command))

    finished = subprocess.run(
        command,
        cwd=directory,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    output = finished.stdout.decode("utf-8", "replace")

    for line in output.splitlines():
        if NAMED in line:
            return line.split(NAMED, 1)[1].strip(), None
    return None, output.strip() or "the runner named no test to debug"


def host_launch(debugger, command, result, internal_dict):
    """Run the one test the editor chose, under the debugger.

    Unused: no adapter tried survives a hook creating the process, so the host
    example debugs from its launch config instead. Kept for one that does.
    """
    directory = command.strip().strip(chr(34))
    if not directory:
        result.SetError("host-launch takes the example directory to build in")
        return

    target = debugger.GetSelectedTarget()
    arguments = debuggee_args(target)
    if not [argument for argument in arguments if not argument.startswith("-")]:
        result.SetError(
            "no test in the debuggee's arguments: start this from the Debug "
            "lens above a #[test], which is what supplies the test name"
        )
        return

    test, failure = named_test(directory, target.GetExecutable().fullpath, arguments)
    if failure is not None:
        result.SetError(failure)
        return

    info = target.GetLaunchInfo()
    info.SetArguments(["run", test], False)

    # Stopped at entry, because the adapter resolves breakpoints after this
    # command returns: a process already running has finished the test by then.
    info.SetLaunchFlags(info.GetLaunchFlags() | lldb.eLaunchFlagStopAtEntry)

    failed = lldb.SBError()
    target.Launch(info, failed)
    if failed.Fail():
        result.SetError(failed.GetCString())


def runner_connect(debugger, command, result, internal_dict):
    start_session(debugger, command, result, connect)


def runner_disconnect(debugger, command, result, internal_dict):
    global _runner

    stop(_runner)
    _runner = None


def __lldb_init_module(debugger, internal_dict):
    for name, function in (("connect", "runner_connect"), ("disconnect", "runner_disconnect")):
        debugger.HandleCommand(
            "command script add --overwrite --function lldb_runner.%s runner-%s"
            % (function, name)
        )
    debugger.HandleCommand(
        "command script add --overwrite --function lldb_runner.host_launch host-launch"
    )
