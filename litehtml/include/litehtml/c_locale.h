// Numbers read and written as the C locale has them, whatever LC_NUMERIC the process chose: Telegram and Qt choose the user's.
#ifndef LITEHTML_C_LOCALE_H
#define LITEHTML_C_LOCALE_H

#include <clocale>
#include <cstdarg>
#include <cstddef>
#include <cstdio>
#include <cstdlib>
#include <string>
#if defined(__APPLE__)
#include <xlocale.h>
#endif

namespace litehtml
{
#if defined(_WIN32)
    inline _locale_t c_numeric_locale()
    {
        static const _locale_t c = _create_locale(LC_NUMERIC, "C");
        return c;
    }

    inline double c_strtod(const char* text, char** end)
    {
        return _strtod_l(text, end, c_numeric_locale());
    }

    inline float c_strtof(const char* text, char** end)
    {
        return _strtof_l(text, end, c_numeric_locale());
    }

    // The C runtime's own vsnprintf, with the C locale in place of the thread's.
    inline int c_vsnprintf(char* buffer, size_t size, const char* format, va_list args)
    {
        const int n = __stdio_common_vsprintf(_CRT_INTERNAL_LOCAL_PRINTF_OPTIONS | _CRT_INTERNAL_PRINTF_STANDARD_SNPRINTF_BEHAVIOR, buffer, size,
                                              format, c_numeric_locale(), args);
        return n < 0 ? -1 : n;
    }
#elif defined(__ANDROID__)
    // bionic's LC_NUMERIC is always "C".
    inline double c_strtod(const char* text, char** end)
    {
        return std::strtod(text, end);
    }

    inline float c_strtof(const char* text, char** end)
    {
        return std::strtof(text, end);
    }

    inline int c_vsnprintf(char* buffer, size_t size, const char* format, va_list args)
    {
        return std::vsnprintf(buffer, size, format, args);
    }
#else
    inline locale_t c_numeric_locale()
    {
        static const locale_t c = newlocale(LC_NUMERIC_MASK, "C", (locale_t)0);
        return c;
    }

    inline double c_strtod(const char* text, char** end)
    {
        return strtod_l(text, end, c_numeric_locale());
    }

    inline float c_strtof(const char* text, char** end)
    {
        return strtof_l(text, end, c_numeric_locale());
    }

    // glibc has no vsnprintf_l: the thread takes the C locale for the call.
    inline int c_vsnprintf(char* buffer, size_t size, const char* format, va_list args)
    {
        const locale_t previous = uselocale(c_numeric_locale());
        const int      n        = std::vsnprintf(buffer, size, format, args);
        uselocale(previous);
        return n;
    }
#endif

    inline double c_atof(const char* text)
    {
        return c_strtod(text, nullptr);
    }

    inline int c_snprintf(char* buffer, size_t size, const char* format, ...)
    {
        va_list args;
        va_start(args, format);
        const int n = c_vsnprintf(buffer, size, format, args);
        va_end(args);
        return n;
    }

    // std::to_string of a float or a double, which is "%f".
    inline std::string c_to_string(double value)
    {
        char buffer[64];
        const int n = c_snprintf(buffer, sizeof(buffer), "%f", value);
        if(n < 0)
            return {};
        if(size_t(n) < sizeof(buffer))
            return std::string(buffer, size_t(n));
        std::string out(size_t(n), '\0');
        c_snprintf(out.data(), out.size() + 1, "%f", value);
        return out;
    }
} // namespace litehtml

#endif // LITEHTML_C_LOCALE_H
