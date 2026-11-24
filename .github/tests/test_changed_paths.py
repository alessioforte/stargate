"""Path filters decide which pull requests run the expensive CI jobs."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('changed_paths', Path(__file__).parents[1] / 'scripts/changed-paths.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

ROOT = Path(__file__).parents[2]


def patterns(name):
    return module.read_patterns(ROOT / '.github/paths' / name)


class GlobTests(unittest.TestCase):
    def assert_matches(self, pattern, path):
        self.assertTrue(module.matches(path, [pattern]), f'{pattern!r} should match {path!r}')

    def assert_does_not_match(self, pattern, path):
        self.assertFalse(module.matches(path, [pattern]), f'{pattern!r} should not match {path!r}')

    def test_exact_file(self):
        self.assert_matches('src/main.rs', 'src/main.rs')
        self.assert_does_not_match('src/main.rs', 'src/main.rs.bak')

    def test_single_star_stays_in_one_segment(self):
        self.assert_matches('src/*.rs', 'src/main.rs')
        self.assert_does_not_match('src/*.rs', 'src/api/main.rs')

    def test_double_star_matches_every_depth(self):
        self.assert_matches('crates/oidc/**', 'crates/oidc/src/lib.rs')
        self.assert_does_not_match('crates/oidc/**', 'crates/jwt/src/lib.rs')

    def test_double_star_slash_matches_no_directory(self):
        self.assert_matches('**/config.yaml', 'config.yaml')
        self.assert_matches('**/config.yaml', '.github/oidc-conformance/config.yaml')

    def test_question_mark_and_character_class(self):
        self.assert_matches('src/ap?/mod.rs', 'src/api/mod.rs')
        self.assert_does_not_match('src/ap?/mod.rs', 'src/apii/mod.rs')
        self.assert_matches('src/api/[mo]od.rs', 'src/api/mod.rs')
        self.assert_matches('src/api/[!x]od.rs', 'src/api/mod.rs')

    def test_comments_and_blank_lines_are_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'paths.txt'
            path.write_text('# a comment\n\nsrc/main.rs\n')
            self.assertEqual(module.read_patterns(path), ['src/main.rs'])


class WorkflowPathTests(unittest.TestCase):
    def test_oidc_paths(self):
        filtered = patterns('oidc-conformance.txt')
        self.assertTrue(module.matches('src/api/oidc/consent.rs', filtered))
        self.assertTrue(module.matches('.github/oidc-conformance/config-op.json', filtered))
        self.assertFalse(module.matches('src/iam/email_throttle.rs', filtered))
        self.assertFalse(module.matches('README.md', filtered))

    def test_redis_paths(self):
        filtered = patterns('redis-failures.txt')
        self.assertTrue(module.matches('crates/store/src/keys.rs', filtered))
        self.assertTrue(module.matches('src/api/admin/health.rs', filtered))
        self.assertFalse(module.matches('apps/auth/package.json', filtered))
        self.assertFalse(module.matches('README.md', filtered))

    def test_dependency_audit_paths(self):
        filtered = patterns('dependency-audit.txt')
        for path in (
            'Cargo.toml', 'Cargo.lock', 'crates/store/Cargo.toml',
            'tools/example/Cargo.lock', '.cargo/audit.toml',
            'package.json', 'package-lock.json',
            'apps/auth/package.json', 'apps/auth/package-lock.json',
            'apps/console/package.json', 'apps/console/package-lock.json',
            'apps/mail/package.json', 'apps/mail/package-lock.json',
            '.github/npm-audit-exceptions.toml',
        ):
            with self.subTest(path=path):
                self.assertTrue(module.matches(path, filtered))
        for path in (
            'src/main.rs', 'crates/store/src/lib.rs',
            'apps/auth/app/routes/login.tsx', 'README.md', 'AGENTS.md',
            '.github/workflows/ci.yml', '.github/scripts/check-npm-audit.py',
            'apps/mail/package.json.bak',
        ):
            with self.subTest(path=path):
                self.assertFalse(module.matches(path, filtered))


if __name__ == '__main__':
    unittest.main()
