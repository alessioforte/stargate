#!/usr/bin/env python3
"""Whether a pull request changed paths matching a workflow's filters.

Usage: changed-paths.py --base <sha> [--head <sha>] <patterns file>

Prints `true` when at least one file changed between the two revisions matches
one of the globs in the patterns file, `false` otherwise. Blank lines and lines
starting with `#` are ignored. Globs follow the GitHub Actions path-filter
syntax: `*` and `?` stay within one path segment, `**` spans any number of
them. The patterns file exists so the workflow runs on every pull request, and
a required check always reports a result, while the expensive jobs stay
conditional.
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path


def compile_glob(pattern):
    """Compile one GitHub Actions path glob into a full-match regex."""
    regex = ['^']
    index = 0
    while index < len(pattern):
        char = pattern[index]
        if char == '*':
            if pattern.startswith('**', index):
                index += 2
                if pattern.startswith('/', index):
                    # `**/` also matches no directory at all.
                    regex.append('(?:.*/)?')
                    index += 1
                else:
                    regex.append('.*')
                continue
            regex.append('[^/]*')
        elif char == '?':
            regex.append('[^/]')
        elif char == '[':
            end = pattern.find(']', index + 1)
            if end == -1:
                regex.append(re.escape(char))
            else:
                body = pattern[index + 1:end]
                if body.startswith('!'):
                    body = '^' + body[1:]
                regex.append(f'[{body}]')
                index = end
        else:
            regex.append(re.escape(char))
        index += 1
    regex.append('$')
    return re.compile(''.join(regex))


def read_patterns(path):
    lines = (line.strip() for line in path.read_text().splitlines())
    return [line for line in lines if line and not line.startswith('#')]


def matches(path, patterns):
    """Whether one repository-relative path matches any pattern."""
    return any(compile_glob(pattern).match(path) for pattern in patterns)


def changed_files(base, head):
    """Files that differ between two revisions, with both sides of a rename."""
    result = subprocess.run(
        ['git', '-c', 'core.quotePath=false', 'diff', '--name-only', '--no-renames', base, head],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit(f'git diff {base} {head} failed: {result.stderr.strip()}')
    return [line for line in result.stdout.splitlines() if line]


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--base', required=True, help='base revision of the pull request')
    parser.add_argument('--head', default='HEAD', help='head revision (default: HEAD)')
    parser.add_argument('patterns', type=Path, help='file with one path glob per line')
    args = parser.parse_args(argv[1:])

    patterns = read_patterns(args.patterns)
    changed = changed_files(args.base, args.head)
    print('true' if any(matches(path, patterns) for path in changed) else 'false')


if __name__ == '__main__':
    main(sys.argv)
