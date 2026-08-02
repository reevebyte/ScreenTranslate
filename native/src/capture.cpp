#include "capture.hpp"

#include "util.hpp"

#include <cstring>

namespace screentrans {

namespace {

class ScreenDc {
public:
    ScreenDc() : value_(GetDC(nullptr)) {
        if (!value_) {
            throw_last_error("GetDC failed");
        }
    }
    ~ScreenDc() { ReleaseDC(nullptr, value_); }
    operator HDC() const noexcept { return value_; }

private:
    HDC value_{};
};

class MemoryDc {
public:
    explicit MemoryDc(HDC compatible) : value_(CreateCompatibleDC(compatible)) {
        if (!value_) {
            throw_last_error("CreateCompatibleDC failed");
        }
    }
    ~MemoryDc() { DeleteDC(value_); }
    operator HDC() const noexcept { return value_; }

private:
    HDC value_{};
};

}  // namespace

PixelBuffer capture_rect(const RECT& physical_rect) {
    const int width = std::max(1L, physical_rect.right - physical_rect.left);
    const int height = std::max(1L, physical_rect.bottom - physical_rect.top);

    ScreenDc screen;
    MemoryDc memory(screen);

    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;

    void* bitmap_bits = nullptr;
    HBITMAP bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &bitmap_bits, nullptr, 0);
    if (!bitmap || !bitmap_bits) {
        throw_last_error("CreateDIBSection failed");
    }
    const auto old = SelectObject(memory, bitmap);
    if (!old || old == HGDI_ERROR) {
        DeleteObject(bitmap);
        throw_last_error("SelectObject failed");
    }

    const BOOL copied = BitBlt(memory, 0, 0, width, height, screen,
                               physical_rect.left, physical_rect.top,
                               SRCCOPY | CAPTUREBLT);
    GdiFlush();

    PixelBuffer result;
    if (copied) {
        result.width = width;
        result.height = height;
        result.stride = width * 4;
        result.bgra.resize(static_cast<std::size_t>(result.stride) * result.height);
        std::memcpy(result.bgra.data(), bitmap_bits, result.bgra.size());
        // BI_RGB alpha bytes are undefined. Make them deterministic for WIC,
        // Windows OCR, and Direct2D consumers.
        for (std::size_t index = 3; index < result.bgra.size(); index += 4) {
            result.bgra[index] = 0xFF;
        }
    }

    SelectObject(memory, old);
    DeleteObject(bitmap);
    if (!copied) {
        throw_last_error("BitBlt screen capture failed");
    }
    return result;
}

DesktopImage capture_virtual_desktop() {
    DesktopImage result;
    result.bounds.left = GetSystemMetrics(SM_XVIRTUALSCREEN);
    result.bounds.top = GetSystemMetrics(SM_YVIRTUALSCREEN);
    result.bounds.right = result.bounds.left + GetSystemMetrics(SM_CXVIRTUALSCREEN);
    result.bounds.bottom = result.bounds.top + GetSystemMetrics(SM_CYVIRTUALSCREEN);
    result.pixels = capture_rect(result.bounds);
    return result;
}

}  // namespace screentrans
