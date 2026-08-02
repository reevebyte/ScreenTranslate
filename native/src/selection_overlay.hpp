#pragma once

#include "capture.hpp"

#include <windows.h>

#include <functional>
#include <memory>
#include <string>
#include <vector>

namespace screentrans {

class SelectionOverlay {
public:
    using SelectedCallback = std::function<void(const RECT&, PixelBuffer)>;
    using CancelledCallback = std::function<void()>;

    explicit SelectionOverlay(HINSTANCE instance);
    ~SelectionOverlay();

    SelectionOverlay(const SelectionOverlay&) = delete;
    SelectionOverlay& operator=(const SelectionOverlay&) = delete;

    bool start(std::wstring_view accent,
               SelectedCallback selected,
               CancelledCallback cancelled = {});
    void close();
    [[nodiscard]] bool active() const noexcept { return !windows_.empty(); }

private:
    struct WindowState;

    static LRESULT CALLBACK window_proc(HWND window, UINT message,
                                        WPARAM wparam, LPARAM lparam);
    LRESULT handle_message(WindowState& state, UINT message,
                           WPARAM wparam, LPARAM lparam);
    void paint(WindowState& state);
    POINT local_cursor(const WindowState& state) const;
    RECT selection_rect(const WindowState& state) const;
    void finish_selection(WindowState& state);
    void cancel();
    bool ensure_paint_resources(WindowState& state, HDC compatible,
                                int width, int height);
    void release_paint_resources(WindowState& state) noexcept;

    HINSTANCE instance_{};
    std::vector<std::unique_ptr<WindowState>> windows_;
    DesktopImage desktop_;
    COLORREF accent_{RGB(40, 199, 111)};
    bool finishing_{};
    SelectedCallback selected_;
    CancelledCallback cancelled_;
};

}  // namespace screentrans
