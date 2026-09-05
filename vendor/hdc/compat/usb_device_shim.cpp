#include "ffi_utils.h"
#include "securec.h"

#include <cstdint>
#include <fcntl.h>
#ifdef _WIN32
#include <io.h>
#define hdc_open _open
#define hdc_close _close
#define hdc_read _read
#define hdc_write _write
#else
#include <unistd.h>
#define hdc_open open
#define hdc_close close
#define hdc_read read
#define hdc_write write
#endif

namespace Hdc {
extern "C" int32_t ConfigEpPointEx(const char *path)
{
    return hdc_open(path, O_RDWR);
}

extern "C" int32_t OpenEpPointEx(const char *path)
{
    return hdc_open(path, O_RDWR);
}

extern "C" int32_t CloseUsbFdEx(int32_t fd)
{
    return hdc_close(fd);
}

extern "C" void CloseEndPointEx(int32_t bulkInFd, int32_t bulkOutFd, int32_t ctrlEp,
                                uint8_t closeCtrlEp)
{
    if (bulkInFd >= 0) hdc_close(bulkInFd);
    if (bulkOutFd >= 0) hdc_close(bulkOutFd);
    if (closeCtrlEp != 0 && ctrlEp >= 0) hdc_close(ctrlEp);
}

extern "C" int32_t WriteUsbDevEx(int32_t fd, SerializedBuffer buffer)
{
    return static_cast<int32_t>(hdc_write(fd, buffer.ptr, static_cast<unsigned int>(buffer.size)));
}

extern "C" size_t ReadUsbDevEx(int32_t fd, uint8_t *buffer, size_t size)
{
    const auto result = hdc_read(fd, buffer, static_cast<unsigned int>(size));
    return result < 0 ? 0 : static_cast<size_t>(result);
}

extern "C" char *GetDevPathEx(const char *path)
{
    const size_t size = strlen(path) + 1;
    char *copy = new char[size];
    return strcpy_s(copy, size, path) == EOK ? copy : nullptr;
}
} // namespace Hdc
