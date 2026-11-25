"""Run real production media HTTP tests using two private loopback fixtures.
The anonymous native client gets its test endpoint only in the child env. No
saved account state or real Discord/player/capture resource is used.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import subprocess
import sys
import threading

ROOT = Path(__file__).resolve().parents[1]
state = {'target_requests': 0, 'loop_requests': 0}
lock = threading.Lock()


class FixtureServer(ThreadingHTTPServer):
    fixture_kind: str = 'source'


class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, format: str, *args: object) -> None:
        pass

    def reply(self, status, body=b'', headers=None):
        self.send_response(status)
        self.send_header('Connection', 'close')
        for key, value in (headers or {}).items():
            self.send_header(key, str(value))
        if not headers or ('Content-Length' not in headers and 'Transfer-Encoding' not in headers):
            self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass  # A correct early limit refusal can close before body transfer.

    def do_GET(self):
        path = self.path.split('?', 1)[0]
        if isinstance(self.server, FixtureServer) and self.server.fixture_kind == 'target':
            with lock:
                state['target_requests'] += 1
            self.reply(200, b'unexpected-third-party-fixture')
        elif path == '/redirect-outside':
            self.reply(302, headers={'Location': target_url + '/private'})
        elif path == '/redirect-safe':
            self.reply(302, headers={'Location': '/direct'})
        elif path == '/direct':
            self.reply(200, b'original-fixture')
        elif path == '/sized':
            self.reply(200, b'x' * 64)
        elif path == '/chunked':
            body = b'x' * 64
            wire = f'{len(body):x}\r\n'.encode() + body + b'\r\n0\r\n\r\n'
            self.reply(200, wire, {'Transfer-Encoding': 'chunked'})
        elif path == '/hostile-length':
            self.reply(200, headers={'Content-Length': 1 << 40})
        elif path == '/target-count':
            with lock:
                body = str(state['target_requests']).encode()
            self.reply(200, body)
        elif path == '/loop':
            with lock:
                state['loop_requests'] += 1
            self.reply(302, headers={'Location': '/loop'})
        elif path == '/loop-count':
            with lock:
                body = str(state['loop_requests']).encode()
            self.reply(200, body)
        else:
            self.reply(404)


source = FixtureServer(('127.0.0.1', 0), Handler)
target = FixtureServer(('127.0.0.1', 0), Handler)
source.fixture_kind = 'source'
target.fixture_kind = 'target'
target_url = f'http://127.0.0.1:{target.server_port}'
threads = [threading.Thread(target=server.serve_forever, kwargs={'poll_interval': 0.02}, daemon=True)
           for server in (source, target)]
for thread in threads:
    thread.start()
env = {key: value for key, value in os.environ.items()
       if key.lower() not in {'http_proxy', 'https_proxy', 'all_proxy', 'no_proxy'}}
env['NO_PROXY'] = '127.0.0.1,localhost'
env['YAPFAST_DISCORD_TEST_SERVER'] = f'http://127.0.0.1:{source.server_port}'
command = [sys.executable, str(ROOT / 'testing/run_fullres_pixel_tests.py'),
           '--source', str(ROOT / 'testing/media_download_regression.rs'),
           '--suite', 'media-downloads', '--test-threads=1', *sys.argv[1:]]
try:
    result = subprocess.run(command, cwd=ROOT, env=env,
                            creationflags=subprocess.BELOW_NORMAL_PRIORITY_CLASS)
finally:
    for server in (source, target):
        server.shutdown()
        server.server_close()
    for thread in threads:
        thread.join(timeout=2)
print('Loopback fixture counters:', state, flush=True)
raise SystemExit(result.returncode)
