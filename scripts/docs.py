#!/usr/bin/env python3
"""Check repository Markdown and render the explicit current-documentation wiki."""

import argparse
import html
import json
import re
import subprocess
from pathlib import Path
from urllib.parse import quote, unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
GENERATED = ".labello-wiki-pages.json"
# Repository prose uses inline links, reference definitions, and HTML image tags.
LINK = re.compile(r'!?\[[^\]\n]*\]\((?P<url><[^>]+>|[^\s)]+)(?:\s+"[^"\n]*")?\)')
REFERENCE = re.compile(r'^\s{0,3}\[[^\]\n]+\]:\s*(?P<url><[^>]+>|\S+)')
HTML_URL = re.compile(r'\b(?:src|href)=["\'](?P<url>[^"\']+)["\']')
METADATA = re.compile(
    r'^(?:>\s*)?(?:\*\*)?(?:Status|Owner|Audience|Last verified|Last classified|'
    r'Last updated|Prepared(?: against)?|Research date|Completed|Baseline commit|'
    r'Commit|Revision)(?:\*\*)?:', re.I
)


def prose_lines(text):
    """Yield only prose; preserve code blocks and inline code byte for byte."""
    fence = None
    for line in text.splitlines(keepends=True):
        marker = re.match(r'^\s*(`{3,}|~{3,})', line)
        if marker:
            value = marker[1]
            if fence is None:
                fence = value
            elif value[0] == fence[0] and len(value) >= len(fence):
                fence = None
            yield line, False
        else:
            yield line, fence is None


def link_spans(line):
    code = [m.span() for m in re.finditer(r'(`+).*?\1', line)]
    spans = []
    for pattern in (LINK, REFERENCE, HTML_URL):
        for match in pattern.finditer(line):
            a, b = match.span("url")
            if not any(start <= a < end for start, end in code):
                if line[a:b].startswith("<"):
                    a, b = a + 1, b - 1
                spans.append((a, b))
    return sorted(set(spans))


def anchors(text):
    result = set()
    for line, prose in prose_lines(text):
        if not prose:
            continue
        result.update(re.findall(r'\b(?:id|name)=["\']([^"\']+)["\']', line))
        heading = re.match(r'^#{1,6}\s+(.+?)(?:\s+#+)?\s*$', line)
        if not heading:
            continue
        label = re.sub(r'<[^>]*>', '', heading[1])
        label = re.sub(r'!?\[([^\]]+)\]\([^)]*\)', r'\1', label)
        slug = re.sub(r'[^\w\- ]', '', html.unescape(label).lower()).replace(' ', '-')
        candidate = slug
        count = 0
        while candidate in result:
            count += 1
            candidate = f'{slug}-{count}'
        result.add(candidate)
    return result


def resolve_link(root, source, url):
    parsed = urlsplit(html.unescape(url))
    if parsed.scheme or parsed.netloc:
        return None
    path = unquote(parsed.path)
    target = (source.parent / path).resolve() if path else source.resolve()
    if not target.is_relative_to(root.resolve()):
        raise ValueError(f'link escapes repository: {url}')
    return target, unquote(parsed.fragment)


def load_manifest(root=ROOT):
    config = json.loads((root / 'docs/wiki.json').read_text())
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', config['repository']):
        raise ValueError('invalid wiki repository')
    if not config['ref'] or config['ref'].startswith('-'):
        raise ValueError('invalid source ref')
    mapping = {}
    names = set()
    allowed = {Path(p) for p in (
        'README.md', 'CONTRIBUTING.md', 'apps/egui-mcp-inspector/README.md',
        'deployment/guest/README.md',
    )} | {p.relative_to(root) for p in (root / 'docs').glob('*.md')}
    for group in config['groups']:
        for entry in group['pages']:
            source = Path(entry['source'])
            name = entry['page']
            if source.is_absolute() or '..' in source.parts or source.suffix != '.md':
                raise ValueError(f'invalid wiki source: {source}')
            if source.is_relative_to('docs/archive'):
                raise ValueError(f'archive cannot be published: {source}')
            if source not in allowed:
                raise ValueError(f'not a current documentation source: {source}')
            if not (root / source).is_file() or (root / source).is_symlink():
                raise ValueError(f'missing or symlink wiki source: {source}')
            if not re.fullmatch(r'[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*', name):
                raise ValueError(f'invalid wiki page name: {name}')
            if source in mapping or name.lower() in names:
                raise ValueError(f'duplicate wiki source/page: {source}, {name}')
            mapping[source] = name
            names.add(name.lower())
    if mapping.get(Path('docs/README.md')) != 'Home':
        raise ValueError('docs/README.md must publish as Home')
    current = {p.relative_to(root) for p in (root / 'docs').glob('*.md')}
    if missing := current - mapping.keys():
        raise ValueError(f'current docs missing from wiki: {sorted(map(str, missing))}')
    return config, mapping


