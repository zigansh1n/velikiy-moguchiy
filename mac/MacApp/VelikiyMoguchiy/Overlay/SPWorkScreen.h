#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

/// The display the user works on: the one holding most of the focused window
/// of the frontmost app, else the one under the pointer, else the first.
/// `screenFrames` are Cocoa frames (origin bottom-left of the primary display);
/// `focusedWindow` is in Accessibility coordinates (origin top-left of the
/// primary display, y down), NSZeroRect when there is none; `primaryHeight` is
/// the primary display's height, which links the two.
NSUInteger SPWorkScreenIndex(NSArray<NSValue *> *screenFrames,
                             CGFloat primaryHeight,
                             NSRect focusedWindow,
                             NSPoint mouseLocation);

@interface SPWorkScreen : NSObject

/// SPWorkScreenIndex for the screens, focused window and pointer right now.
+ (NSScreen *)currentScreen;

@end

NS_ASSUME_NONNULL_END
