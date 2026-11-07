#!/usr/bin/env python3
"""Offline Harmony release assembler. Never uploads, signs, executes or installs.
Run on controlled build output, not an untrusted shared directory. The publisher
must separately verify binary/source/build-stamp correspondence and signing.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import stat
import sys

REPOSITORY = 'TimeShallHalt/harmony'
MAX_ASSET = 1_500_000_000
PLATFORMS = {'windows-x86_64': ('exe','exe',0x8664), 'windows-aarch64': ('exe','exe',0xaa64), 'linux-x86_64': ('appimage','AppImage',62), 'linux-aarch64': ('appimage','AppImage',183)}

def local_path(path):
    path = Path(os.path.abspath(path))
    if str(path).startswith(('\\\\','//')):
        raise ValueError('network/device paths are not release inputs')
    for part in [*reversed(path.parents), path]:
        if not part.exists() and not part.is_symlink():
            continue
        m = part.lstat()
        if stat.S_ISLNK(m.st_mode) or getattr(m,'st_file_attributes',0) & 0x400:
            raise ValueError('symlink/junction release paths refused')
    return path

def validate_header(file, platform, linux_format='appimage'):
    kind, _, machine = PLATFORMS[platform]
    header = file.read(64)
    if len(header) != 64:
        raise ValueError('short executable header')
    if kind == 'exe':
        offset = int.from_bytes(header[60:64],'little')
        if header[:2] != b'MZ' or not 64 <= offset <= 1024*1024:
            raise ValueError('expected raw PE executable, never an installer/archive')
        file.seek(offset); pe = file.read(6)
        if pe[:4] != b'PE\0\0' or int.from_bytes(pe[4:6],'little') != machine:
            raise ValueError('PE platform/architecture mismatch')
    else:
        if header[:4] != b'\x7fELF' or header[4:7] != bytes([2,1,1]) or int.from_bytes(header[18:20],'little') != machine:
            raise ValueError('ELF platform/architecture mismatch')
        if linux_format == 'native-elf':
            if header[8:11] == b'AI\2' or int.from_bytes(header[16:18],'little') not in (2,3):
                raise ValueError('direct native ELF required for stable layout, not mounted AppImage')
        elif header[8:11] != b'AI\2':
            raise ValueError('expected compatible type-2 AppImage')
    file.seek(0)

def assemble(version, build_id, commit, inputs, output, build_infos, linux_format='appimage'):
    if linux_format not in ('appimage','native-elf'):
        raise ValueError('explicit supported Linux format required')
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)',version):
        raise ValueError('canonical stable semantic version required')
    if any(int(n)>2**64-1 for n in version.split('.')):
        raise ValueError('semantic version component exceeds u64')
    if not 0 < build_id <= 2**64-1 or not re.fullmatch('[0-9a-f]{40}',commit):
        raise ValueError('positive build ID and full lowercase source commit required')
    if not inputs or len(inputs)>4:
        raise ValueError('provide one to four --artifact PLATFORM=PATH values')
    parsed=[]; seen=set()
    for item in inputs:
        platform, separator, source=item.partition('=')
        if not separator or platform not in PLATFORMS or platform in seen:
            raise ValueError('invalid/duplicate platform')
        seen.add(platform); parsed.append((platform,local_path(source)))
    stamps={}
    for item in build_infos:
        platform,separator,name=item.partition('=')
        if not separator or platform in stamps or platform not in seen:
            raise ValueError('one matching --build-info PLATFORM=STAMP.json required per artifact')
        source=local_path(name)
        fd=os.open(source,os.O_RDONLY|getattr(os,'O_NOFOLLOW',0)|getattr(os,'O_NONBLOCK',0))
        with os.fdopen(fd,'rb') as f:
            if not stat.S_ISREG(os.fstat(f.fileno()).st_mode): raise ValueError('regular build stamp required')
            raw=f.read(4097)
            if len(raw)>4096: raise ValueError('build stamp exceeds size limit')
        meta=json.loads(raw)
        fields={'version','build_id','commit','dirty','platform','architecture','upstream','license'}
        if not isinstance(meta,dict) or set(meta)!=fields or meta['version']!=version or type(meta['build_id']) is not int or meta['build_id']!=build_id or meta['commit']!=commit or meta['dirty'] is not False or platform!=f"{meta['platform']}-{meta['architecture']}" or meta['upstream']!='Yapfast 0.3.0' or meta['license']!='GPL-3.0-or-later':
            raise ValueError('build identity/platform mismatch or dirty/unverifiable build')
        stamps[platform]=meta
    if set(stamps)!=seen:
        raise ValueError('provide the clean Cargo harmony-build-info.json for every platform')
    output=local_path(output)
    output.mkdir(parents=True,exist_ok=True)
    if any(output.iterdir()):
        raise ValueError('output directory must be empty; releases are never overwritten')
    manifest={'schema':1,'repository':REPOSITORY,'tag':f'harmony-v{version}-b{build_id}','version':version,'build_id':build_id,'commit':commit,'artifacts':[]}
    created=[]
    try:
        for platform,source in parsed:
            kind,extension,_=PLATFORMS[platform]
            if platform.startswith('linux-') and linux_format == 'native-elf': kind,extension='elf','elf'
            name=f'Harmony-{version}-{platform}.{extension}'
            dest=output/name
            # A single opened input handle supplies validation, hash and copied
            # bytes. Controlled inputs are never executed to infer metadata.
            fd=os.open(source,os.O_RDONLY|getattr(os,'O_NOFOLLOW',0)|getattr(os,'O_NONBLOCK',0))
            with os.fdopen(fd,'rb') as file:
                m=os.fstat(file.fileno())
                if not stat.S_ISREG(m.st_mode) or m.st_nlink != 1 or not 64<=m.st_size<=MAX_ASSET:
                    raise ValueError('regular single-link bounded artifact required')
                validate_header(file,platform,linux_format)
                sha=hashlib.sha256(); size=0
                with dest.open('xb') as target:
                    created.append(dest)
                    while block:=file.read(64*1024):
                        size+=len(block)
                        if size>MAX_ASSET: raise ValueError('artifact grew beyond limit')
                        sha.update(block); target.write(block)
                    target.flush(); os.fsync(target.fileno())
                if size != m.st_size: raise ValueError('artifact changed size during assembly')
            if kind in ('appimage','elf'): dest.chmod(0o755)
            manifest['artifacts'].append({'platform':platform,'name':name,'size':size,'sha256':sha.hexdigest(),'format':kind})
        sums=output/'SHA256SUMS.txt'
        with sums.open('x',encoding='utf-8',newline='\n') as f:
            created.append(sums)
            for a in manifest['artifacts']: f.write(f"{a['sha256']}  {a['name']}\n")
        # Retain upstream legal/source attribution as separate release assets.
        license_path=output/'LICENSE.txt'
        with license_path.open('xb') as f:
            created.append(license_path); f.write((Path(__file__).resolve().parents[1]/'LICENSE').read_bytes())
        source_path=output/'SOURCE.txt'
        with source_path.open('x',encoding='utf-8',newline='\n') as f:
            created.append(source_path)
            f.write(f'Harmony {version}, build {build_id}, commit {commit}\nGPL-3.0-or-later\nSource: https://github.com/{REPOSITORY}/tree/{commit}\nUpstream: Yapfast 0.3.0, https://github.com/yapfast/yapfast\nPreserve the complete source and third-party notices when redistributing.\n')
        # Publish metadata last, only after every artifact has been finalized.
        dest=output/'harmony-update-v1.json'
        with dest.open('x',encoding='utf-8',newline='\n') as f:
            created.append(dest); json.dump(manifest,f,indent=2); f.write('\n'); f.flush(); os.fsync(f.fileno())
        return manifest
    except BaseException:
        for path in reversed(created):
            try: path.unlink()
            except OSError: pass  # Never recurse into or erase unrelated output.
        raise

def build_id_token(raw):
    # Validate the original CLI bytes-as-text, never int()'s permissive syntax.
    if not re.fullmatch(r'[1-9][0-9]*', raw, flags=re.ASCII) or len(raw)>20:
        raise argparse.ArgumentTypeError('canonical positive ASCII build ID required')
    value=int(raw)
    if value>2**64-1:
        raise argparse.ArgumentTypeError('build ID exceeds u64')
    return value

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--version',required=True); p.add_argument('--build-id',required=True,type=build_id_token); p.add_argument('--commit',required=True)
    p.add_argument('--artifact',action='append',required=True,help='PLATFORM=PATH (repeat for each tested portable artifact)')
    p.add_argument('--linux-format',choices=['appimage','native-elf'],default='appimage',help='native-elf is ONLY for separately installed stable-launch layouts; not an in-place AppImage migration')
    p.add_argument('--build-info',action='append',required=True,help='PLATFORM=clean Cargo harmony-build-info.json (repeat per artifact)')
    p.add_argument('--output-dir',required=True,type=Path)
    args=p.parse_args()
    try: manifest=assemble(args.version,args.build_id,args.commit,args.artifact,args.output_dir,args.build_info,args.linux_format)
    except (ValueError,OSError) as e: p.error(str(e))
    print(json.dumps({'tag':manifest['tag'],'repository':REPOSITORY,'output_dir':str(args.output_dir),'artifacts':[a['name'] for a in manifest['artifacts']],'uploaded':False,'signed':False},indent=2))
    return 0
if __name__=='__main__': raise SystemExit(main())
