"""Read-only Windows Installer APIs; never enumerate Win32_Product (it can repair apps)."""
import ctypes as c
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import re
import sys
import uuid

# Explicit column order is an API: lifecycle preflight must not guess SELECT * order.
TABLE_COLUMNS = {
    'Property': ('Property', 'Value'),
    'Directory': ('Directory', 'Directory_Parent', 'DefaultDir'),
    'Component': ('Component', 'ComponentId', 'Directory_', 'Attributes', 'Condition', 'KeyPath'),
    'File': ('File', 'Component_', 'FileName', 'FileSize', 'Version', 'Language', 'Attributes', 'Sequence'),
    'Registry': ('Registry', 'Root', 'Key', 'Name', 'Value', 'Component_'),
    'Upgrade': ('UpgradeCode', 'VersionMin', 'VersionMax', 'Language', 'Attributes', 'Remove', 'ActionProperty'),
    'CustomAction': ('Action', 'Type', 'Source', 'Target'),
    'InstallExecuteSequence': ('Action', 'Condition', 'Sequence'),
    'InstallUISequence': ('Action', 'Condition', 'Sequence'),
    'RemoveFile': ('FileKey', 'Component_', 'FileName', 'DirProperty', 'InstallMode'),
    'LaunchCondition': ('Condition', 'Description'),
    'AppSearch': ('Property', 'Signature_'),
    'RegLocator': ('Signature_', 'Root', 'Key', 'Name', 'Type'),
}


def msi_api():
    if os.name != 'nt':
        raise OSError('Native MSI inspection requires Windows')
    return c.WinDLL('msi', use_last_error=True)


def guid_text(value):
    """Validate, do not repair, a canonical UUID (optionally brace-delimited)."""
    text = str(value)
    if not re.fullmatch(r'(?:[0-9a-fA-F]{8}-(?:[0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}|\{[0-9a-fA-F]{8}-(?:[0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}\})', text):
        raise ValueError('Expected canonical MSI GUID')
    return '{' + str(uuid.UUID(text)).upper() + '}'


def product_state(product_code):
    """MsiQueryProductStateW: -1 unknown, 5 installed; no repair or registration."""
    code = guid_text(product_code)
    api = msi_api()
    api.MsiQueryProductStateW.argtypes = [w.LPCWSTR]
    api.MsiQueryProductStateW.restype = c.c_int
    return int(api.MsiQueryProductStateW(code))


def related_products(upgrade_code):
    """Read-only enumeration restricted to one explicit UpgradeCode."""
    code = guid_text(upgrade_code)
    api = msi_api()
    api.MsiEnumRelatedProductsW.argtypes = [w.LPCWSTR, w.DWORD, w.DWORD, w.LPWSTR]
    api.MsiEnumRelatedProductsW.restype = w.UINT
    result = []
    while True:
        buffer = c.create_unicode_buffer(39)
        status = api.MsiEnumRelatedProductsW(code, 0, len(result), buffer)
        if status == 259:
            return result
        Database.check(status)
        result.append(guid_text(buffer.value))