def markdown_files(root):
    result = subprocess.run(
        ['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', '*.md'],
        cwd=root, check=True, capture_output=True, text=True,
    )
    return sorted({root / p for p in result.stdout.split('\0') if p and (root / p).is_file()})


def check(root=ROOT):
    config, mapping = load_manifest(root)
    errors = []
    files = markdown_files(root)
    anchor_cache = {}
    for source in files:
        for number, (line, prose) in enumerate(prose_lines(source.read_text()), 1):
            if not prose:
                continue
            location = f'{source.relative_to(root)}:{number}'
            if METADATA.match(line):
                errors.append(f'{location}: document metadata header')
            for start, end in link_spans(line):
                url = line[start:end]
                try:
                    resolved = resolve_link(root, source, url)
                    if resolved is None:
                        continue
                    target, fragment = resolved
                    if not target.exists():
                        raise ValueError(f'missing link target: {url}')
                    if fragment and target.suffix == '.md':
                        if target not in anchor_cache:
                            anchor_cache[target] = anchors(target.read_text())
                        if fragment not in anchor_cache[target]:
                            raise ValueError(f'missing anchor: {url}')
                except ValueError as error:
                    errors.append(f'{location}: {error}')
    if errors:
        raise ValueError('\n'.join(errors))
    print(f'Checked {len(files)} Markdown files and {len(mapping)} wiki pages.')
    return config, mapping


def rewrite(root, source, text, config, mapping):
    repo = config['repository']
    ref = quote(config['ref'], safe='')
    wiki = f'https://github.com/{repo}/wiki/'

    def url_for(url):
        resolved = resolve_link(root, source, url)
        if resolved is None:
            return url
        target, fragment = resolved
        relative = target.relative_to(root)
        suffix = '#' + quote(fragment, safe='-_') if fragment else ''
        if target == source and not urlsplit(url).path:
            return suffix
        if relative in mapping:
            return wiki + mapping[relative] + suffix
        path = quote(relative.as_posix(), safe='/')
        if target.suffix.lower() in {'.svg', '.png', '.jpg', '.jpeg', '.webp', '.gif'}:
            return f'https://raw.githubusercontent.com/{repo}/{ref}/{path}' + suffix
        kind = 'tree' if target.is_dir() else 'blob'
        return f'https://github.com/{repo}/{kind}/{ref}/{path}' + suffix

    output = []
    for line, prose in prose_lines(text):
        if prose:
            for start, end in reversed(link_spans(line)):
                line = line[:start] + url_for(line[start:end]) + line[end:]
        output.append(line)
    return ''.join(output)


def build(destination, root=ROOT):
    config, mapping = check(root)
    if destination.is_symlink():
        raise ValueError('wiki output must not be a symlink')
    destination = destination.resolve()
    # Never render into source documents or an arbitrary non-directory file.
    if destination == root or destination.is_relative_to(root):
        raise ValueError('wiki output must be outside the source repository')
    destination.mkdir(parents=True, exist_ok=True)
    pages = {name + '.md': rewrite(root, root / source, (root / source).read_text(), config, mapping)
             for source, name in mapping.items()}
    sidebar = []
    for group in config['groups']:
        sidebar.append('### ' + group['title'] + '\n')
        for entry in group['pages']:
            title = (root / entry['source']).read_text().splitlines()[0].removeprefix('# ')
            sidebar.append(f"- [{title}](https://github.com/{config['repository']}/wiki/{entry['page']})")
        sidebar.append('')
    pages['_Sidebar.md'] = '\n'.join(sidebar) + '\n'
    pages['_Footer.md'] = (
        'Edit documentation in the '
        f"[Labello repository](https://github.com/{config['repository']}/tree/{quote(config['ref'], safe='')}/docs). "
        'The wiki is generated from the current documentation; archived plans stay in the repository.\n'
    )
    record = destination / GENERATED
    previous = json.loads(record.read_text()) if record.exists() else []
    if not isinstance(previous, list) or any(not isinstance(p, str) or not re.fullmatch(r'[A-Za-z0-9_-]+\.md', p) for p in previous):
        raise ValueError('invalid generated-page inventory')
    for name in set(previous) | pages.keys() | {GENERATED}:
        path = destination / name
        if path.is_symlink() or (path.exists() and not path.is_file()):
            raise ValueError(f'unsafe wiki output: {name}')
    # Remove only pages recorded by a previous build, preserving unrelated wiki files.
    for name in set(previous) - pages.keys():
        (destination / name).unlink(missing_ok=True)
    for name, content in pages.items():
        (destination / name).write_text(content)
    record.write_text(json.dumps(sorted(pages), indent=2) + '\n')
    print(f'Rendered {len(mapping)} pages plus sidebar/footer to {destination}.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('check')
    render = sub.add_parser('build')
    render.add_argument('destination', type=Path)
    args = parser.parse_args()
    try:
        if args.command == 'check':
            check()
        else:
            build(args.destination)
    except (ValueError, OSError) as error:
        parser.exit(1, f'{error}\n')


if __name__ == '__main__':
    main()
