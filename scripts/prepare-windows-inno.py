#!/usr/bin/env python3
"""Prepare a pinned, verified, project-local portable Inno compiler; no global install."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import urllib.request
import winreg

ROOT = Path(__file__).resolve().parents[1]
VERSION = '7.1.1'
ASSET = 'innosetup-7.1.1-x64.exe'
URL = 'https://github.com/jrsoftware/issrc/releases/download/is-7_1_1/' + ASSET
# GitHub's published immutable release asset digest, not a self-derived trust pin.
SHA256 = '8a5a6711c3766a6e5fb74f280ba0e3551e719ce94538d901fdbd1f8f9920b3f0'
SIZE = 14336568
PUBLISHER = 'Pyrsys B.V.'


def require(value, message):
    if not value:
        raise ValueError(message)


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def no_reparse(path):
    path = Path(path).absolute()
    for item in (path, *path.parents):
        if item.exists() or item.is_symlink():
            require(not item.is_symlink() and not getattr(item.lstat(), 'st_file_attributes', 0) & 0x400,
                    'Reparse boundary refused: ' + str(item))
    return path


def scan(path, environment):
    platform = Path(os.environ['PROGRAMDATA'])/'Microsoft/Windows Defender/Platform'
    scanners = [p for p in platform.glob('*/MpCmdRun.exe') if p.is_file()]
    if not scanners:
        scanners = [Path(os.environ['PROGRAMFILES'])/'Windows Defender/MpCmdRun.exe']
    scanner = max(scanners, key=lambda p:p.stat().st_mtime)
    before = {p.relative_to(path).as_posix():digest(p) for p in path.rglob('*') if p.is_file()} if path.is_dir() else {path.name:digest(path)}
    command = [str(scanner),'-Scan','-ScanType','3','-File',str(path),'-DisableRemediation']
    result = subprocess.run(command,capture_output=True,text=True,encoding='utf-8',errors='replace',env=environment,
                            timeout=600,creationflags=subprocess.CREATE_NO_WINDOW)
    after = {p.relative_to(path).as_posix():digest(p) for p in path.rglob('*') if p.is_file()} if path.is_dir() else {path.name:digest(path)}
    report = {'command':command,'exit_code':result.returncode,'output':result.stdout+result.stderr,
              'file_count':len(before),'file_sha256':before,'unchanged':before == after}
    require(result.returncode == 0 and before == after, 'Antivirus scan failed or scanned files changed')
    return report


def integration_fingerprint():
    # The official /PORTABLE=1 mode must not register a compiler/uninstaller or
    # file association. Check only these known keys; never inspect license keys.
    targets = [(r'Software\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 7_is1', None),
               (r'Software\Classes\.iss', ''),
               (r'Software\Classes\InnoSetupScriptFile', '')]
    result = {}
    for root, root_name in ((winreg.HKEY_CURRENT_USER,'HKCU'),(winreg.HKEY_LOCAL_MACHINE,'HKLM')):
        for view in (winreg.KEY_WOW64_32KEY,winreg.KEY_WOW64_64KEY):
            for key, value_name in targets:
                identity = root_name+':'+str(view)+':'+key
                try:
                    with winreg.OpenKey(root,key,0,winreg.KEY_READ | view) as handle:
                        if value_name is None:
                            values = [winreg.EnumValue(handle,i) for i in range(winreg.QueryInfoKey(handle)[1])]
                        else:
                            values = winreg.QueryValueEx(handle,value_name)
                    result[identity] = hashlib.sha256(repr(values).encode('utf-8')).hexdigest()
                except FileNotFoundError:
                    result[identity] = None
    return result


def prepare(output, scratch):
    require(os.name == 'nt' and hasattr(Path,'is_junction'), 'Use supported Windows Python with junction-aware paths')
    output, scratch = no_reparse(output), no_reparse(scratch)
    require(not output.exists() and scratch.is_dir(), 'Choose a fresh tool directory and an existing explicit scratch directory')
    output.mkdir(parents=True)
    download = output/ASSET
    request = urllib.request.Request(URL,headers={'User-Agent':'Harmony-build-tool-preparer'})
    with urllib.request.urlopen(request,timeout=90) as response, download.open('xb') as stream:
        total = 0
        while block := response.read(1024*1024):
            total += len(block)
            require(total <= SIZE, 'Tool download exceeds the pinned asset size')
            stream.write(block)
    require(total == SIZE and digest(download) == SHA256, 'Pinned compiler download integrity mismatch')
    environment = dict(os.environ,TMPDIR=str(scratch),TMP=str(scratch),TEMP=str(scratch),HARMONY_TEST_SCRATCH=str(scratch))
    quoted = str(download).replace("'", "''")
    script = ("$s=Get-AuthenticodeSignature -LiteralPath '"+quoted+"'; "
              "$n=if($s.SignerCertificate){$s.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName,$false)}; "
              "[pscustomobject]@{Status=$s.Status.ToString();Publisher=$n;Thumbprint=$s.SignerCertificate.Thumbprint}|ConvertTo-Json -Compress")
    signature_process = subprocess.run(['powershell.exe','-NoLogo','-NoProfile','-NonInteractive','-Command',script],
                                       capture_output=True,text=True,encoding='utf-8',errors='strict',env=environment,
                                       timeout=90,creationflags=subprocess.CREATE_NO_WINDOW)
    require(signature_process.returncode == 0, 'Authenticode verification failed')
    signature = json.loads(signature_process.stdout)
    require(signature['Status'] == 'Valid' and signature['Publisher'] == PUBLISHER, 'Unexpected or invalid compiler publisher')
    before = integration_fingerprint()
    download_scan = scan(download,environment)
    compiler = output/'compiler'
    require(not compiler.exists(), 'Never merge a compiler into existing tools')
    # Official is-7_1_1/isportable.iss disables uninstallation and previous-dir
    # reuse; setup.iss excludes desktop/file-association tasks in portable mode.
    command = [str(download),'/PORTABLE=1','/CURRENTUSER','/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',
               '/NOICONS','/TASKS=','/DIR='+str(compiler),'/LOG='+str(output/'portable-extraction.log')]
    extraction = subprocess.run(command,capture_output=True,text=True,encoding='utf-8',errors='replace',env=environment,
                                timeout=300,creationflags=subprocess.CREATE_NO_WINDOW | subprocess.BELOW_NORMAL_PRIORITY_CLASS)
    require(extraction.returncode == 0, 'Portable compiler extraction failed: '+str(extraction.returncode))
    for path in compiler.rglob('*'):
        no_reparse(path)
    require((compiler/'ISCC.exe').is_file() and (compiler/'ISCmplr.dll').is_file(), 'Incomplete portable compiler')
    after = integration_fingerprint()
    require(after == before, 'Portable preparation changed compiler registration/file associations')
    compiler_scan = scan(compiler,environment)
    version = subprocess.run([str(compiler/'ISCC.exe'),'--version'],capture_output=True,text=True,encoding='utf-8',errors='replace',
                             env=environment,timeout=30,creationflags=subprocess.CREATE_NO_WINDOW)
    require(version.returncode == 0 and VERSION in version.stdout+version.stderr, 'Unexpected compiler CLI version')
    receipt = {'version':VERSION,'asset_url':URL,'asset_sha256':SHA256,'asset_bytes':SIZE,'authenticode':signature,
               'portable_source_contract':['https://raw.githubusercontent.com/jrsoftware/issrc/is-7_1_1/isportable.iss',
                                           'https://raw.githubusercontent.com/jrsoftware/issrc/is-7_1_1/setup.iss'],
               'compiler':str(compiler/'ISCC.exe'),'portable_extraction_command':command,'extraction_exit_code':extraction.returncode,
               'compiler_cli_output':version.stdout+version.stderr,'integration_fingerprint_unchanged':after == before,
               'download_scan':download_scan,'compiler_scan':compiler_scan,'scratch':str(scratch)}
    (output/'tooling-provenance.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ROOT/'.local/tools/inno-7.1.1-x64')
    parser.add_argument('--scratch-root',type=Path,required=True)
    args = parser.parse_args()
    receipt = prepare(args.output,args.scratch_root)
    print(json.dumps({key:receipt[key] for key in ('version','compiler','asset_sha256','authenticode','integration_fingerprint_unchanged')},indent=2))


if __name__ == '__main__':
    main()