class Database:
    def __init__(self, path):
        self.api = msi_api()
        signatures = {
            'MsiOpenDatabaseW': [w.LPCWSTR, w.LPCWSTR, c.POINTER(w.UINT)],
            'MsiDatabaseOpenViewW': [w.UINT, w.LPCWSTR, c.POINTER(w.UINT)],
            'MsiViewExecute': [w.UINT, w.UINT],
            'MsiViewFetch': [w.UINT, c.POINTER(w.UINT)],
            'MsiRecordGetFieldCount': [w.UINT],
            'MsiRecordGetStringW': [w.UINT, w.UINT, w.LPWSTR, c.POINTER(w.DWORD)],
            'MsiRecordReadStream': [w.UINT, w.UINT, c.c_void_p, c.POINTER(w.DWORD)],
            'MsiCloseHandle': [w.UINT],
            'MsiGetSummaryInformationW': [w.UINT, w.LPCWSTR, w.UINT, c.POINTER(w.UINT)],
            'MsiSummaryInfoGetPropertyW': [w.UINT, w.UINT, c.POINTER(w.UINT), c.POINTER(c.c_int),
                                           c.POINTER(w.FILETIME), w.LPWSTR, c.POINTER(w.DWORD)],
        }
        for name, arguments in signatures.items():
            function = getattr(self.api, name)
            function.argtypes = arguments
            function.restype = w.UINT
        self.handle = w.UINT()
        # MSIDBOPEN_READONLY is NULL; never transact or commit a database.
        self.check(self.api.MsiOpenDatabaseW(str(Path(path).resolve()), None, c.byref(self.handle)))

    @staticmethod
    def check(code):
        if code:
            raise OSError(code, 'MSI database API failed')

    def query(self, sql):
        if not sql.startswith('SELECT ') or ';' in sql:
            raise ValueError('Single read-only SELECT queries only')
        view = w.UINT()
        self.check(self.api.MsiDatabaseOpenViewW(self.handle, sql, c.byref(view)))
        rows = []
        try:
            self.check(self.api.MsiViewExecute(view, 0))
            while True:
                record = w.UINT()
                code = self.api.MsiViewFetch(view, c.byref(record))
                if code == 259:
                    break
                self.check(code)
                try:
                    fields = []
                    for index in range(1, self.api.MsiRecordGetFieldCount(record) + 1):
                        length = w.DWORD()
                        code = self.api.MsiRecordGetStringW(record, index, None, c.byref(length))
                        if code not in (0, 234):
                            self.check(code)
                        buffer = c.create_unicode_buffer(length.value + 1)
                        length.value = len(buffer)
                        self.check(self.api.MsiRecordGetStringW(record, index, buffer, c.byref(length)))
                        fields.append(buffer.value)
                    rows.append(fields)
                finally:
                    self.api.MsiCloseHandle(record)
        finally:
            self.api.MsiCloseHandle(view)
        return rows

    def summary_template(self):
        summary = w.UINT()
        self.check(self.api.MsiGetSummaryInformationW(self.handle, None, 0, c.byref(summary)))
        try:
            datatype = w.UINT()
            integer = c.c_int()
            timestamp = w.FILETIME()
            length = w.DWORD()
            arguments = (summary, 7, c.byref(datatype), c.byref(integer), c.byref(timestamp))
            status = self.api.MsiSummaryInfoGetPropertyW(*arguments, None, c.byref(length))
            if status not in (0, 234):
                self.check(status)
            buffer = c.create_unicode_buffer(length.value + 1)
            length.value = len(buffer)
            self.check(self.api.MsiSummaryInfoGetPropertyW(*arguments, buffer, c.byref(length)))
            return buffer.value
        finally:
            self.api.MsiCloseHandle(summary)

    def read_stream(self, select_sql, *, max_bytes=256 * 1024 * 1024):
        """Return one binary field from one SELECT row, bounded and read-only.

        Example: read_stream("SELECT `Data` FROM `Icon` WHERE `Name` = 'Harmony.ico'").
        Zero/multiple records or columns and oversized streams fail closed. Every
        view/record handle is closed, including on a read error or bound violation.
        """
        if not select_sql.startswith('SELECT ') or ';' in select_sql:
            raise ValueError('Single read-only SELECT queries only')
        if type(max_bytes) is not int or not 0 <= max_bytes <= 512 * 1024 * 1024:
            raise ValueError('Stream limit must be an integer from 0 through 512 MiB')
        view = w.UINT()
        self.check(self.api.MsiDatabaseOpenViewW(self.handle, select_sql, c.byref(view)))
        try:
            self.check(self.api.MsiViewExecute(view, 0))
            record = w.UINT()
            code = self.api.MsiViewFetch(view, c.byref(record))
            if code == 259:
                raise ValueError('Binary SELECT must return exactly one row')
            self.check(code)
            try:
                if self.api.MsiRecordGetFieldCount(record) != 1:
                    raise ValueError('Binary SELECT must return exactly one field')
                result = bytearray()
                while True:
                    # At the bound, read at most one extra byte to detect overflow.
                    size = min(64 * 1024, max_bytes - len(result) + 1)
                    buffer = c.create_string_buffer(size)
                    length = w.DWORD(size)
                    self.check(self.api.MsiRecordReadStream(record, 1, buffer, c.byref(length)))
                    if length.value == 0:
                        break
                    if length.value > size or len(result) + length.value > max_bytes:
                        raise ValueError('MSI binary stream exceeds the configured bound')
                    result.extend(buffer.raw[:length.value])
            finally:
                self.api.MsiCloseHandle(record)
            extra = w.UINT()
            code = self.api.MsiViewFetch(view, c.byref(extra))
            if code == 0:
                self.api.MsiCloseHandle(extra)
                raise ValueError('Binary SELECT must return exactly one row')
            if code != 259:
                self.check(code)
            return bytes(result)
        finally:
            self.api.MsiCloseHandle(view)

    def close(self):
        if self.handle.value:
            self.api.MsiCloseHandle(self.handle)
            self.handle = w.UINT()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def inspect(path):
    """Capture guard/search/sequence tables as well as payload ownership, read-only."""
    with Database(path) as db:
        tables = {row[0] for row in db.query('SELECT `Name` FROM `_Tables`')}
        required = {'Property', 'Directory', 'Component', 'File', 'Registry', 'Upgrade',
                    'AppSearch', 'RegLocator', 'LaunchCondition', 'InstallExecuteSequence', 'InstallUISequence'}
        if not required <= tables:
            raise ValueError('Missing required MSI tables: ' + ', '.join(sorted(required - tables)))
        rows = {
            name: db.query('SELECT ' + ', '.join('`' + column + '`' for column in columns) + ' FROM `' + name + '`')
            for name, columns in TABLE_COLUMNS.items() if name in tables
        }
        properties = dict(rows['Property'])
        if properties.get('ALLUSERS') not in (None, ''):
            raise ValueError('Not a per-user-only package')
        expected = {'REBOOT': 'ReallySuppress', 'MSIRESTARTMANAGERCONTROL': 'DisableShutdown', 'MSIDISABLERMRESTART': '1'}
        if any(properties.get(key) != value for key, value in expected.items()):
            raise ValueError('Automatic restart/shutdown policy changed')
        forbidden = {'Shortcut', 'ServiceInstall', 'ServiceControl', 'Environment', 'IniFile', 'RemoveIniFile',
                     'RemoveRegistry', 'MoveFiles', 'DuplicateFile', 'Class', 'ProgId', 'Extension', 'Verb', 'MIME',
                     'TypeLib', 'SelfReg', 'BindImage', 'IsolatedComponent', 'ODBCDataSource', 'ODBCDriver',
                     'ODBCTranslator', 'Complus', 'MsiLockPermissionsEx', 'LockPermissions'}
        if forbidden & tables:
            raise ValueError('Unexpected integration ownership: ' + ', '.join(sorted(forbidden & tables)))
        if any(row[1] != '1' for row in rows['Registry']):
            raise ValueError('Unexpected registry root outside HKCU')
        if any(row[2] != '' or row[4] != '2' for row in rows.get('RemoveFile', [])):
            raise ValueError('Unexpected file removal beyond uninstall-only empty folder cleanup')
        return {
            'properties': properties, 'table_names': sorted(tables), 'summary_template': db.summary_template(),
            'file_count': len(rows['File']), 'component_count': len(rows['Component']),
            'registry_rows': len(rows['Registry']), 'custom_actions': rows.get('CustomAction', []),
            'upgrade_rows': rows['Upgrade'], 'directory_rows': rows['Directory'],
            'install_sequence': rows['InstallExecuteSequence'], 'raw_tables': rows,
        }


if __name__ == '__main__':
    result = inspect(Path(sys.argv[1]))
    with Path(sys.argv[2]).open('x', encoding='utf-8') as out:
        json.dump(result, out, indent=2)
        out.write('\n')
    print(json.dumps({key: result[key] for key in ('properties', 'file_count', 'component_count',
                                                 'registry_rows', 'custom_actions', 'upgrade_rows')}, indent=2))
