"""Windows integration test: edit a disposable world through the game's normal input handlers."""
import argparse
import ctypes
from ctypes import wintypes
from contextlib import contextmanager
from pathlib import Path
import struct
import subprocess
import tempfile
import time
import zlib


def wait_for(read, label, seconds=8):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = read()
        if value:
            return value
        time.sleep(0.05)
    raise RuntimeError(f"Délai dépassé : {label}")


def read_edits(path):
    data = path.read_bytes()
    assert data[:4] == b"PVXD" and zlib.crc32(data[:-4]) == int.from_bytes(data[-4:], "little")
    records = list(struct.iter_unpack("<bbbB", data[20:-4]))
    assert len(records) == int.from_bytes(data[16:20], "little")
    return {(x, y, z): block for x, y, z, block in records}


@contextmanager
def open_game(executable, path):
    win = ctypes.windll.user32
    process = subprocess.Popen([executable, "--world", path, "--resolution", "800x450"])
    callback = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    found = []

    @callback
    def visit(hwnd, _):
        pid = wintypes.DWORD()
        win.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        title = ctypes.create_unicode_buffer(100)
        win.GetWindowTextW(hwnd, title, len(title))
        if pid.value == process.pid and title.value.startswith("PRIMAL VOXEL"):
            found.append(hwnd)
        return True

    def find():
        assert process.poll() is None, "Le client a quitté avant le test"
        win.EnumWindows(visit, 0)
        return found and found[0]

    hwnd = None
    try:
        hwnd = wait_for(find, "fenêtre du processus de test")

        def post(message, key=0, data=0):
            if not win.PostMessageW(wintypes.HWND(hwnd), message, key, data):
                raise ctypes.WinError()

        # F5 must replace the snapshot. This also waits for world and mesh initialization.
        original_time = path.stat().st_mtime_ns

        def ready():
            post(0x100, 0x74, (0x3F << 16) | 1)
            post(0x101, 0x74, (0x3F << 16) | 0xC0000001)
            return path.stat().st_mtime_ns != original_time

        wait_for(ready, "sauvegarde par F5")
        yield post
    finally:
        if hwnd:
            win.PostMessageW(wintypes.HWND(hwnd), 0x10, 0, 0)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise
    assert process.returncode == 0


def click(post, right=False):
    down, up, button = (0x204, 0x205, 2) if right else (0x201, 0x202, 1)
    point = (225 << 16) | 400
    post(down, button, point)
    time.sleep(0.09)
    post(up, 0, point)
    time.sleep(0.09)


def verify(executable, path):
    # Two stones along the initial camera ray. All later mutations go through the GUI.
    target, backing = (-1, 11, 4), (-2, 11, 4)
    data = struct.pack("<4sHHQI", b"PVXD", 1, 1, 20_261_120, 2)
    data += b"".join(struct.pack("<bbbB", *p, 3) for p in sorted([target, backing]))
    path.write_bytes(data + struct.pack("<I", zlib.crc32(data)))
    with open_game(executable, path) as post:
        click(post)  # Capture. The first click must preserve both stones.
        assert read_edits(path) == {backing: 3, target: 3}
        click(post)
        wait_for(lambda: read_edits(path) == {backing: 3}, "destruction par clic gauche")
    with open_game(executable, path) as post:
        assert read_edits(path) == {backing: 3}
        click(post)
        post(0x100, ord("7"), (0x08 << 16) | 1)
        post(0x101, ord("7"), (0x08 << 16) | 0xC0000001)
        time.sleep(0.12)
        click(post, right=True)
        wait_for(lambda: read_edits(path) == {backing: 3, target: 7}, "pose de cristal par clic droit")
    print("PASS Windows : F5, capture, destruction, fermeture, relecture, sélection 7 et repose.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="primal-window-test-") as directory:
        verify(args.executable.resolve(), Path(directory) / "fixture.pvx")
