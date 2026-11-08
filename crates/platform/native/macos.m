#import <AppKit/AppKit.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <Vision/Vision.h>
#import <CoreGraphics/CoreGraphics.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>

// Small ABI bridge for Apple frameworks. All application state stays in Rust.
@interface STCaptureReply : NSObject
@property(nonatomic, strong) NSData *data;
@property(nonatomic, strong) NSError *error;
@end
@implementation STCaptureReply
@end

static pthread_mutex_t g_tcc_mutex = PTHREAD_MUTEX_INITIALIZER;
static BOOL g_tcc_reset_this_run = NO;

static NSString *st_current_binary_tcc_key(void) {
    NSBundle *bundle = [NSBundle mainBundle];
    NSString *version = [bundle objectForInfoDictionaryKey:@"CFBundleShortVersionString"] ?: @"0";
    NSString *execPath = [bundle executablePath] ?: @"";
    NSDictionary *attrs = [[NSFileManager defaultManager] attributesOfItemAtPath:execPath error:nil];
    NSDate *mtime = [attrs fileModificationDate];
    unsigned long long size = [attrs fileSize];
    long long ts = mtime ? (long long)[mtime timeIntervalSince1970] : 0;
    return [NSString stringWithFormat:@"%@:%lld:%llu", version, ts, size];
}

static void st_reset_screen_capture_tcc(void) {
    @try {
        NSString *bundleId = [[NSBundle mainBundle] bundleIdentifier] ?: @"io.github.reevebyte.screentranslate.v2";
        NSTask *task = [[NSTask alloc] init];
        [task setLaunchPath:@"/usr/bin/tccutil"];
        [task setArguments:@[@"reset", @"ScreenCapture", bundleId]];
        [task setStandardOutput:[NSPipe pipe]];
        [task setStandardError:[NSPipe pipe]];
        [task launch];
        [task waitUntilExit];
    } @catch (NSException *exception) {
    }
}

static void st_ensure_screen_tcc_fresh(BOOL force_on_denied) {
    pthread_mutex_lock(&g_tcc_mutex);
    NSString *sig = st_current_binary_tcc_key();
    NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
    NSString *prepared = [defaults stringForKey:@"ScreenTranslateTCCBinaryKey"];
    if (![sig isEqualToString:prepared] && !g_tcc_reset_this_run) {
        if (force_on_denied || !CGPreflightScreenCaptureAccess()) {
            g_tcc_reset_this_run = YES;
            st_reset_screen_capture_tcc();
            if (force_on_denied) {
                CGRequestScreenCaptureAccess();
            }
        }
    }
    pthread_mutex_unlock(&g_tcc_mutex);
}

static void st_mark_screen_tcc_verified(void) {
    pthread_mutex_lock(&g_tcc_mutex);
    NSString *sig = st_current_binary_tcc_key();
    NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
    if (![sig isEqualToString:[defaults stringForKey:@"ScreenTranslateTCCBinaryKey"]]) {
        [defaults setObject:sig forKey:@"ScreenTranslateTCCBinaryKey"];
        [defaults synchronize];
    }
    pthread_mutex_unlock(&g_tcc_mutex);
}

void st_free(void *pointer) { free(pointer); }

unsigned char *st_capture_display(uint32_t displayID, size_t *length, char **error) {
    @autoreleasepool {
        st_ensure_screen_tcc_fresh(NO);
        if (!CGPreflightScreenCaptureAccess() && !CGRequestScreenCaptureAccess()) {
            *error = strdup("screen_recording_permission");
            return NULL;
        }
        STCaptureReply *reply = [STCaptureReply new];
        dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
        [SCShareableContent getShareableContentExcludingDesktopWindows:NO onScreenWindowsOnly:YES
            completionHandler:^(SCShareableContent *content, NSError *failure) {
                if (failure) { reply.error = failure; dispatch_semaphore_signal(semaphore); return; }
                SCDisplay *selected = nil;
                for (SCDisplay *display in content.displays) { if (display.displayID == displayID) { selected = display; break; } }
                if (!selected) { dispatch_semaphore_signal(semaphore); return; }
                SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:selected excludingWindows:@[]];
                SCStreamConfiguration *configuration = [SCStreamConfiguration new];
                configuration.width = CGDisplayPixelsWide(displayID);
                configuration.height = CGDisplayPixelsHigh(displayID);
                configuration.showsCursor = NO;
                [SCScreenshotManager captureImageWithFilter:filter configuration:configuration
                    completionHandler:^(CGImageRef image, NSError *captureError) {
                        reply.error = captureError;
                        if (image) {
                            NSBitmapImageRep *bitmap = [[NSBitmapImageRep alloc] initWithCGImage:image];
                            reply.data = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
                        }
                        dispatch_semaphore_signal(semaphore);
                    }];
            }];
        if (dispatch_semaphore_wait(semaphore, dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC)) != 0 || !reply.data) {
            if (reply.error) {
                NSInteger code = reply.error.code;
                NSString *desc = [reply.error localizedDescription];
                if (code == -3801 || (desc && ([desc localizedCaseInsensitiveContainsString:@"declined"] ||
                                               [desc localizedCaseInsensitiveContainsString:@"denied"] ||
                                               [desc localizedCaseInsensitiveContainsString:@"permission"]))) {
                    st_ensure_screen_tcc_fresh(YES);
                    *error = strdup("screen_recording_permission");
                    return NULL;
                }
            }
            *error = strdup("capture_failed");
            return NULL;
        }
        st_mark_screen_tcc_verified();
        *length = reply.data.length;
        unsigned char *result = malloc(*length);
        if (result) { memcpy(result, reply.data.bytes, *length); }
        return result;
    }
}

