#!/usr/bin/env python3
"""Redis Cluster test: run the store and limiter suites on a 3+3 cluster.

Build first: cargo build --features cluster
Run: python3 benches/scripts/test_redis_cluster.py --redis-image <image>
Requires Docker. Six disposable nodes stay on localhost and are removed on
exit. Only synthetic data is used.

The plain Redis client cannot follow MOVED redirects, so the tests connect to
the master that owns one hash-tag slot and use only keys under that tag. The
node then accepts every operation of the entity, which is what proves the
schema keeps co-accessed keys in one slot and the Lua scripts declare them.
"""
import argparse
import json
import os
import secrets
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PREFIX = "tenant:"
SCHEMA = "stargate:v1"


def run(args, **kwargs):
    return subprocess.run(args, check=True, capture_output=True, text=True, **kwargs).stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--redis-image', required=True)
    args = parser.parse_args()
    containers = []
    try:
        def redis_cmd(cid, *command):
            return run(['docker', 'exec', cid, 'redis-cli', '--raw', *command])

        for index in range(6):
            name = f'stargate-redis-cluster-{secrets.token_hex(4)}-{index}'
            cid = run([
                'docker', 'run', '-d', '--rm', '--name', name,
                '-p', '127.0.0.1::6379',
                args.redis_image,
                'redis-server', '--cluster-enabled', 'yes',
                '--cluster-config-file', 'nodes.conf',
                '--appendonly', 'no',
            ])
            containers.append(cid)

        def wait_for(condition, message, attempts=200):
            for _ in range(attempts):
                try:
                    if condition():
                        return
                except subprocess.CalledProcessError:
                    pass
                time.sleep(.1)
            raise AssertionError(message)

        def pinging(cid):
            return redis_cmd(cid, 'PING') == 'PONG'

        for cid in containers:
            wait_for(lambda cid=cid: pinging(cid), 'cluster node startup timeout')

        def address(cid):
            ip = run([
                'docker', 'inspect', '--format',
                '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}', cid,
            ])
            return f'{ip}:6379'

        addresses = [address(cid) for cid in containers]
        run(['docker', 'exec', containers[0], 'redis-cli', '--cluster', 'create',
             *addresses, '--cluster-replicas', '1', '--cluster-yes'])

        def cluster_ok():
            return all(
                'cluster_state:ok' in run(['docker', 'exec', cid, 'redis-cli', 'CLUSTER', 'INFO'])
                for cid in containers
            )

        wait_for(cluster_ok, 'cluster did not reach ok state')
        wait_for(
            lambda: all(
                'cluster_known_nodes:6' in run(['docker', 'exec', cid, 'redis-cli', 'CLUSTER', 'INFO'])
                for cid in containers
            ),
            'cluster did not converge on six nodes',
        )

        # Find two tags whose slots the first master owns. The tests use only
        # these tags, so the plain client never needs a MOVED redirect. They
        # must use different slots: a raw cross-slot command is the negative
        # control for the store's per-slot batching.
        host_port = run(['docker', 'port', containers[0], '6379']).rsplit(':', 1)[1]
        tags = []
        slots = set()
        for _ in range(200):
            candidate = f'test:{secrets.token_hex(8)}'
            key = f'{PREFIX}{SCHEMA}:{{{candidate}}}:probe'
            if redis_cmd(containers[0], 'SET', key, '1') != 'OK':
                continue
            slot = int(redis_cmd(containers[0], 'CLUSTER', 'KEYSLOT', key))
            redis_cmd(containers[0], 'DEL', key)
            if slot not in slots:
                tags.append(candidate)
                slots.add(slot)
            if len(tags) == 2:
                break
        assert len(tags) == 2, 'no two local slots found on the first master'
        tag, alt_tag = tags

        for cid in containers:
            assert redis_cmd(
                cid, 'ACL', 'SETUSER', 'stargate', 'reset', 'on', 'nopass',
                f'~{PREFIX}{SCHEMA}:*', '+@all', '-@admin', '-@dangerous',
            ) == 'OK'
            redis_cmd(cid, 'ACL', 'LOG', 'RESET')

        env = {key: os.environ[key] for key in ('PATH', 'HOME', 'TMPDIR', 'RUSTUP_HOME', 'CARGO_HOME') if key in os.environ}
        env.update(
            REDIS_CLUSTER_URL=f'redis://stargate@127.0.0.1:{host_port}',
            REDIS_CLUSTER_TAG=tag,
            REDIS_CLUSTER_TAG_ALT=alt_tag,
        )
        result = subprocess.run(
            ['cargo', 'test', '-p', 'store', '-p', 'lim', '--features', 'redis',
             '--locked', '--offline', '--test', 'redis_cluster', '--', '--ignored'],
            cwd=ROOT, env=env, capture_output=True, text=True,
        )
        assert result.returncode == 0, 'Redis Cluster tests failed: ' + result.stdout + result.stderr
        for cid in containers:
            assert json.loads(run(['docker', 'exec', cid, 'redis-cli', '--json', 'ACL', 'LOG'])) == [], \
                'ACL permission failures recorded'
        print('PASS: store and limiter suites on a 3+3 Redis Cluster; all keys in one entity slot; '
              'zero CROSSSLOT, non local key or NOPERM.')
    finally:
        for cid in reversed(containers):
            subprocess.run(['docker', 'rm', '-f', cid], capture_output=True)


if __name__ == '__main__':
    main()
