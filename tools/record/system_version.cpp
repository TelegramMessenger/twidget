// The system the recorder ran on, for the recording's origin, in a translation unit of its own so that windows.h's macros stay out of the recorder.
#include <string>

#if defined(__APPLE__)
#include <sys/sysctl.h>
#elif defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#else
#include <sys/utsname.h>
#endif

#if defined(__APPLE__)

std::string system_version() {
    char version[64] = {};
    size_t size = sizeof version - 1;
    if (sysctlbyname("kern.osproductversion", version, &size, nullptr, 0) != 0) return "macOS, version unknown";
    return "macOS " + std::string(version);
}

#elif defined(_WIN32)

std::string system_version() {
    // RtlGetVersion tells the version as it is; GetVersionEx tells what the program's manifest declares.
    using Get = LONG(WINAPI*)(OSVERSIONINFOW*);
    const HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
    const auto get = ntdll ? reinterpret_cast<Get>(reinterpret_cast<void*>(GetProcAddress(ntdll, "RtlGetVersion"))) : nullptr;
    OSVERSIONINFOW info = {};
    info.dwOSVersionInfoSize = sizeof info;
    if (!get || get(&info) != 0) return "Windows, version unknown";
    return "Windows " + std::to_string(info.dwMajorVersion) + "." + std::to_string(info.dwMinorVersion) + "." + std::to_string(info.dwBuildNumber);
}

#else

std::string system_version() {
    utsname name = {};
    if (uname(&name) != 0) return "unknown system";
    return std::string(name.sysname) + " " + name.release + " " + name.machine;
}

#endif
