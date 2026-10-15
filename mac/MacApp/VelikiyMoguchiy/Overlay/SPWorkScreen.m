#import "SPWorkScreen.h"
#import <ApplicationServices/ApplicationServices.h>

// A hung app must not hold up the pill: Accessibility gives up after this long.
static const float kFocusedWindowTimeout = 0.05f;

NSUInteger SPWorkScreenIndex(NSArray<NSValue *> *screenFrames,
                             CGFloat primaryHeight,
                             NSRect focusedWindow,
                             NSPoint mouseLocation) {
    if (screenFrames.count == 0) return 0;
    if (!NSIsEmptyRect(focusedWindow)) {
        NSRect window = focusedWindow;
        window.origin.y = primaryHeight - NSMaxY(focusedWindow);
        NSUInteger best = NSNotFound;
        CGFloat bestArea = 0.0;
        for (NSUInteger i = 0; i < screenFrames.count; i++) {
            NSRect overlap = NSIntersectionRect(window, screenFrames[i].rectValue);
            CGFloat area = NSWidth(overlap) * NSHeight(overlap);
            if (area > bestArea) {
                bestArea = area;
                best = i;
            }
        }
        if (best != NSNotFound) return best;
    }
    for (NSUInteger i = 0; i < screenFrames.count; i++) {
        if (NSMouseInRect(mouseLocation, screenFrames[i].rectValue, NO)) return i;
    }
    return 0;
}

@implementation SPWorkScreen

/// The frontmost app's focused window in Accessibility coordinates; NSZeroRect without one.
+ (NSRect)focusedWindowFrame {
    pid_t pid = NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier;
    if (pid <= 0 || pid == NSProcessInfo.processInfo.processIdentifier) return NSZeroRect;
    AXUIElementRef app = AXUIElementCreateApplication(pid);
    if (!app) return NSZeroRect;
    AXUIElementSetMessagingTimeout(app, kFocusedWindowTimeout);
    AXUIElementRef window = NULL;
    AXError error = AXUIElementCopyAttributeValue(app, kAXFocusedWindowAttribute, (CFTypeRef *)&window);
    CFRelease(app);
    if (error != kAXErrorSuccess || !window) return NSZeroRect;

    CGPoint origin = CGPointZero;
    CGSize size = CGSizeZero;
    CFTypeRef positionValue = NULL;
    CFTypeRef sizeValue = NULL;
    AXUIElementCopyAttributeValue(window, kAXPositionAttribute, &positionValue);
    AXUIElementCopyAttributeValue(window, kAXSizeAttribute, &sizeValue);
    CFRelease(window);
    BOOL ok = positionValue && sizeValue &&
              CFGetTypeID(positionValue) == AXValueGetTypeID() && CFGetTypeID(sizeValue) == AXValueGetTypeID() &&
              AXValueGetValue((AXValueRef)positionValue, kAXValueTypeCGPoint, &origin) &&
              AXValueGetValue((AXValueRef)sizeValue, kAXValueTypeCGSize, &size);
    if (positionValue) CFRelease(positionValue);
    if (sizeValue) CFRelease(sizeValue);
    return ok ? NSMakeRect(origin.x, origin.y, size.width, size.height) : NSZeroRect;
}

+ (NSScreen *)currentScreen {
    NSArray<NSScreen *> *screens = NSScreen.screens;
    if (screens.count <= 1) return screens.firstObject ?: NSScreen.mainScreen;
    NSMutableArray<NSValue *> *frames = [NSMutableArray arrayWithCapacity:screens.count];
    for (NSScreen *screen in screens) [frames addObject:[NSValue valueWithRect:screen.frame]];
    // screens[0] is the display with the menu bar, whose top-left is the Accessibility origin.
    NSUInteger index = SPWorkScreenIndex(frames, NSHeight(screens.firstObject.frame),
                                         [self focusedWindowFrame], NSEvent.mouseLocation);
    return screens[index];
}

@end
