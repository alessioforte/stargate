"""Release gates must fail when an architecture, attestation or profile is missing."""
import copy
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('verify_image', Path(__file__).parents[1] / 'scripts/verify-release-image.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReleaseImageTests(unittest.TestCase):
    def setUp(self):
        self.index = {'manifests': []}
        self.manifests = {}
        for arch in ('amd64', 'arm64'):
            digest = f'sha256:{arch}'
            attestation = f'sha256:attestation-{arch}'
            self.index['manifests'] += [
                {'digest': digest, 'platform': {'os': 'linux', 'architecture': arch}},
                {'digest': attestation, 'platform': {'os': 'unknown', 'architecture': 'unknown'}, 'annotations': {'vnd.docker.reference.digest': digest}},
            ]
            self.manifests[attestation] = {'layers': [
                {'mediaType': 'application/vnd.in-toto+json', 'annotations': {'in-toto.io/predicate-type': predicate}}
                for predicate in ('https://spdx.dev/Document', 'https://slsa.dev/provenance/v1')
            ]}
            self.manifests[digest] = {'os': 'linux', 'architecture': arch, 'config': {'Env': ['STARGATE_RUNTIME_PROFILE=cluster'], 'Labels': {'org.opencontainers.image.revision': 'test-revision'}}}

    def inspect(self, reference, template=None):
        if reference == 'registry/stargate@sha256:index':
            return copy.deepcopy(self.index)
        return copy.deepcopy(self.manifests[reference.split('@', 1)[1]])

    def verify(self):
        with patch.object(module, 'inspect', side_effect=self.inspect), patch.dict(module.os.environ, {'GITHUB_SHA': 'test-revision'}):
            module.verify('registry/stargate@sha256:index')

    def test_both_architectures_with_attestations(self):
        self.verify()

    def test_missing_arm64(self):
        self.index['manifests'] = self.index['manifests'][:2]
        with self.assertRaisesRegex(ValueError, 'both linux'):
            self.verify()

    def test_missing_sbom(self):
        self.manifests['sha256:attestation-arm64']['layers'].pop(0)
        with self.assertRaisesRegex(ValueError, 'Missing SBOM'):
            self.verify()

    def test_missing_provenance(self):
        self.manifests['sha256:attestation-amd64']['layers'].pop()
        with self.assertRaisesRegex(ValueError, 'Missing provenance'):
            self.verify()

    def test_wrong_profile(self):
        self.manifests['sha256:arm64']['config']['Env'] = ['STARGATE_RUNTIME_PROFILE=edge']
        with self.assertRaisesRegex(ValueError, 'not the cluster profile'):
            self.verify()

    def test_wrong_revision(self):
        self.manifests['sha256:amd64']['config']['Labels'] = {}
        with self.assertRaisesRegex(ValueError, 'revision mismatch'):
            self.verify()

    def test_oci_subject_links_attestation(self):
        for descriptor in self.index['manifests'][1::2]:
            target = descriptor.pop('annotations')['vnd.docker.reference.digest']
            self.manifests[descriptor['digest']]['subject'] = {'digest': target}
        self.verify()

    def test_attestation_for_another_image(self):
        self.index['manifests'][1]['annotations']['vnd.docker.reference.digest'] = 'sha256:other'
        with self.assertRaisesRegex(ValueError, 'Missing SBOM'):
            self.verify()
