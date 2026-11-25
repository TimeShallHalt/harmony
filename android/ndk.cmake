# Read the ABI chosen for this individual Cargo invocation, also during try_compile.
set(ANDROID_ABI "$ENV{ANDROID_ABI}" CACHE STRING "" FORCE)
set(ANDROID_PLATFORM android-28 CACHE STRING "" FORCE)
file(TO_CMAKE_PATH "$ENV{ANDROID_NDK_HOME}" harmony_ndk)
include("${harmony_ndk}/build/cmake/android.toolchain.cmake")
