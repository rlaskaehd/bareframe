"""One-connection, public-key-only loopback SSH fixture; no system SSH config."""
from pathlib import Path
import fcntl
import json
import os
import pty
import select
import shlex
import socket
import struct
import sys
import termios
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.tools/ssh-fixture'))
import paramiko

DIRECTORY = ROOT / 'target/validation/ssh'
DIRECTORY.mkdir(parents=True, exist_ok=True)
host_key = paramiko.RSAKey.generate(2048)
client_key = paramiko.RSAKey.generate(2048)
identity = DIRECTORY / 'identity'
client_key.write_private_key_file(str(identity))
identity.chmod(0o600)


class Server(paramiko.ServerInterface):
    def __init__(self):
        self.ready = threading.Event()
        self.columns, self.rows = 80, 24
        self.master = None
        self.command = None

    def check_auth_publickey(self, username, key):
        return paramiko.AUTH_SUCCESSFUL if username == 'bareframe' and key == client_key else paramiko.AUTH_FAILED

    def get_allowed_auths(self, username):
        return 'publickey'

    def check_channel_request(self, kind, channel_id):
        return paramiko.OPEN_SUCCEEDED if kind == 'session' else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def resize(self):
        if self.master is not None:
            fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack('HHHH', self.rows, self.columns, 0, 0))
        with (DIRECTORY / 'sizes.log').open('a') as stream:
            stream.write(f'{self.rows} {self.columns}\n')

    def check_channel_pty_request(self, channel, term, width, height, pixelwidth, pixelheight, modes):
        self.columns, self.rows = width, height
        self.resize()
        return True

    def check_channel_window_change_request(self, channel, width, height, pixelwidth, pixelheight):
        self.columns, self.rows = width, height
        self.resize()
        return True

    def check_channel_shell_request(self, channel):
        self.ready.set()
        return True

    def check_channel_exec_request(self, channel, command):
        self.command = command.decode('utf-8')
        self.ready.set()
        return True


listener = socket.socket()
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(('127.0.0.1', 0))
listener.listen(1)
listener.settimeout(600)
port = listener.getsockname()[1]
known_hosts = DIRECTORY / 'known_hosts'
known_hosts.write_text(f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n')
command = ['ssh', '-F', '/dev/null', '-o', 'BatchMode=yes', '-o', 'IdentitiesOnly=yes',
           '-o', 'StrictHostKeyChecking=yes', '-o', f'UserKnownHostsFile={known_hosts}',
           '-i', str(identity), '-p', str(port), '-tt', 'bareframe@127.0.0.1']
connect = ROOT / 'target/validation/connect-ssh'
connect.write_text('#!/bin/sh\nexec ' + shlex.join(command) + '\n')
connect.chmod(0o700)
(DIRECTORY / 'connection.json').write_text(json.dumps({'pid': os.getpid(), 'port': port, 'expires_in_seconds': 600}))
print(f'Listening on 127.0.0.1:{port} for one connection', flush=True)

transport = None
child = None
master = None
try:
    peer, _ = listener.accept()
    listener.close()
    transport = paramiko.Transport(peer)
    transport.add_server_key(host_key)
    server = Server()
    transport.start_server(server=server)
    channel = transport.accept(30)
    if channel is None or not server.ready.wait(30):
        raise RuntimeError('SSH fixture did not receive a session request')
    child, master = pty.fork()
    if child == 0:
        os.chdir(ROOT / 'target/validation')
        os.environ['TERM'] = 'xterm-256color'
        os.environ['PS1'] = 'ssh-fixture% '
        args = ['zsh', '-f', '-c', server.command] if server.command else ['zsh', '-f', '-i']
        os.execv('/bin/zsh', args)
    server.master = master
    server.resize()
    deadline = time.monotonic() + 600
    while time.monotonic() < deadline and transport.is_active():
        readable, _, _ = select.select([master, channel], [], [], 1)
        if master in readable:
            try:
                data = os.read(master, 65536)
            except OSError:
                break
            if not data:
                break
            channel.sendall(data)
        if channel in readable:
            data = channel.recv(65536)
            if not data:
                break
            view = memoryview(data)
            while view:
                written = os.write(master, view)
                view = view[written:]
    channel.close()
finally:
    listener.close()
    if master is not None:
        os.close(master)
    if child:
        # Closing the controlling PTY sends HUP. Reap a normally exited shell
        # before attempting any signal; avoid signaling a departed process group.
        waited, _ = os.waitpid(child, os.WNOHANG)
        if waited == 0:
            os.waitpid(child, 0)
    if transport is not None:
        transport.close()
    (DIRECTORY / 'finished').write_text('fixture finished\n')
