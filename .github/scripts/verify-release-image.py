#!/usr/bin/env python3
"""Validate a published image index, cluster profile and per-platform attestations."""
import json
import os
import subprocess
import sys


def inspect(reference, template=None):
    command = ['docker', 'buildx', 'imagetools', 'inspect', reference]
    command += ['--format', template] if template else ['--raw']
    return json.loads(subprocess.check_output(command, text=True))


def verify(reference):
    repository = reference.split('@', 1)[0]
    index = inspect(reference)
    images = {}
    attestations = {}
    for descriptor in index['manifests']:
        platform = descriptor.get('platform', {})
        if platform.get('os') == 'linux' and platform.get('architecture') in ('amd64', 'arm64'):
            name = f"linux/{platform['architecture']}"
            if name in images:
                raise ValueError(f'Duplicate platform: {name}')
            images[name] = descriptor['digest']
        else:
            manifest = inspect(f"{repository}@{descriptor['digest']}")
            subject = manifest.get('subject', {}).get('digest')
            target = descriptor.get('annotations', {}).get('vnd.docker.reference.digest', subject)
            predicates = {
                layer.get('annotations', {}).get('in-toto.io/predicate-type', '')
                for layer in manifest.get('layers', [])
                if layer.get('mediaType') == 'application/vnd.in-toto+json'
            }
            attestations.setdefault(target, set()).update(predicates)
    if set(images) != {'linux/amd64', 'linux/arm64'}:
        raise ValueError('Index must contain both linux/amd64 and linux/arm64')
    for platform, digest in images.items():
        predicates = attestations.get(digest, set())
        if 'https://spdx.dev/Document' not in predicates:
            raise ValueError(f'Missing SBOM for {platform}')
        if not any(value.startswith('https://slsa.dev/provenance/') for value in predicates):
            raise ValueError(f'Missing provenance for {platform}')
        image = inspect(f'{repository}@{digest}', '{{json .Image}}')
        if f"{image['os']}/{image['architecture']}" != platform:
            raise ValueError(f'Image config platform mismatch: {platform}')
        config = image['config']
        if 'STARGATE_RUNTIME_PROFILE=cluster' not in config.get('Env', []):
            raise ValueError(f'Image is not the cluster profile: {platform}')
        revision = os.environ.get('GITHUB_SHA')
        if revision and config.get('Labels', {}).get('org.opencontainers.image.revision') != revision:
            raise ValueError(f'Image revision mismatch: {platform}')
    print('Verified amd64/arm64 cluster images, provenance and SBOM:', reference)


if __name__ == '__main__':
    verify(sys.argv[1])
