"""Read-only exact cached Cargo fingerprint resolution for focused updater tests."""
from pathlib import Path
import json

class Cache:
    def __init__(self, root):
        self.root=Path(root); self.entries=[]
        for stamp in (self.root/'.fingerprint').glob('*/lib-*'):
            if stamp.suffix or not stamp.with_suffix('.json').is_file(): continue
            try:
                digest=int.from_bytes(bytes.fromhex(stamp.read_text().strip()),'little')
                meta=json.loads(stamp.with_suffix('.json').read_text())
            except (ValueError,OSError): continue
            name=stamp.name.removeprefix('lib-')
            lib=self.root/'deps'/f'lib{name}-{stamp.parent.name.rsplit("-",1)[1]}.rlib'
            if not lib.exists(): continue
            self.entries.append((name,digest,lib,meta))
        roots=sorted((self.root/'.fingerprint').glob('yapfast-*/test-bin-yapfast.json'),key=lambda p:p.stat().st_mtime,reverse=True)
        self.graph={}
        def visit(meta,graph):
            for _,name,_,digest in meta['deps']:
                matches=[e for e in self.entries if e[:2]==(name,digest)]
                if not matches: continue  # build scripts/proc-macro DLLs aren't rlibs
                if len(matches)!=1: raise RuntimeError(f'ambiguous exact cached {name}')
                key=(name,digest)
                if key not in graph:
                    graph[key]=matches[0]; visit(matches[0][3],graph)
        if not roots: raise RuntimeError('no owner test-bin-yapfast fingerprint')
        visit(json.loads(roots[0].read_text()),self.graph)
        self.host_graph={}
        # slint-build is a build-script dependency, not a runtime root dep.
        for build in (self.root/'.fingerprint').glob('yapfast-*/build-script-*.json'):
            visit(json.loads(build.read_text()),self.host_graph)
    def lib(self,name,version_prefix=None):
        matches=[e for e in self.graph.values() if e[0]==name]
        if not matches: matches=[e for e in self.host_graph.values() if e[0]==name]
        if version_prefix:
            def version(e):
                dep=e[2].parent/(e[2].stem.removeprefix('lib')+'.d')
                return dep.exists() and f'/{name.replace("_","-")}-{version_prefix}' in dep.read_text().replace('\\','/')
            matches=[e for e in matches if version(e)]
            if not matches: matches=[e for e in self.entries if e[0]==name and version(e)]
        if len(matches)!=1: raise RuntimeError(f'expected one exact cached {name} {version_prefix}: {[e[2] for e in matches]}')
        entry=matches[0]
        # The on-disk dependency graph, not file mtime, selects every rlib.
        def verify(e,seen):
            key=e[:2]
            if key in seen:return
            seen.add(key)
            for _,child,_,digest in e[3]['deps']:
                children=[v for v in self.entries if v[:2]==(child,digest)]
                if children: verify(children[0],seen)
        verify(entry,set())
        return entry[2]
