#!/usr/bin/env python3
"""SG02: disposable local cluster-profile smoke test with the production ACL.

Build first: cargo build --features cluster
Run: python3 benches/scripts/test_redis_prefix.py --redis-image <digest> --postgres-image <digest>
Requires Docker and cargo. No cluster access; all fixtures are removed on exit.
Only synthetic data is used. Credentials/tokens and server output are never printed.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]


def run(args, **kwargs):
    return subprocess.run(args, check=True, capture_output=True, text=True, **kwargs).stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--redis-image', required=True)
    parser.add_argument('--postgres-image', required=True)
    args = parser.parse_args()
    containers = []
    server = None
    try:
        def container(image, port, extra):
            cid = run(['docker', 'run', '-d', '--rm', '-p', f'127.0.0.1::{port}', *extra, image])
            containers.append(cid)
            mapping = run(['docker', 'port', cid, str(port)]).rsplit(':', 1)[1]
            return cid, mapping

        redis, redis_port = container(args.redis_image, 6379, [])
        postgres, pg_port = container(args.postgres_image, 5432, ['-e', 'POSTGRES_HOST_AUTH_METHOD=trust'])

        def redis_cmd(*command):
            return run(['docker', 'exec', redis, 'redis-cli', '--json', *command])

        for _ in range(100):
            try:
                if redis_cmd('PING') == '"PONG"' and 'accepting connections' in run(['docker', 'exec', postgres, 'pg_isready', '-U', 'postgres']):
                    break
            except subprocess.CalledProcessError:
                pass
            time.sleep(.2)
        else:
            raise AssertionError('fixture startup timeout')
        assert redis_cmd('ACL', 'SETUSER', 'stargate', 'reset', 'on', 'nopass', '~stargate:*', '~audit.raw', '+@all', '-@admin', '-@dangerous') == '"OK"'
        redis_cmd('XGROUP', 'CREATE', 'audit.raw', 'sg02', '0', 'MKSTREAM')
        redis_cmd('ACL', 'LOG', 'RESET')
        redis_cmd('CONFIG', 'RESETSTAT')
        with tempfile.TemporaryDirectory(prefix='sg02-') as directory:
            folder = Path(directory)
            # Run outside the checkout so dotenv never reads the operator's .env.
            env = {key: os.environ[key] for key in ('PATH', 'HOME', 'TMPDIR', 'RUSTUP_HOME', 'CARGO_HOME') if key in os.environ}
            with socket.socket() as sock:
                sock.bind(('127.0.0.1', 0))
                port = sock.getsockname()[1]
            env.update(CONFIG_PATH=directory, POSTGRES_ENDPOINT=f'127.0.0.1:{pg_port}', POSTGRES_USERNAME='postgres', POSTGRES_PASSWORD='', POSTGRES_DATABASE='postgres', POSTGRES_CREATE_DATABASE='false', POSTGRES_SSLMODE='disable', REDIS_URL=f'redis://127.0.0.1:{redis_port}', REDIS_USERNAME='stargate', REDIS_PASSWORD='', REDIS_KEY_PREFIX='stargate:', MFA_MODE='off', JWT_ALGORITHM='HS256', JWT_SECRET=secrets.token_urlsafe(48), JWT_ISSUER=f'http://127.0.0.1:{port}', PORT=str(port), LOG_FORMAT='json', AUDIT_REDIS_STREAM='audit.raw', AUDIT_STREAM_CONSUMER_GROUPS='sg02', OTEL_ENABLED='false', SERVER_DRAIN_DELAY_SECS='0', SG02_REDIS_URL=f'redis://stargate@127.0.0.1:{redis_port}')
            (folder / 'config.yaml').write_text('''schema: stargate/v1
ingress:
  limit: ingress
  on_store_error: deny
limits:
  default:
    strategy: gcra
    params: {max_burst: 1000, replenish_1_per: 1s}
  ingress:
    strategy: gcra
    params: {max_burst: 1000, replenish_1_per: 1ms}
  smoke:
    strategy: gcra
    params: {max_burst: 1, replenish_1_per: 1h}
http:
  services:
    ok: {kind: direct_response, status: 200}
  policies:
    limit: {kind: rate_limit, limit: smoke, on_store_error: deny}
  routers:
    smoke:
      match: {path: {exact: /sg02}}
      service: ok
      policies: [limit]
''')
            binary = str(ROOT / 'target/debug/stargate')
            password = secrets.token_urlsafe(30) + 'Aa1!'
            run([binary, 'admin', 'bootstrap', '--email', 'sg02@example.com', '--password-stdin', '--name', 'SG02', '--nickname', 'sg02'], input=password+'\n', env=env, cwd=directory)
            with (folder / 'server.log').open('w+') as log:
                server = subprocess.Popen([binary], env=env, cwd=directory, stdout=log, stderr=subprocess.STDOUT)
                def request(path, method='GET', body=None, token=None):
                    headers = {'Content-Type': 'application/json'}
                    if token:
                        headers['Authorization'] = f'Bearer {token}'
                    req = urllib.request.Request(f'http://127.0.0.1:{port}{path}', data=json.dumps(body).encode() if body is not None else None, method=method, headers=headers)
                    try:
                        with urllib.request.urlopen(req, timeout=5) as response:
                            raw = response.read()
                            return response.status, json.loads(raw) if raw else None
                    except urllib.error.HTTPError as error:
                        return error.code, None
                for _ in range(150):
                    try:
                        if request('/readyz')[0] == 200:
                            break
                    except (OSError, ValueError):
                        pass
                    if server.poll() is not None:
                        log.flush()
                        diagnostic = (folder / 'server.log').read_text().replace(password, '[redacted]').replace(env['JWT_SECRET'], '[redacted]')
                        raise AssertionError('server exited before readiness: ' + diagnostic)
                    time.sleep(.2)
                else:
                    raise AssertionError('server readiness timeout')
                status, login = request('/account/login', 'POST', {'username': 'sg02', 'password': password})
                assert status == 200, f'login status {status}'
                status, refreshed = request('/account/refresh-token', 'PUT', {'refresh_token': login['refreshToken']})
                assert status == 200, f'refresh status {status}'
                # A second session performs revocation of the first one.
                status, admin = request('/account/login', 'POST', {'username': 'sg02', 'password': password})
                assert status == 200
                claims = json.loads(base64.urlsafe_b64decode(refreshed['accessToken'].split('.')[1] + '=='))
                assert request(f"/admin/sessions/{claims['sid']}", 'DELETE', token=admin['accessToken'])[0] == 200
                assert request('/account/refresh-token', 'PUT', {'refresh_token': refreshed['refreshToken']})[0] == 401
                revoked_status = request('/admin/sessions', token=refreshed['accessToken'])[0]
                assert revoked_status in (401, 403), f'revoked access status {revoked_status}'
                assert request('/sg02')[0] == 200
                assert request('/sg02')[0] == 429
                for _ in range(100):
                    if int(json.loads(redis_cmd('XLEN', 'audit.raw'))) > 0:
                        break
                    time.sleep(.1)
                else:
                    raise AssertionError('relay did not publish audit.raw')
                # Exercise all store operations and every Lua limiter under the same ACL.
                result = subprocess.run(['cargo', 'test', '-p', 'store', '-p', 'lim', '--features', 'redis', '--locked', '--offline', '--test', 'redis_prefix', '--', '--ignored'], cwd=ROOT, env=env, capture_output=True, text=True)
                assert result.returncode == 0, 'Redis prefix integration tests failed: ' + result.stdout + result.stderr
                log.flush()
                assert 'NOPERM' not in (folder / 'server.log').read_text(), 'NOPERM in server log'
                assert json.loads(redis_cmd('ACL', 'LOG')) == [], 'ACL permission failures recorded'
                stats = redis_cmd('INFO', 'commandstats')
                commands = sorted(line.split(':', 1)[0].removeprefix('cmdstat_') for line in stats.splitlines() if line.startswith('cmdstat_'))
                keys = json.loads(redis_cmd('KEYS', '*'))
                assert 'audit.raw' in keys
                assert 'stargate:audit.raw' not in keys
                assert all(key == 'audit.raw' or key.startswith('stargate:') for key in keys)
                print('PASS: login, refresh, revoke, fail-closed gateway limiter, all Lua limiters, relay; zero NOPERM; all state keys prefixed, audit.raw unchanged.')
                print('Observed Redis commands (including fixture diagnostics): ' + ', '.join(commands))
    finally:
        if server and server.poll() is None:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
        for cid in reversed(containers):
            subprocess.run(['docker', 'rm', '-f', cid], capture_output=True)


if __name__ == '__main__':
    main()
