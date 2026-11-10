#!/usr/bin/env python3
"""Real HTTP -> Rust -> SSH -> SFTP tests; no storage mock in the application.
Run after building: .tools/venv/bin/python tests/integration.py [--serve] [--large]
The fixture uses temporary data and random credentials, only on loopback.
"""
import concurrent.futures
import hashlib
import http.client
import json
import os
import secrets
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.parse
from pathlib import Path
from sftp_fixture import start

REPO = Path(__file__).resolve().parents[1]
APP_PORT, SFTP_PORT = 18080, 22222
BASE = f'http://127.0.0.1:{APP_PORT}'
passed = []


def check(name, condition):
    assert condition, name
    passed.append(name)
    print(f'PASS {name}', flush=True)


def request(method, path, data=None, headers=None, expected=200):
    connection = http.client.HTTPConnection('127.0.0.1', APP_PORT, timeout=45)
    h = {'Origin': BASE, 'X-GlassFTP': '1'}
    h.update(headers or {})
    if data is not None and not isinstance(data, bytes):
        data = json.dumps(data).encode()
        h['Content-Type'] = 'application/json'
    connection.request(method, path, body=data, headers=h)
    response = connection.getresponse()
    body = response.read()
    assert response.status == expected, (method, path, response.status, body[:400])
    result = json.loads(body) if response.getheader('Content-Type', '').startswith('application/json') else body
    result_headers = dict(response.getheaders())
    connection.close()
    return result, result_headers


def get_info(path):
    return request('GET', '/api/v1/files/info?path=' + urllib.parse.quote(path))[0]


def upload(path, data, fragment=131072):
    u, _ = request('POST', '/api/v1/uploads', {'path': path, 'size': len(data)}, expected=201)
    for offset in range(0, len(data), fragment):
        request('PATCH', f'/api/v1/uploads/{u["id"]}', data[offset:offset+fragment], {'Upload-Offset': str(offset)})
    return request('POST', f'/api/v1/uploads/{u["id"]}/complete', {})[0]


def wait_ready(proc):
    for _ in range(150):
        if proc.poll() is not None:
            raise RuntimeError('Backend exited; inspect test-results/server.log')
        try:
            request('GET', '/health')
            return
        except (ConnectionError, OSError):
            time.sleep(.1)
    raise TimeoutError('Backend startup')


