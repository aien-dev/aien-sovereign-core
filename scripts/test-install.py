#!/usr/bin/env python3
"""Exercise piped installation from an arbitrary cwd without real builds."""
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    mocks = root / "mocks"
    mocks.mkdir()
    git = mocks / "git"
    git.write_text('''#!/usr/bin/env python3
import sys
from pathlib import Path
args=sys.argv[1:]
if args[0]=='clone':
 p=Path(args[-1]);p.mkdir(parents=True);(p/'Cargo.toml').write_text('[workspace]')
elif 'rev-parse' in args:
 print('7ac6facb630ca7e9a6ab4125b292203fe2bb6687')
elif 'config' in args:
 print('Fixture Operator' if args[-1]=='user.name' else 'fixture@example.test')
''')
    cargo = mocks / "cargo"
    cargo.write_text('''#!/usr/bin/env python3
import sys
from pathlib import Path
assert Path('Cargo.toml').is_file(), 'installer did not locate checkout'
assert '--locked' in sys.argv
assert 'spark-aegis' not in sys.argv
for name in ['aien-cli','spark-cockpit-rs','cortex-rs','spark-supervisor','spark-debugger','spark-harness','spark-crumbs']:
 p=Path('target/release')/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_text('#!/bin/sh\\nexit 0\\n');p.chmod(0o755)
''')
    for script in [git, cargo]: script.chmod(0o755)
    env = dict(os.environ, PATH=str(mocks) + os.pathsep + os.environ['PATH'],
               AIEN_BIN_DIR=str(root/'bin'), AIEN_CONFIG_DIR=str(root/'config'),
               AIEN_INSTALL_NO_PROFILE='1')
    env.pop('AIEN_SOURCE_DIR', None)
    result = subprocess.run(['bash'], input=(ROOT/'install.sh').read_text(), cwd=root,
                            env=env, text=True, capture_output=True)
    assert result.returncode == 0, result.stderr
    assert (root/'bin/aien').is_file()
    assert (root/'config/operator.toml').is_file()
    print('Piped installer bootstraps a checkout and installs the CLI from an arbitrary cwd')
