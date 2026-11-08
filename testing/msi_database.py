"""Read-only native MSI inspection. Never use Win32_Product (it can trigger repairs)."""
import ctypes as c
from ctypes import wintypes as w
from pathlib import Path
import json
import sys

class Database:
    def __init__(self,path):
        self.api=c.WinDLL('msi',use_last_error=True)
        self.api.MsiOpenDatabaseW.argtypes=[w.LPCWSTR,w.LPCWSTR,c.POINTER(w.UINT)];self.api.MsiOpenDatabaseW.restype=w.UINT
        self.api.MsiDatabaseOpenViewW.argtypes=[w.UINT,w.LPCWSTR,c.POINTER(w.UINT)];self.api.MsiDatabaseOpenViewW.restype=w.UINT
        self.api.MsiViewExecute.argtypes=[w.UINT,w.UINT];self.api.MsiViewExecute.restype=w.UINT
        self.api.MsiViewFetch.argtypes=[w.UINT,c.POINTER(w.UINT)];self.api.MsiViewFetch.restype=w.UINT
        self.api.MsiRecordGetFieldCount.argtypes=[w.UINT];self.api.MsiRecordGetFieldCount.restype=w.UINT
        self.api.MsiRecordGetStringW.argtypes=[w.UINT,w.UINT,w.LPWSTR,c.POINTER(w.DWORD)];self.api.MsiRecordGetStringW.restype=w.UINT
        self.api.MsiCloseHandle.argtypes=[w.UINT]
        self.handle=w.UINT()
        self.check(self.api.MsiOpenDatabaseW(str(Path(path).resolve()),None,c.byref(self.handle)))
    @staticmethod
    def check(code):
        if code:raise OSError(code,'MSI database API failed')
    def query(self,sql):
        if not sql.startswith('SELECT '):raise ValueError('Read-only queries only')
        view=w.UINT();self.check(self.api.MsiDatabaseOpenViewW(self.handle,sql,c.byref(view)))
        rows=[]
        try:
            self.check(self.api.MsiViewExecute(view,0))
            while True:
                record=w.UINT();code=self.api.MsiViewFetch(view,c.byref(record))
                if code==259:break
                self.check(code)
                try:
                    fields=[]
                    for index in range(1,self.api.MsiRecordGetFieldCount(record)+1):
                        length=w.DWORD();code=self.api.MsiRecordGetStringW(record,index,None,c.byref(length))
                        if code not in (0,234):self.check(code)
                        buffer=c.create_unicode_buffer(length.value+1);length.value=len(buffer)
                        self.check(self.api.MsiRecordGetStringW(record,index,buffer,c.byref(length)))
                        fields.append(buffer.value)
                    rows.append(fields)
                finally:self.api.MsiCloseHandle(record)
        finally:self.api.MsiCloseHandle(view)
        return rows
    def close(self):self.api.MsiCloseHandle(self.handle)
    def __enter__(self):return self
    def __exit__(self,*_):self.close()

def inspect(path):
    with Database(path) as db:
        tables={row[0] for row in db.query('SELECT `Name` FROM `_Tables`')}
        rows={name:db.query('SELECT * FROM `'+name+'`') for name in ('Property','Directory','Component','File','Registry','Upgrade','CustomAction','InstallExecuteSequence','RemoveFile') if name in tables}
        properties=dict(rows['Property'])
        if properties.get('ALLUSERS') not in (None,''):raise ValueError('Not a per-user-only package')
        if properties.get('REBOOT')!='ReallySuppress' or properties.get('MSIRESTARTMANAGERCONTROL')!='DisableShutdown':raise ValueError('Automatic restart/shutdown policy changed')
        if 'Shortcut' in tables or 'ServiceInstall' in tables or 'ServiceControl' in tables:raise ValueError('Unexpected launcher/service ownership')
        if any(row[1]!='1' for row in rows.get('Registry',[])):raise ValueError('Unexpected registry root outside HKCU')
        if any(row[2] != '' or row[4] != '2' for row in rows.get('RemoveFile',[])):raise ValueError('Unexpected file removal beyond uninstall-only empty folder cleanup')
        return {'properties':properties,'table_names':sorted(tables),'file_count':len(rows['File']),'component_count':len(rows['Component']),
                'registry_rows':len(rows.get('Registry',[])),'custom_actions':rows.get('CustomAction',[]),'upgrade_rows':rows.get('Upgrade',[]),
                'directory_rows':rows['Directory'],'install_sequence':rows['InstallExecuteSequence'],'raw_tables':rows}

if __name__=='__main__':
    result=inspect(Path(sys.argv[1]));destination=Path(sys.argv[2])
    with destination.open('x',encoding='utf-8') as out:json.dump(result,out,indent=2);out.write('\n')
    print(json.dumps({key:result[key] for key in ('properties','file_count','component_count','registry_rows','custom_actions','upgrade_rows')},indent=2))
