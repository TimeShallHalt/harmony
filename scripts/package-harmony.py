#!/usr/bin/env python3
"""Assemble a local, portable Harmony ZIP and matching tracked source; never upload or install."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]

def validate_stamp(stamp, commit, version):
    expected = {'version':version, 'commit':commit, 'dirty':False, 'platform':'windows',
                'architecture':'x86_64', 'upstream':'Yapfast 0.4.1', 'license':'GPL-3.0-or-later'}
    if (not isinstance(stamp, dict) or set(stamp) != set(expected) | {'build_id'}
            or any(stamp.get(k) != v for k,v in expected.items())
            or stamp.get('dirty') is not False
            or type(stamp.get('build_id')) is not int or not 0 < stamp['build_id'] < 2**64):
        raise ValueError('Package requires a matching clean Windows x86_64 release build stamp')

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def main():
    from release_package import dependency_notices, source_snapshot, zip_directory
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('exe','stamp','metadata','skia-archive','skia-source','webrtc','output'):
        p.add_argument('--'+name, type=Path, required=True)
    args=p.parse_args()
    commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    if subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip():
        p.error('Commit the exact clean candidate before packaging')
    version=tomllib.loads((ROOT/'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    stamp=json.loads(args.stamp.read_bytes())
    validate_stamp(stamp,commit,version)
    with args.exe.open('rb') as f:
        h=f.read(64)
        if len(h)!=64 or h[:2]!=b'MZ': raise ValueError('Expected PE executable')
        offset=struct.unpack_from('<I',h,60)[0]
        if not 64<=offset<=1024*1024: raise ValueError('Invalid PE header offset')
        f.seek(offset)
        if f.read(6)!=b'PE\0\0\x64\x86': raise ValueError('Expected AMD64 PE executable')
    metadata=json.loads(args.metadata.read_bytes())
    app=next(x for x in metadata['packages'] if x['name']=='yapfast')
    if Path(app['manifest_path']).resolve()!=ROOT/'Cargo.toml' or app['version']!=version:
        raise ValueError('Metadata does not belong to this candidate')
    if any(x['name'].startswith('matrix-rtc') for x in metadata['packages']):
        raise ValueError('Use Discord-only metadata for this portable release')
    output=args.output.resolve()
    output.mkdir(parents=True,exist_ok=True)
    if any(output.iterdir()): raise ValueError('Output must be empty; never overwrite a release')
    base=f'Harmony-{version}-b{stamp["build_id"]}'
    archive=output/f'{base}-windows-x64.zip'
    source=output/f'{base}-source.zip'
    with tempfile.TemporaryDirectory(prefix='harmony-package-') as temp:
        temp=Path(temp)
        source_snapshot(metadata,base,temp,source)
        package=temp/f'{base}-windows-x64'
        package.mkdir()
        shutil.copy2(args.exe,package/'Harmony.exe')
        shutil.copy2(ROOT/'LICENSE',package/'LICENSE.txt')
        shutil.copy2(args.stamp,package/'harmony-build-info.json')
        deps=dependency_notices(metadata,package,package/'licenses',args.skia_archive,args.skia_source,args.webrtc)
        (package/'THIRD-PARTY.json').write_text(json.dumps(deps,indent=2)+'\n',encoding='utf-8')
        (package/'SOURCE.txt').write_text(f'Harmony {version}, build {stamp["build_id"]}\nCommit: {commit}\nGPL-3.0-or-later.\nMatching source accompanies this package: {source.name}\nSource SHA-256: {digest(source)}\nUpstream: Yapfast 0.4.1, https://github.com/yapfast/yapfast\nDependency notices and licenses are included in licenses/.\n',encoding='utf-8')
        (package/'README.txt').write_text('Harmony for Windows 10/11 x64\n\nExtract the whole ZIP, then run Harmony.exe.\nFind releases in Settings > About. Windows Setup can be downloaded and verified in Harmony. Portable copies can keep using the ZIP or switch to Setup.\nDo not replace a running app: close it yourself when convenient before a manual update.\nExisting data folders are preserved. The default Windows folder is %LOCALAPPDATA%/Harmony/data; Credential Manager keeps its legacy service IDs for compatibility.\nIf an existing shortcut still targets yapfast.exe, manually retarget it to Harmony.exe while preserving its --data-dir arguments.\nNo credentials, sessions, account cache or user settings are included.\nDiscord does not authorize third-party personal-account clients; use carries account risk.\nGPL-3.0-or-later; see LICENSE.txt, SOURCE.txt and licenses/.\n',encoding='utf-8')
        build=dict(stamp,executable_sha256=digest(args.exe),source_archive_sha256=digest(source),
                   updater_included=True,installation_performed=False,activation_performed=False)
        (package/'BUILD.json').write_text(json.dumps(build,indent=2)+'\n',encoding='utf-8')
        zip_directory(package,archive)
    sums={x.name:digest(x) for x in (archive,source)}
    (output/'SHA256SUMS.txt').write_text(''.join(f'{v}  {k}\n' for k,v in sums.items()),encoding='utf-8')
    manifest = dict(stamp, executable={'name':'Harmony.exe','sha256':digest(args.exe)},
                    artifacts=[{'path':str(path),'sha256':sums[path.name]} for path in (archive,source)],
                    uploaded=False, installed=False)
    (output/'release-artifacts.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({'commit':commit,'artifacts':sums,'output':str(output),'uploaded':False,'installed':False},indent=2))

if __name__=='__main__':
    main()
