#!/usr/bin/env python3
"""
Quick development launcher:
Starts a clean Firefox session with wol-highlights pre-installed and opens wol.jw.org.
"""

import os
import sys
import json
import time
import socket
import tempfile
import subprocess

DIR = os.path.dirname(os.path.abspath(__file__))
XPI_PATH = os.path.join(DIR, "wol-highlights.xpi")
SRC_DIR = os.path.join(DIR, "wol-highlights")

# Re-build XPI if missing
if not os.path.exists(XPI_PATH):
    print("Packaging wol-highlights.xpi...")
    subprocess.run(["bash", os.path.join(DIR, "build.sh")], check=True)

tmpdir = tempfile.mkdtemp(prefix="firefox-wol-")
user_js = os.path.join(tmpdir, "user.js")
with open(user_js, "w") as f:
    f.write("""
user_pref("marionette.enabled", true);
user_pref("marionette.port", 2828);
user_pref("devtools.chrome.enabled", true);
user_pref("devtools.debugger.remote-enabled", true);
""")

cmd = [
    "firefox",
    "--marionette",
    "--profile", tmpdir,
    "--new-instance",
    "https://wol.jw.org"
]

print(f"Starting Firefox with temporary profile at {tmpdir}...")
proc = subprocess.Popen(cmd)

# Connect to Marionette and install addon
time.sleep(2)
try:
    s = socket.create_connection(("127.0.0.1", 2828), timeout=10)
    
    def read_msg():
        length_str = b""
        while True:
            c = s.recv(1)
            if c == b":": break
            length_str += c
        length = int(length_str)
        data = b""
        while len(data) < length:
            chunk = s.recv(length - len(data))
            if not chunk: break
            data += chunk
        return json.loads(data.decode("utf-8"))

    def send_cmd(cmd_id, name, params):
        payload = json.dumps([0, cmd_id, name, params]).encode("utf-8")
        s.sendall(f"{len(payload)}:".encode("utf-8") + payload)
        return read_msg()

    read_msg() # handshake
    send_cmd(1, "WebDriver:NewSession", {})
    res = send_cmd(2, "Addon:Install", {"path": SRC_DIR, "temporary": True})
    print(f"Extension successfully installed into Firefox session! (ID: {res[3].get('value')})")
    s.close()
except Exception as e:
    print(f"Note: Auto-injection reported: {e}. You can also load via about:debugging.")

print("Firefox is running. Press Ctrl+C or close Firefox to exit.")
try:
    proc.wait()
except KeyboardInterrupt:
    proc.terminate()