def main():
    (REPO / 'test-results').mkdir(exist_ok=True)
    (REPO / 'test-results/integration.json').unlink(missing_ok=True)
    state = tempfile.TemporaryDirectory(prefix='glassftp-test-')
    work = Path(state.name)
    root = work / 'sftp'
    shared = root / 'shared'
    shared.mkdir(parents=True)
    (shared / 'Documents').mkdir()
    (shared / 'Images').mkdir()
    (shared / 'Projets').mkdir()
    (shared / 'Bienvenue.md').write_text('# Bienvenue dans GlassFTP\n\nVos fichiers restent sur votre réseau local.\n', encoding='utf-8')
    (shared / 'exemple.rs').write_text('fn main() {\n    println!("Bonjour, équipe !");\n}\n', encoding='utf-8')
    (shared / 'denied').mkdir()
    (work / 'outside.txt').write_text('PRIVATE TEST SENTINEL')
    (shared / 'escape').symlink_to(work / 'outside.txt')
    key = work / 'host_key'
    subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(key)], check=True)
    fingerprint = subprocess.check_output(['ssh-keygen', '-lf', str(key) + '.pub', '-E', 'sha256'], text=True).split()[1]
    password = secrets.token_urlsafe(24)
    listener, transports = start(root, key, 'glassftp-test', password, SFTP_PORT)
    env = os.environ | {'APP_PORT': str(APP_PORT), 'APP_BASE_URL': BASE, 'SFTP_HOST': '127.0.0.1', 'SFTP_PORT': str(SFTP_PORT), 'SFTP_USERNAME': 'glassftp-test', 'SFTP_PASSWORD': password, 'SFTP_ROOT': '/shared', 'SFTP_HOST_KEY_SHA256': fingerprint, 'DATABASE_PATH': str(work / 'state.sqlite'), 'ASSET_DIRECTORY': str(REPO / 'dist'), 'SFTP_TIMEOUT_SECONDS': '5'}
    binary = REPO / 'target/debug/glassftp-server'
    env['RUST_LOG'] = 'glassftp_server=info'
    log = (REPO / 'test-results/server.log').open('w')
    proc = None
    try:
        for field, invalid in [('SFTP_PASSWORD', 'wrong-password'), ('SFTP_HOST_KEY_SHA256', 'SHA256:' + 'A' * 43)]:
            bad = subprocess.run([str(binary)], env=env | {field: invalid}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)
            check(f'reject {field}', bad.returncode != 0)
        proc = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
        wait_ready(proc)
        check('health uses real SFTP', request('GET', '/health')[0]['storage'] == 'sftp')
        listing = request('GET', '/api/v1/files?path=/')[0]
        check('listing hides internal areas and symlinks', all(not e['name'].startswith('.glassftp') and e['name'] != 'escape' for e in listing['entries']))
        for path in ['/../outside.txt', '/.glassftp/uploads', '/Documents/../Bienvenue.md']:
            request('GET', '/api/v1/files/info?path=' + urllib.parse.quote(path), expected=400)
        check('traversal and reserved paths rejected', True)
        request('GET', '/api/v1/download?path=/escape', expected=403)
        request('GET', '/api/v1/files?path=/denied', expected=403)
        request('GET', '/api/v1/files/info?path=/missing', expected=404)
        check('symlink, permissions and missing file errors', True)
        request('POST', '/api/v1/folders', {'path': '/wrong-origin'}, {'Origin': 'https://evil.invalid'}, expected=403)
        request('GET', '/health', headers={'Host': 'evil.invalid'}, expected=403)
        check('origin and DNS rebinding checks', True)
        request('POST', '/api/v1/folders', {'path': '/Équipe'}, expected=201)
        request('POST', '/api/v1/folders', {'path': '/Équipe'}, expected=409)
        check('mkdir and explicit collision', True)
        data = bytes(range(256)) * 8192
        u, _ = request('POST', '/api/v1/uploads', {'path': '/Équipe/data.bin', 'size': len(data)}, expected=201)
        request('PATCH', f'/api/v1/uploads/{u["id"]}', data[:65536], {'Upload-Offset': '1'}, expected=409)
        request('PATCH', f'/api/v1/uploads/{u["id"]}', data[:65536], {'Upload-Offset': '0'})
        request('GET', '/api/v1/files/info?path=' + urllib.parse.quote('/Équipe/data.bin'), expected=404)
        request('POST', f'/api/v1/uploads/{u["id"]}/complete', {}, expected=409)
        check('offset validation and invisible partial upload', True)
        # Send an incomplete HTTP body. Committed offset must not advance.
        sock = socket.create_connection(('127.0.0.1', APP_PORT))
        payload = f'PATCH /api/v1/uploads/{u["id"]} HTTP/1.1\r\nHost: 127.0.0.1:{APP_PORT}\r\nOrigin: {BASE}\r\nX-GlassFTP: 1\r\nUpload-Offset: 65536\r\nContent-Length: 131072\r\n\r\n'.encode() + data[65536:70000]
        sock.sendall(payload)
        sock.close()
        time.sleep(.2)
        check('interrupted chunk does not commit', request('GET', f'/api/v1/uploads/{u["id"]}')[0]['offset'] == 65536)
        events = http.client.HTTPConnection('127.0.0.1', APP_PORT, timeout=20)
        events.request('GET', '/api/v1/events')
        event_response = events.getresponse()
        assert event_response.status == 200
        proc.send_signal(signal.SIGINT); proc.wait(timeout=15)
        event_response.read()
        events.close()
        check('graceful shutdown closes live SSE clients', proc.returncode == 0)
        proc = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log); wait_ready(proc)
        check('committed offset survives restart', request('GET', f'/api/v1/uploads/{u["id"]}')[0]['offset'] == 65536)
        request('PATCH', f'/api/v1/uploads/{u["id"]}', data[65536:], {'Upload-Offset': '65536'})
        done = request('POST', f'/api/v1/uploads/{u["id"]}/complete', {})[0]
        check('upload confirmed only after finalization', done['status'] == 'complete')
        downloaded, headers = request('GET', '/api/v1/download?path=' + urllib.parse.quote('/Équipe/data.bin'))
        check('download byte equality', hashlib.sha256(downloaded).digest() == hashlib.sha256(data).digest())
        partial, headers = request('GET', '/api/v1/download?path=' + urllib.parse.quote('/Équipe/data.bin'), headers={'Range': 'bytes=17-1024'}, expected=206)
        check('HTTP range exact bytes', partial == data[17:1025] and headers['content-range'] == f'bytes 17-1024/{len(data)}')
        request('GET', '/api/v1/download?path=' + urllib.parse.quote('/Équipe/data.bin'), headers={'Range': f'bytes={len(data)}-'}, expected=416)
        check('unsatisfiable range rejected', True)
        upload('/Équipe/notes.txt', b'initial')
        text = request('GET', '/api/v1/text?path=' + urllib.parse.quote('/Équipe/notes.txt'))[0]
        saved = request('PUT', '/api/v1/text', text | {'content': 'updated'})[0]
        request('PUT', '/api/v1/text', text | {'content': 'overwrite'}, expected=409)
        check('editor saves and detects stale content hash', saved['content'] == 'updated')
        item = get_info('/Équipe/notes.txt')
        move = {'source': item['path'], 'destination': '/Équipe/renamed.txt', 'version': item['version']}
        def race():
            try: return request('POST', '/api/v1/files/move', move, expected=204)[0]
            except AssertionError: return 'conflict'
        with concurrent.futures.ThreadPoolExecutor(2) as executor:
            results = list(executor.map(lambda _: race(), range(2)))
        check('concurrent rename has one winner', results.count('conflict') == 1)
        info = get_info('/Équipe/renamed.txt')
        request('POST', '/api/v1/files/copy', {'source': info['path'], 'destination': '/Équipe/copy.txt', 'version': info['version']}, expected=201)
        check('SFTP copy preserves bytes', request('GET', '/api/v1/download?path=' + urllib.parse.quote('/Équipe/copy.txt'))[0] == b'updated')
        trash = request('POST', '/api/v1/files/trash', {'path': info['path'], 'version': info['version']})[0]
        request('GET', '/api/v1/files/info?path=' + urllib.parse.quote(info['path']), expected=404)
        request('POST', f'/api/v1/trash/{trash["id"]}/restore', {'destination': '/Équipe/copy.txt'}, expected=409)
        request('POST', f'/api/v1/trash/{trash["id"]}/restore', {'destination': info['path']}, expected=204)
        check('trash and restore refuse collisions', get_info(info['path'])['size'] == 7)
        trash = request('POST', '/api/v1/files/trash', {'path': info['path'], 'version': get_info(info['path'])['version']})[0]
        request('DELETE', f'/api/v1/trash/{trash["id"]}', expected=204)
        check('permanent deletion', request('GET', '/api/v1/trash')[0] == [])
        with concurrent.futures.ThreadPoolExecutor(3) as executor:
            result = list(executor.map(lambda i: upload(f'/parallel-{i}.bin', data), range(3)))
        check('three simultaneous uploads', all(u['status'] == 'complete' for u in result))
        upload('/empty.txt', b'')
        check('zero-length file', request('GET', '/api/v1/download?path=/empty.txt')[0] == b'')
        upload('/unsafe.html', b'<script>alert(document.domain)</script>')
        _, headers = request('GET', '/api/v1/download?path=/unsafe.html&inline=true')
        check('active content forced to sandboxed attachment', headers['content-type'] == 'application/octet-stream' and headers['content-disposition'].startswith('attachment') and 'sandbox' in headers['content-security-policy'])
        # Five users can do useful work while the transfer pool is bounded to four.
        with concurrent.futures.ThreadPoolExecutor(5) as executor:
            jobs = [executor.submit(upload, f'/mixed-{i}.bin', data) for i in range(2)]
            jobs += [executor.submit(request, 'GET', '/api/v1/download?path=/parallel-0.bin') for _ in range(2)]
            jobs.append(executor.submit(request, 'GET', '/api/v1/files?path=/'))
            for job in jobs: job.result()
        check('five simultaneous clients with mixed operations', True)
        connection = http.client.HTTPConnection('127.0.0.1', APP_PORT, timeout=30)
        connection.request('GET', '/api/v1/download?path=/parallel-0.bin')
        response = connection.getresponse()
        assert response.status == 200
        response.read(4096)
        connection.close()
        check('interrupted download leaves service available', request('GET', '/health')[0]['status'] == 'ok')
        large_report = None
        if '--large' in sys.argv:
            size = 2 * 1024**3
            block = bytes(range(256)) * (4 * 1024**2 // 256)
            expected_hash = hashlib.sha256()
            started = time.monotonic()
            peak_rss = 0
            def sample_rss():
                return int(subprocess.check_output(['ps', '-o', 'rss=', '-p', str(proc.pid)], text=True).strip()) * 1024
            baseline_rss = sample_rss()
            u, _ = request('POST', '/api/v1/uploads', {'path': '/large-2GiB.bin', 'size': size}, expected=201)
            for offset in range(0, size, len(block)):
                request('PATCH', f'/api/v1/uploads/{u["id"]}', block, {'Upload-Offset': str(offset)})
                expected_hash.update(block)
                peak_rss = max(peak_rss, sample_rss())
                if (offset + len(block)) % (256 * 1024**2) == 0:
                    print(f'LARGE upload {(offset + len(block)) // 1024**2} / 2048 MiB', flush=True)
            request('POST', f'/api/v1/uploads/{u["id"]}/complete', {})
            connection = http.client.HTTPConnection('127.0.0.1', APP_PORT, timeout=60)
            connection.request('GET', '/api/v1/download?path=/large-2GiB.bin')
            response = connection.getresponse()
            assert response.status == 200
            observed_hash = hashlib.sha256()
            received = 0
            while chunk := response.read(65536):
                observed_hash.update(chunk)
                received += len(chunk)
                if received % (64 * 1024**2) == 0: peak_rss = max(peak_rss, sample_rss())
                if received % (256 * 1024**2) == 0: print(f'LARGE download {received // 1024**2} / 2048 MiB', flush=True)
            connection.close()
            check('2 GiB streaming upload/download SHA256 equality', received == size and observed_hash.digest() == expected_hash.digest())
            check('backend RSS stays below 256 MiB during 2 GiB transfer', peak_rss < 256 * 1024**2)
            large_report = {'bytes': size, 'backend_baseline_rss_bytes': baseline_rss, 'backend_sampled_peak_rss_bytes': peak_rss, 'elapsed_upload_and_download_seconds': round(time.monotonic() - started, 2), 'platform': sys.platform, 'backend_profile': 'debug', 'fixture': 'Paramiko loopback', 'rss_sampling': 'after each 4 MiB upload fragment and each 64 MiB download segment'}
        report = {'passed': len(passed), 'checks': passed, 'fixture': 'Paramiko SSH/SFTP on loopback', 'largest_file_bytes': len(data), 'parallel_uploads': 3}
        if large_report:
            report['large_transfer'] = large_report
            report['largest_file_bytes'] = large_report['bytes']
        (REPO / 'test-results/integration.json').write_text(json.dumps(report, indent=2))
        print(f'ALL {len(passed)} CHECKS PASSED', flush=True)
        if '--serve' in sys.argv:
            print(f'QA READY {BASE}', flush=True)
            while True: time.sleep(1)
    finally:
        if proc and proc.poll() is None:
            proc.send_signal(signal.SIGINT)
            try: proc.wait(timeout=10)
            except subprocess.TimeoutExpired: proc.kill(); proc.wait()
        listener.close()
        for transport in transports: transport.close()
        log.close()
        state.cleanup()


if __name__ == '__main__':
    main()
