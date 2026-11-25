"""Verify that both Android ABIs carry the application and its C++ runtime."""
from pathlib import Path
import struct
import sys
import zipfile


def verify(apk):
    with zipfile.ZipFile(apk) as archive:
        if archive.testzip() is not None:
            raise ValueError("Corrupt APK")
        for abi, machine in {"arm64-v8a": 183, "x86_64": 62}.items():
            for library in ("libharmony_android.so", "libc++_shared.so"):
                member = f"lib/{abi}/{library}"
                with archive.open(member) as f:
                    header = f.read(64)
                    if header[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", header, 18)[0] != machine:
                        raise ValueError(f"Incorrect architecture in {member}")
                    offset = struct.unpack_from("<Q", header, 32)[0]
                    entry_size, count = struct.unpack_from("<HH", header, 54)
                    f.seek(offset)
                    for _ in range(count):
                        entry = f.read(entry_size)
                        if struct.unpack_from("<I", entry)[0] == 1 and struct.unpack_from("<Q", entry, 48)[0] < 16384:
                            raise ValueError(f"Unsupported 16 KiB page alignment in {member}")
    print(f"Verified arm64-v8a and x86_64 libraries in {apk.name}")


if __name__ == "__main__":
    verify(Path(sys.argv[1]))
