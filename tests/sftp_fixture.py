"""Isolated SFTP protocol fixture, for tests only. Never install as a service."""
import errno
import os
import socket
import threading
from pathlib import Path
import paramiko
from paramiko.sftp import CMD_INIT, CMD_VERSION, CMD_EXTENDED


class Auth(paramiko.ServerInterface):
    def __init__(self, username, password):
        self.username, self.password = username, password

    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL if (username, password) == (self.username, self.password) else paramiko.AUTH_FAILED

    def get_allowed_auths(self, username):
        return 'password'

    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == 'session' else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED


class Handle(paramiko.SFTPHandle):
    def stat(self):
        return paramiko.SFTPAttributes.from_stat(os.fstat(self.file.fileno()))

    def chattr(self, attr):
        if attr.st_size is not None:
            self.file.truncate(attr.st_size)
        if attr.st_mode is not None:
            os.fchmod(self.file.fileno(), attr.st_mode)
        return paramiko.SFTP_OK


def status(fn):
    def call(*args):
        try:
            return fn(*args)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
    return call


class Files(paramiko.SFTPServerInterface):
    def __init__(self, server, root):
        super().__init__(server)
        self.root = Path(root).resolve()

    def path(self, path):
        resolved = Path(os.path.abspath(self.root / path.lstrip('/')))
        if not resolved.is_relative_to(self.root):
            raise PermissionError(errno.EACCES, 'Outside test fixture')
        if 'denied' in resolved.parts:
            raise PermissionError(errno.EACCES, 'Permission fixture')
        return resolved

    @status
    def stat(self, path):
        return paramiko.SFTPAttributes.from_stat(self.path(path).stat())

    @status
    def lstat(self, path):
        return paramiko.SFTPAttributes.from_stat(self.path(path).lstat())

    @status
    def list_folder(self, path):
        result = []
        for p in self.path(path).iterdir():
            attr = paramiko.SFTPAttributes.from_stat(p.lstat())
            attr.filename = p.name
            result.append(attr)
        return result

    @status
    def open(self, path, flags, attr):
        fd = os.open(self.path(path), flags, (attr.st_mode or 0o600) & 0o777)
        file = os.fdopen(fd, 'r+b' if flags & os.O_RDWR else 'wb' if flags & os.O_WRONLY else 'rb', buffering=0)
        handle = Handle(flags)
        handle.file = file
        if flags & (os.O_WRONLY | os.O_RDWR):
            handle.writefile = file
        if not flags & os.O_WRONLY:
            handle.readfile = file
        return handle

    @status
    def mkdir(self, path, attr):
        self.path(path).mkdir(mode=(attr.st_mode or 0o750) & 0o777)
        return paramiko.SFTP_OK

    @status
    def rename(self, old, new):
        old, new = self.path(old), self.path(new)
        if new.exists() or new.is_symlink():
            return paramiko.SFTP_FAILURE
        os.rename(old, new)
        return paramiko.SFTP_OK

    @status
    def posix_rename(self, old, new):
        os.replace(self.path(old), self.path(new))
        return paramiko.SFTP_OK

    @status
    def remove(self, path):
        self.path(path).unlink()
        return paramiko.SFTP_OK

    @status
    def rmdir(self, path):
        self.path(path).rmdir()
        return paramiko.SFTP_OK

    @status
    def chattr(self, path, attr):
        paramiko.SFTPServer.set_file_attr(str(self.path(path)), attr)
        return paramiko.SFTP_OK


class Server(paramiko.SFTPServer):
    def _send_server_version(self):
        kind, data = self._read_packet()
        if kind != CMD_INIT:
            raise paramiko.SSHException('Expected SFTP INIT')
        message = paramiko.Message()
        message.add_int(3)
        message.add_string('fsync@openssh.com')
        message.add_string('1')
        message.add_string('posix-rename@openssh.com')
        message.add_string('1')
        self._send_packet(CMD_VERSION, message)
        return 3

    def _process(self, kind, request, message):
        if kind != CMD_EXTENDED:
            return super()._process(kind, request, message)
        tag = message.get_text()
        if tag == 'fsync@openssh.com':
            handle = self.file_table.get(message.get_binary())
            if handle is None:
                self._send_status(request, paramiko.SFTP_FAILURE)
                return
            handle.file.flush()
            os.fsync(handle.file.fileno())
            self._send_status(request, paramiko.SFTP_OK)
        elif tag == 'posix-rename@openssh.com':
            self._send_status(request, self.server.posix_rename(message.get_text(), message.get_text()))
        else:
            self._send_status(request, paramiko.SFTP_OP_UNSUPPORTED)


def start(root, keyfile, username, password, port):
    key = paramiko.Ed25519Key.from_private_key_file(str(keyfile))
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(('127.0.0.1', port))
    listener.listen(16)
    transports = []

    def accept_one(client):
        transport = paramiko.Transport(client)
        transports.append(transport)
        transport.add_server_key(key)
        transport.set_subsystem_handler('sftp', Server, Files, root=str(root))
        try:
            transport.start_server(server=Auth(username, password))
        except (EOFError, paramiko.SSHException):
            transport.close()

    def loop():
        while True:
            try:
                client, _ = listener.accept()
            except OSError:
                return
            threading.Thread(target=accept_one, args=(client,), daemon=True).start()

    threading.Thread(target=loop, daemon=True).start()
    return listener, transports
