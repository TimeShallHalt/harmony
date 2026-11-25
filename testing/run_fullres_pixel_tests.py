"""Run the pixel regression against cached production Windows core artifacts.
Rebuild the integration application after a production change before claiming
GREEN; this helper reports its exact library inputs and never mutates Cargo.
"""
import argparse
import json
import os
import re
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(allow_abbrev=False)
parser.add_argument('--source', type=Path, default=ROOT / 'testing/fullres_pixel_regression.rs')
parser.add_argument('--suite', default='fullres-pixels')
options, test_args = parser.parse_known_args()
if not re.fullmatch(r'[a-z0-9_-]+', options.suite):
    raise SystemExit('Invalid scratch suite name.')
SCRATCH = Path.home() / 'AppData/Local/hermes/cache/scratch' / ('harmony-' + options.suite)
SCRATCH.mkdir(parents=True, exist_ok=True)
vc = 'C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Auxiliary/Build/vcvars64.bat'
setup = subprocess.run('call "' + vc.replace('/', '\\') + '" >nul && set', shell=True,
                       capture_output=True, text=True, encoding='mbcs', errors='replace', check=True)
env = dict(os.environ)
for line in setup.stdout.splitlines():
    if '=' in line and not line.startswith('='):
        k, v = line.split('=', 1)
        env[k] = v
for key in ('TEMP', 'TMP', 'TMPDIR'):
    env[key] = str(SCRATCH)
deps = ROOT / 'target/fast/deps'
exe = SCRATCH / 'pixel-regression.exe'
args = ['C:/Users/ren/.cargo/bin/rustc.exe', '--edition=2024', '--test',
        str(options.source.resolve()), '-C', 'target-feature=+crt-static',
        '-C', 'opt-level=1', '-L', 'dependency=' + str(deps), '-o', str(exe)]
def newest_library(name):
    found = sorted(deps.glob('lib' + name + '-*.rlib'), key=lambda p: p.stat().st_mtime, reverse=True)
    if not found:
        raise SystemExit('Missing built ' + name + '; build the integration app first.')
    return found[0]


def dependency_library(library, dependency):
    # Cargo fingerprints identify the exact dependency instance. Picking the
    # newest serde_json can silently select a different serde_core feature graph.
    crate, fingerprint = library.stem.removeprefix('lib').rsplit('-', 1)
    metadata = list((ROOT / 'target/fast/.fingerprint').glob(
        '*-' + fingerprint + '/lib-' + crate + '.json'))
    if len(metadata) != 1:
        raise SystemExit('Missing unambiguous Cargo metadata for ' + library.name)
    recorded = json.loads(metadata[0].read_text(encoding='utf-8'))
    expected = next(entry[3] for entry in recorded['deps'] if entry[1] == dependency)
    for stamp in (ROOT / 'target/fast/.fingerprint').glob('*/lib-' + dependency):
        value = int.from_bytes(bytes.fromhex(stamp.read_text().strip()), 'little')
        candidate = deps / ('lib' + dependency + '-' + stamp.parent.name.rsplit('-', 1)[1] + '.rlib')
        if value == expected and candidate.is_file():
            return candidate
    raise SystemExit('No cached matching ' + dependency + '; rebuild the integration app.')


core = newest_library('yapfast_core')
discord = dependency_library(core, 'yapfast_discord')
libraries = {'yapfast_core': core, 'image': newest_library('image'),
             'yapfast_discord': discord, 'serde_json': dependency_library(discord, 'serde_json'),
             'tokio': dependency_library(discord, 'tokio')}
for name, library in libraries.items():
    print('Production input:', library.name, flush=True)
    args += ['--extern', name + '=' + str(library)]
native = {p for p in (ROOT / 'target/fast/build').glob('*/out') if p.is_dir()}
native.update(p.parent for p in (ROOT / 'target/fast/build').glob('*/out/**/*.lib'))
registry = Path.home() / '.cargo/registry/src'
native.update(p for p in registry.glob('*/windows_x86_64_msvc-*/lib') if p.is_dir())
for path in sorted(native):
    args += ['-L', 'native=' + str(path)]
flags = subprocess.BELOW_NORMAL_PRIORITY_CLASS
compiled = subprocess.run(args, env=env, cwd=ROOT, creationflags=flags)
if compiled.returncode:
    raise SystemExit(compiled.returncode)
raise SystemExit(subprocess.run([str(exe), '--nocapture', *test_args], env=env,
                               cwd=ROOT, creationflags=flags).returncode)