char *st_recognize(const unsigned char *png, size_t length, const char *languageJSON) {
    @autoreleasepool {
        NSData *data = [NSData dataWithBytes:png length:length];
        NSBitmapImageRep *bitmap = [NSBitmapImageRep imageRepWithData:data];
        if (!bitmap) { return NULL; }
        VNRecognizeTextRequest *request = [VNRecognizeTextRequest new];
        request.recognitionLevel = VNRequestTextRecognitionLevelAccurate;
        request.usesLanguageCorrection = YES;
        NSArray *languages = [NSJSONSerialization JSONObjectWithData:[[NSString stringWithUTF8String:languageJSON] dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil];
        NSArray *supported = [request supportedRecognitionLanguagesAndReturnError:nil];
        NSMutableArray *filtered = [NSMutableArray array];
        for (NSString *language in languages) {
            NSString *mapped = [language hasPrefix:@"zh-Hans"] ? @"zh-Hans" : language;
            if ([supported containsObject:mapped]) { [filtered addObject:mapped]; }
        }
        if (filtered.count) { request.recognitionLanguages = filtered; }
        VNImageRequestHandler *handler = [[VNImageRequestHandler alloc] initWithData:data options:@{}];
        NSError *error = nil;
        if (![handler performRequests:@[request] error:&error]) { return NULL; }
        NSMutableArray *lines = [NSMutableArray array];
        for (VNRecognizedTextObservation *observation in request.results) {
            VNRecognizedText *candidate = [[observation topCandidates:1] firstObject];
            if (!candidate || !candidate.string.length) { continue; }
            CGRect bounds = observation.boundingBox;
            [lines addObject:@{
                @"text": candidate.string,
                @"x": @(bounds.origin.x * bitmap.pixelsWide),
                @"y": @((1 - bounds.origin.y - bounds.size.height) * bitmap.pixelsHigh),
                @"width": @(bounds.size.width * bitmap.pixelsWide),
                @"height": @(bounds.size.height * bitmap.pixelsHigh),
                @"confidence": @(candidate.confidence)
            }];
        }
        NSData *json = [NSJSONSerialization dataWithJSONObject:lines options:0 error:nil];
        return json ? strndup(json.bytes, json.length) : NULL;
    }
}

void st_activate_app(void) {
    @autoreleasepool {
        dispatch_async(dispatch_get_main_queue(), ^{
            [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
            [NSApp activateIgnoringOtherApps:YES];
        });
    }
}

void st_hide_dock_icon(void) {
    @autoreleasepool {
        dispatch_async(dispatch_get_main_queue(), ^{
            [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        });
    }
}

void st_focus_app(void) {
    @autoreleasepool {
        dispatch_async(dispatch_get_main_queue(), ^{
            [NSApp activateIgnoringOtherApps:YES];
        });
    }
}

bool st_mouse_button_down(void) {
    @autoreleasepool {
        return ([NSEvent pressedMouseButtons] & 1) != 0;
    }
}

void st_attach_child_window(void *parentPtr, void *childPtr) {
    if (!parentPtr || !childPtr) return;
    dispatch_async(dispatch_get_main_queue(), ^{
        @autoreleasepool {
            NSWindow *parent = (__bridge NSWindow *)parentPtr;
            NSWindow *child = (__bridge NSWindow *)childPtr;
            if (child.parentWindow != parent) {
                [parent addChildWindow:child ordered:NSWindowAbove];
            }
        }
    });
}

bool st_system_dark(void) {
    @autoreleasepool {
        NSString *style = [[NSUserDefaults standardUserDefaults] stringForKey:@"AppleInterfaceStyle"];
        return [style isEqualToString:@"Dark"];
    }
}

void st_configure_window(void *windowPtr, int themeMode) {
    if (!windowPtr) return;
    dispatch_async(dispatch_get_main_queue(), ^{
        @autoreleasepool {
            NSWindow *window = (__bridge NSWindow *)windowPtr;
            BOOL isDark = NO;
            if (themeMode == 1) {
                isDark = YES;
            } else if (themeMode == 2) {
                isDark = NO;
            } else {
                isDark = st_system_dark();
            }
            [window setTitlebarAppearsTransparent:YES];
            [window setTitleVisibility:NSWindowTitleHidden];
            if (@available(macOS 11.0, *)) {
                [window setTitlebarSeparatorStyle:NSTitlebarSeparatorStyleNone];
            }
            NSColor *bg = isDark ? [NSColor colorWithSRGBRed:0x1d/255.0 green:0x1c/255.0 blue:0x20/255.0 alpha:1.0] : [NSColor colorWithSRGBRed:0xf0/255.0 green:0xee/255.0 blue:0xf1/255.0 alpha:1.0];
            [window setBackgroundColor:bg];
        }
    });
}
