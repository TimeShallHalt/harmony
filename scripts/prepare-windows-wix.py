#!/usr/bin/env python3
"""Fetch pinned, build-only WiX v4 tooling; never install SDKs or execute downloads."""
from pathlib import Path, PurePosixPath
import argparse
import base64
import hashlib
import json
import os
import shutil
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
VERSION = '4.0.6'
# SHA-512 values published in NuGet's catalog for these exact package versions.
PACKAGES = {
    'wix': '9wTcXMuWRtWEj2ccCVJn6vkLXnjTJWTJ+d3uIpzFYuYjPsyEbN9sBQLwzOSRszQ5IVK4wdBDoh0MhCcdYJ61yw==',
    'wixtoolset.ui.wixext': 'AE53E07+cNPMpSu3wxWBDrZztEIGx62dVOZNkcSlRSsyTRBl45qpWHqQjtANDMSKvLkiMc9D5Fpu4hm7oM1jNA==',
}


def reject_links(path):
    for part in (path, *path.parents):
        try:
            info = part.lstat()
        except FileNotFoundError:
            continue
        if part.is_symlink() or part.is_junction() or getattr(info, 'st_file_attributes', 0) & 0x400:
            raise ValueError(f'Refusing linked/reparse tooling ancestor: {part}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'.local/tools/wix-4.0.6')
    args = parser.parse_args()
    if not hasattr(Path, 'is_junction'):
        parser.error('Use the supported newer Python interpreter with junction checks')
    output = args.output.absolute()
    reject_links(output)
    if output.exists():
        parser.error('Choose a fresh tooling directory; existing contents are never overwritten')
    scratch = Path(os.environ.get('HARMONY_TEST_SCRATCH') or os.environ['TMPDIR']).absolute()
    reject_links(scratch)
    if not scratch.is_dir():
        parser.error('TMPDIR must be the existing profile scratch directory')
    packages = []
    with tempfile.TemporaryDirectory(prefix='harmony-wix-download-', dir=scratch) as temp:
        stage = Path(temp)/'tools'
        stage.mkdir()
        for name, expected in PACKAGES.items():
            filename = f'{name}.{VERSION}.nupkg'
            url = f'https://api.nuget.org/v3-flatcontainer/{name}/{VERSION}/{filename}'
            archive = stage/filename
            digest = hashlib.sha512()
            size = 0
            with urllib.request.urlopen(url, timeout=90) as response, archive.open('xb') as stream:
                while chunk := response.read(1024*1024):
                    size += len(chunk)
                    if size > 128*1024*1024:
                        raise ValueError('Tool archive exceeds download bound')
                    digest.update(chunk)
                    stream.write(chunk)
            actual = base64.b64encode(digest.digest()).decode('ascii')
            if actual != expected:
                raise ValueError(f'NuGet SHA-512 mismatch for {filename}')
            folder = stage/name
            folder.mkdir()
            with zipfile.ZipFile(archive) as bundle:
                entries = bundle.infolist()
                if len(entries) > 10000 or sum(entry.file_size for entry in entries) > 512*1024*1024:
                    raise ValueError('Tool archive exceeds extraction bounds')
                seen = set()
                for entry in entries:
                    raw_name = entry.orig_filename
                    path = PurePosixPath(raw_name)
                    if (raw_name != entry.filename or path.is_absolute() or any(part in ('.', '..') or ':' in part or part.rstrip(' .') != part for part in path.parts)
                            or '\\' in raw_name or (entry.external_attr >> 16) & 0o170000 == 0o120000
                            or raw_name.casefold() in seen):
                        raise ValueError('Unsafe or duplicate tooling archive member')
                    seen.add(entry.filename.casefold())
                    target = folder/Path(*path.parts)
                    if entry.is_dir():
                        target.mkdir(parents=True, exist_ok=True)
                    else:
                        target.parent.mkdir(parents=True, exist_ok=True)
                        with bundle.open(entry) as source, target.open('xb') as destination:
                            shutil.copyfileobj(source, destination)
            packages.append(dict(id=name,version=VERSION,url=url,sha512=actual,bytes=size,license='MS-RL'))
        wix = list((stage/'wix').rglob('wix.dll'))
        ui = list((stage/'wixtoolset.ui.wixext').rglob('WixToolset.UI.wixext.dll'))
        if len(wix) != 1 or len(ui) != 1:
            raise ValueError('Pinned packages did not expose unique expected CLI/UI assemblies')
        info = dict(version=VERSION,packages=packages,wix_dll=str(output/wix[0].relative_to(stage)),
                    ui_extension=str(output/ui[0].relative_to(stage)),executed=False,
                    antivirus_scan_required_before_execution=True)
        (stage/'tooling-provenance.json').write_text(json.dumps(info,indent=2),encoding='utf-8')
        reject_links(output)
        output.parent.mkdir(parents=True, exist_ok=True)
        # Same-volume profile scratch and checkout; a fresh destination only.
        stage.rename(output)
    print(json.dumps(info,indent=2))


if __name__ == '__main__':
    main()
