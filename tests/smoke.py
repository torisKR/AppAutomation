"""Offline CLI smoke tests; no real agents, accounts, or GUI mutations."""
import os
from pathlib import Path
import subprocess
import tempfile

binary = Path(__file__).resolve().parents[1] / 'target/release/appforge'
with tempfile.TemporaryDirectory(prefix='appforge-smoke-') as tmp:
    root = Path(tmp)
    tools = root / 'bin'
    tools.mkdir()
    config = root / 'config/appforge'
    config.mkdir(parents=True)
    projects = root / 'projects'
    config.joinpath('config').write_text(f'primary=codex\nsecondary=claude\nenabled=codex,claude\nprojects_dir={projects}\naside_enabled=false\n')
    env = dict(os.environ, HOME=str(root), XDG_CONFIG_HOME=str(root / 'config'), PATH=str(tools))
    python = __import__('sys').executable
    agent = f'''#!{python}
from pathlib import Path
import sys
p = Path.cwd()
with (p / 'calls').open('a') as out: out.write(repr(sys.argv[1:]) + '\\n')
prompt = sys.argv[-1]
for gate, keyword, report in [('quality', 'FUNCTIONAL + PERFORMANCE', '04-quality'), ('qa', 'PRIMARY REVIEWER', '05-qa')]:
    if keyword in prompt:
        (p / 'docs' / (report + '.md')).write_text('Checks executed; evidence recorded')
        if not (p / 'missing-decision').exists():
            (p / '.appforge' / (gate + '-decision')).write_text('BLOCKED' if (p / 'blocked').exists() else 'PASS')
'''
    for name in ['codex', 'claude']:
        path = tools / name
        path.write_text(agent)
        path.chmod(0o755)
    driver = tools / 'cua-driver'
    driver.write_text('#!/bin/sh\nif [ "$1" = status ]; then echo "Cua Driver daemon is not running"; else echo "Accessibility: ❓ unknown"; fi\n')
    driver.chmod(0o755)

    def run(*args, ok=True):
        result = subprocess.run([str(binary), *map(str, args)], env=env, capture_output=True, text=True, timeout=15)
        assert (result.returncode == 0) == ok, (args, result.stdout, result.stderr)
        return result

    assert '0.2.0' in run('version').stdout
    help_text = run('help').stdout
    assert 'repair-all' in help_text and 'approve-publish' in help_text and 'publish [project]' in help_text
    assert 'daemon=no' in run('computer', 'status').stdout
    app = Path(run('create', 'First game').stdout.strip())
    duplicate = Path(run('create', 'First game').stdout.strip())
    assert app != duplicate
    duplicate.joinpath('.appforge/project.conf').unlink()
    run('run', app)  # Includes skipped Publish: must not deadlock.
    assert 'status=done' in (app / '.appforge/stage-release.status').read_text()
    assert '--mcp-config' not in (app / 'calls').read_text()
    (app / 'blocked').touch()
    run('repair', app, ok=False)
    assert not (app / '.appforge/qa-decision').exists()
    assert 'status=failed' in (app / '.appforge/stage-quality.status').read_text()
    (app / 'blocked').unlink()
    (app / 'missing-decision').touch()
    run('repair', app, ok=False)  # A zero CLI exit does not imply PASS.
    second = Path(run('create', 'Second game').stdout.strip())
    run('repair-all', projects, ok=False)
    assert 'status=done' in (second / '.appforge/stage-qa.status').read_text()
    run('computer', 'setup', ok=False)
    run('setup', ok=False)  # EOF/headless setup must never opt into actions.
    driver.write_text('#!/bin/sh\nif [ "$1" = status ]; then echo "Cua Driver daemon is running"; else echo "Accessibility: ✅ granted"; echo "Screen Recording: ✅ granted"; fi\n')
    run('repair', second)
    calls = (second / 'calls').read_text()
    assert '--ignore-user-config' in calls and 'mcp_servers.computer.command=' in calls
    # Claude controller also receives only its per-run CUA MCP.
    with config.joinpath('config').open('a') as out:
        out.write('computer_backend=claude\nauto_mode=false\n')
    run('repair', second)
    calls = (second / 'calls').read_text()
    assert '--strict-mcp-config' in calls and '--setting-sources' in calls and 'acceptEdits' in calls
    with config.joinpath('config').open('a') as out:
        out.write('store_draft_upload=true\n')
    run('approve-publish', second, ok=False)  # External approval is TTY-only.
    run('run', second, ok=False)
    assert 'status=failed' in (second / '.appforge/stage-publish.status').read_text()
    with config.joinpath('config').open('a') as out:
        out.write('store_draft_upload=false\nnotion_enabled=true\nnotion_target_url=https://notion.so.evil.test/page\n')
    run('run', second, ok=False)
    assert 'invalid Notion' in (second / '.appforge/stage-publish.status').read_text()
print('Offline smoke tests passed')
