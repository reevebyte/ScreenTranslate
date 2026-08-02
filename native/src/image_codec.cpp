#include "image_codec.hpp"

#include "util.hpp"

#include <wincodec.h>
#include <objidl.h>

#include <cstring>

namespace screentrans {

namespace {

template <typename T>
class ComPtr {
public:
    ~ComPtr() { reset(); }
    T** put() {
        reset();
        return &value_;
    }
    T* get() const noexcept { return value_; }
    T* operator->() const noexcept { return value_; }
    void reset() noexcept {
        if (value_) {
            value_->Release();
            value_ = nullptr;
        }
    }

private:
    T* value_{};
};

void check(HRESULT result, std::string_view operation) {
    if (FAILED(result)) {
        throw AppError(std::string(operation) + " failed (HRESULT " +
                       std::to_string(static_cast<unsigned long>(result)) + ")");
    }
}

}  // namespace

std::vector<std::uint8_t> encode_png(const PixelBuffer& image) {
    if (image.empty()) {
        throw AppError("cannot encode an empty image");
    }

    ComPtr<IWICImagingFactory> factory;
    check(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER,
                           IID_PPV_ARGS(factory.put())), "create WIC factory");

    ComPtr<IWICBitmap> bitmap;
    check(factory->CreateBitmapFromMemory(
              static_cast<UINT>(image.width), static_cast<UINT>(image.height),
              GUID_WICPixelFormat32bppBGRA, static_cast<UINT>(image.stride),
              static_cast<UINT>(image.bgra.size()),
              const_cast<BYTE*>(image.bgra.data()), bitmap.put()),
          "create WIC bitmap");

    ComPtr<IStream> stream;
    check(CreateStreamOnHGlobal(nullptr, TRUE, stream.put()), "create memory stream");

    ComPtr<IWICBitmapEncoder> encoder;
    check(factory->CreateEncoder(GUID_ContainerFormatPng, nullptr, encoder.put()),
          "create PNG encoder");
    check(encoder->Initialize(stream.get(), WICBitmapEncoderNoCache), "initialize PNG encoder");

    ComPtr<IWICBitmapFrameEncode> frame;
    ComPtr<IPropertyBag2> options;
    check(encoder->CreateNewFrame(frame.put(), options.put()), "create PNG frame");
    check(frame->Initialize(options.get()), "initialize PNG frame");
    check(frame->SetSize(static_cast<UINT>(image.width), static_cast<UINT>(image.height)),
          "set PNG size");
    WICPixelFormatGUID format = GUID_WICPixelFormat32bppBGRA;
    check(frame->SetPixelFormat(&format), "set PNG pixel format");
    check(frame->WriteSource(bitmap.get(), nullptr), "write PNG pixels");
    check(frame->Commit(), "commit PNG frame");
    check(encoder->Commit(), "commit PNG encoder");

    HGLOBAL memory = nullptr;
    check(GetHGlobalFromStream(stream.get(), &memory), "read PNG memory stream");
    const SIZE_T size = GlobalSize(memory);
    const void* bytes = GlobalLock(memory);
    if (!bytes || size == 0) {
        if (bytes) {
            GlobalUnlock(memory);
        }
        throw AppError("PNG encoder produced no data");
    }
    std::vector<std::uint8_t> output(size);
    std::memcpy(output.data(), bytes, size);
    GlobalUnlock(memory);
    return output;
}

PixelBuffer resize_bgra(const PixelBuffer& image, int width, int height) {
    if (image.empty() || width <= 0 || height <= 0) {
        throw AppError("cannot resize an empty image");
    }
    ComPtr<IWICImagingFactory> factory;
    check(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER,
                           IID_PPV_ARGS(factory.put())), "create WIC factory");
    ComPtr<IWICBitmap> bitmap;
    check(factory->CreateBitmapFromMemory(
              static_cast<UINT>(image.width), static_cast<UINT>(image.height),
              GUID_WICPixelFormat32bppBGRA, static_cast<UINT>(image.stride),
              static_cast<UINT>(image.bgra.size()),
              const_cast<BYTE*>(image.bgra.data()), bitmap.put()),
          "create WIC bitmap");
    ComPtr<IWICBitmapScaler> scaler;
    check(factory->CreateBitmapScaler(scaler.put()), "create WIC scaler");
    check(scaler->Initialize(bitmap.get(), static_cast<UINT>(width),
                             static_cast<UINT>(height), WICBitmapInterpolationModeFant),
          "resize image");
    PixelBuffer result;
    result.width = width;
    result.height = height;
    result.stride = width * 4;
    result.bgra.resize(static_cast<std::size_t>(result.stride) * result.height);
    check(scaler->CopyPixels(nullptr, static_cast<UINT>(result.stride),
                             static_cast<UINT>(result.bgra.size()), result.bgra.data()),
          "copy resized pixels");
    return result;
}

}  // namespace screentrans
