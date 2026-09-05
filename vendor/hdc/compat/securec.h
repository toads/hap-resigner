#ifndef HDC_INLINE_SECUREC_H
#define HDC_INLINE_SECUREC_H

#include <errno.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>

#ifndef _WIN32
typedef int errno_t;
#ifndef EOK
#define EOK 0
#endif

static inline errno_t memcpy_s(void *dest, size_t dest_max, const void *src, size_t count)
{
    if (dest == NULL || src == NULL || count > dest_max) {
        if (dest != NULL && dest_max > 0) {
            memset(dest, 0, dest_max);
        }
        return EINVAL;
    }
    memcpy(dest, src, count);
    return EOK;
}

static inline errno_t memset_s(void *dest, size_t dest_max, int value, size_t count)
{
    if (dest == NULL || count > dest_max) {
        return EINVAL;
    }
    memset(dest, value, count);
    return EOK;
}

static inline errno_t strcpy_s(char *dest, size_t dest_max, const char *src)
{
    if (dest == NULL || src == NULL) {
        return EINVAL;
    }
    size_t length = strlen(src);
    if (length + 1 > dest_max) {
        if (dest_max > 0) {
            dest[0] = '\0';
        }
        return ERANGE;
    }
    memcpy(dest, src, length + 1);
    return EOK;
}

static inline int vsnprintf_s(char *dest, size_t dest_max, size_t count,
                              const char *format, va_list args)
{
    if (dest == NULL || dest_max == 0 || format == NULL) {
        return -1;
    }
    size_t limit = count + 1 < dest_max ? count + 1 : dest_max;
    int result = vsnprintf(dest, limit, format, args);
    if (result < 0 || (size_t)result >= limit) {
        dest[dest_max - 1] = '\0';
        return -1;
    }
    return result;
}

static inline int snprintf_s(char *dest, size_t dest_max, size_t count,
                             const char *format, ...)
{
    va_list args;
    va_start(args, format);
    int result = vsnprintf_s(dest, dest_max, count, format, args);
    va_end(args);
    return result;
}

static inline int sprintf_s(char *dest, size_t dest_max, const char *format, ...)
{
    va_list args;
    va_start(args, format);
    int result = vsnprintf_s(dest, dest_max, dest_max - 1, format, args);
    va_end(args);
    return result;
}

#else
#ifndef EOK
#define EOK 0
#endif
#if defined(_MSC_VER)
#define snprintf_s _snprintf_s
#endif
static inline errno_t memset_s(void *dest, size_t dest_max, int value, size_t count)
{
    if (dest == NULL || count > dest_max) {
        return EINVAL;
    }
    volatile unsigned char *cursor = static_cast<volatile unsigned char *>(dest);
    for (size_t index = 0; index < count; ++index) {
        cursor[index] = static_cast<unsigned char>(value);
    }
    return EOK;
}
#endif

#endif
