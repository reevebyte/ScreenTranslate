#import <AppKit/AppKit.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <Vision/Vision.h>
#import <CoreGraphics/CoreGraphics.h>
#include <stdlib.h>
#include <string.h>

// Small ABI bridge for Apple frameworks. All application state stays in Rust.
@interface STCaptureReply : NSObject
@property(nonatomic, strong) NSData *data;
@property(nonatomic, strong) NSError *error;
@end
@implementation STCaptureReply
@end

void st_free(void *pointer) { free(pointer); }

unsigned char *st_capture_display(uint32_t displayID, size_t *length, char **error) {
    @autoreleasepool {
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
            *error = strdup("capture_failed");
            return NULL;
        }
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
