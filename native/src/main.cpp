#include "app.hpp"
#include "rapidocr_plugin.hpp"
#include "result_renderer.hpp"
#include "updater.hpp"
#include "util.hpp"

#include <shellapi.h>
#include <winrt/base.h>

#include <cstdlib>
#include <cerrno>
#include <cwchar>
#include <optional>
#include <string>
#include <string_view>

namespace {

constexpr wchar_t mutex_name[] = L"Global\\ScreenTranslate.SingleInstance.v1";
constexpr DWORD restart_wait_timeout_ms = 60'000;

void initialize_dpi_awareness() noexcept {
    if (!SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) &&
        GetLastError() != ERROR_ACCESS_DENIED) {
        SetProcessDPIAware();
    }
}

bool has_argument(std::wstring_view expected) {
    int count = 0;
    LPWSTR* arguments = CommandLineToArgvW(GetCommandLineW(), &count);
    if (!arguments) return false;
    bool found = false;
    for (int index = 1; index < count; ++index) {
        if (expected == arguments[index]) { found = true; break; }
    }
    LocalFree(arguments);
    return found;
}

void report_self_test_error(std::string_view message) noexcept {
    const HANDLE error = GetStdHandle(STD_ERROR_HANDLE);
    if (!error || error == INVALID_HANDLE_VALUE) return;
    constexpr std::string_view prefix = "ScreenTranslate self-test failed: ";
    DWORD written = 0;
    WriteFile(error, prefix.data(), static_cast<DWORD>(prefix.size()), &written, nullptr);
    WriteFile(error, message.data(), static_cast<DWORD>(message.size()), &written, nullptr);
    constexpr std::string_view newline = "\r\n";
    WriteFile(error, newline.data(), static_cast<DWORD>(newline.size()), &written, nullptr);
}

std::optional<DWORD> argument_process_id(std::wstring_view name) {
    int count = 0;
    LPWSTR* arguments = CommandLineToArgvW(GetCommandLineW(), &count);
    if (!arguments) return std::nullopt;

    std::optional<DWORD> result;
    for (int index = 1; index + 1 < count; ++index) {
        if (name != arguments[index]) continue;
        wchar_t* end = nullptr;
        errno = 0;
        const unsigned long value = std::wcstoul(arguments[index + 1], &end, 10);
        if (errno == 0 && end && *end == L'\0' && value != 0) {
            result = static_cast<DWORD>(value);
        }
        break;
    }
    LocalFree(arguments);
    return result;
}

void wait_for_restart_parent() {
    const auto process_id = argument_process_id(L"--restart-wait-pid");
    if (!process_id) return;

    SetLastError(ERROR_SUCCESS);
    screentrans::UniqueHandle process(OpenProcess(SYNCHRONIZE, FALSE, *process_id));
    if (!process) {
        const DWORD error = GetLastError();
        if (error == ERROR_INVALID_PARAMETER) return;
        screentrans::throw_last_error("open previous process", error);
    }
    const DWORD wait_result = WaitForSingleObject(process.get(), restart_wait_timeout_ms);
    if (wait_result == WAIT_FAILED) screentrans::throw_last_error("wait for previous process");
    if (wait_result == WAIT_TIMEOUT) {
        throw screentrans::AppError("previous ScreenTranslate process did not exit in time");
    }
}

void signal_existing_instance() noexcept {
    if (const HWND window = FindWindowW(screentrans::AppHost::window_class_name(), nullptr)) {
        PostMessageW(window, screentrans::AppHost::activate_message, 0, 0);
    }
}

}  // namespace

int WINAPI wWinMain(_In_ HINSTANCE instance, _In_opt_ HINSTANCE,
                    _In_ PWSTR, _In_ int) {
    initialize_dpi_awareness();
    const bool self_test =
        has_argument(L"--self-test") || has_argument(L"--selftest");
    const bool update_self_test = has_argument(L"--update-self-test");
    bool update_helper = false;
    try {
        update_helper = screentrans::is_update_helper_request();
        if (update_helper) return screentrans::run_update_helper();
        if (self_test) {
            screentrans::util_self_test();
            screentrans::updater_self_test();
            screentrans::rapidocr_plugin_self_test();
            screentrans::result_renderer_self_test();
        }
        screentrans::UniqueHandle mutex;
        if (!self_test && !update_self_test) {
            wait_for_restart_parent();
            SetLastError(ERROR_SUCCESS);
            HANDLE mutex_handle = CreateMutexW(nullptr, FALSE, mutex_name);
            const DWORD mutex_error = GetLastError();
            mutex.reset(mutex_handle);
            if (!mutex && mutex_error != ERROR_ACCESS_DENIED) {
                screentrans::throw_last_error("create mutex", mutex_error);
            }
            if (!mutex || mutex_error == ERROR_ALREADY_EXISTS) {
                signal_existing_instance();
                return EXIT_SUCCESS;
            }
        }
        int result = EXIT_FAILURE;
        {
            screentrans::WinrtApartment apartment(winrt::apartment_type::single_threaded);
            {
                screentrans::AppHost app(instance);
                result = app.run(self_test, update_self_test);
            }
        }
        return result;
    } catch (const std::exception& error) {
        if (self_test || update_self_test) {
            report_self_test_error(error.what());
            return EXIT_FAILURE;
        }
        const auto message = std::wstring(update_helper ? L"无法启动更新安装程序。\n\n"
                                                         : L"ScreenTranslate 无法启动。\n\n") +
                             screentrans::utf8_to_wide(error.what());
        MessageBoxW(nullptr, message.c_str(), update_helper ? L"ScreenTranslate 更新" : L"ScreenTranslate",
                    MB_OK | MB_ICONERROR);
        return EXIT_FAILURE;
    } catch (...) {
        if (self_test || update_self_test) {
            report_self_test_error("unknown exception");
            return EXIT_FAILURE;
        }
        MessageBoxW(nullptr, L"ScreenTranslate 遇到未知启动错误。",
                    L"ScreenTranslate", MB_OK | MB_ICONERROR);
        return EXIT_FAILURE;
    }
}
