"""Cross-compile the Rust activity and stage its native libraries for Gradle."""
import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
ABIS = {"arm64-v8a": "aarch64-linux-android", "x86_64": "x86_64-linux-android"}

def build(abi, variant, fixture, output):
    target = ABIS[abi]
    env = os.environ.copy()
    ndk = Path(env["ANDROID_NDK_HOME"]).resolve()
    host = {"Windows": "windows-x86_64", "Linux": "linux-x86_64", "Darwin": "darwin-x86_64"}[platform.system()]
    toolchain = ndk / "toolchains/llvm/prebuilt" / host
    suffix = ".exe" if os.name == "nt" else ""
    binary = toolchain / "bin"
    cc = str(binary / ("clang" + suffix))
    arch = target.replace("-", "_")
    clang_target = f"--target={target}28"
    out = ROOT / "target"
    env.update({
        "CARGO_TARGET_DIR": str(out), "ANDROID_NDK_ROOT": str(ndk), "ANDROID_NDK": str(ndk),
        f"CC_{arch}": cc, f"CXX_{arch}": str(binary / ("clang++" + suffix)),
        f"AR_{arch}": str(binary / ("llvm-ar" + suffix)),
        f"RANLIB_{arch}": str(binary / ("llvm-ranlib" + suffix)),
        f"CFLAGS_{arch}": clang_target, f"CXXFLAGS_{arch}": clang_target,
        f"CARGO_TARGET_{arch.upper()}_LINKER": cc,
        f"CARGO_TARGET_{arch.upper()}_RUSTFLAGS": f"-Clink-arg={clang_target} -Clink-arg=-Wl,-z,max-page-size=16384 -Clink-arg=-lc++_shared",
        "ANDROID_JAR": str(Path(env["ANDROID_HOME"]) / "platforms/android-35/android.jar"),
        "LIBOPUS_NO_PKG": "1", "CMAKE_GENERATOR": "Ninja", "ANDROID_ABI": abi, "ANDROID_PLATFORM": "android-28",
        "CMAKE_TOOLCHAIN_FILE": str(ROOT / "android/ndk.cmake"),
        "CARGO_NDK_ANDROID_PLATFORM": "28", "CARGO_NDK_ANDROID_TARGET": abi,
    })
    # cmake-rs forwards the NDK toolchain file, which selects the ABI from Cargo's target.
    flags = ["--release"] if variant == "release" else []
    if fixture:
        if variant == "release":
            raise SystemExit("Fixture builds are debug only")
        flags += ["--features", "fixture"]
    subprocess.run(["cargo", "build", "--locked", "-p", "harmony-android", "--target", target, *flags], cwd=ROOT, env=env, check=True)
    flavor = "fixture" if fixture else "live"
    dest = (output or ROOT / "android/app/build/rust" / (flavor + variant.capitalize())) / abi
    dest.mkdir(parents=True, exist_ok=True)
    library = dest / "libharmony_android.so"
    shutil.copyfile(out / target / variant / library.name, library)
    runtime = dest / "libc++_shared.so"
    shutil.copyfile(toolchain / "sysroot/usr/lib" / target / runtime.name, runtime)
    # Preserve the unstripped Cargo output for a debugger. Only strip the copies going into the APK.
    subprocess.run([str(binary / ("llvm-strip" + suffix)), "--strip-unneeded", str(library), str(runtime)], check=True)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--abi", action="append", choices=ABIS)
    parser.add_argument("--variant", choices=["debug", "release"], default="debug")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--fixture", action="store_true")
    args = parser.parse_args()
    for abi in args.abi or ABIS:
        build(abi, args.variant, args.fixture, args.output)
