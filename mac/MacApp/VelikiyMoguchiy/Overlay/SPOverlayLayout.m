#import "SPOverlayLayout.h"

static const CGFloat kScreenInset = 16.0;

@implementation SPOverlayLayout

+ (NSTimeInterval)cardAutoCloseDelay {
    return 20.0;
}

+ (CGFloat)cardWidth {
    return 380.0;
}

+ (CGFloat)cardMaxHeight {
    return 260.0;
}

+ (CGFloat)captionHeight {
    return 20.0;
}

+ (CGFloat)captionMaxWidth {
    return 420.0;
}

+ (CGFloat)gap {
    return 4.0;
}

+ (BOOL)captionGoesBelowPill:(NSRect)pill visibleFrame:(NSRect)visible {
    return NSMinY(pill) - self.gap - self.captionHeight >= NSMinY(visible);
}

/// x of a `width` wide box centred on `pill`, kept inside `visible`.
+ (CGFloat)originXForWidth:(CGFloat)width pill:(NSRect)pill visibleFrame:(NSRect)visible {
    CGFloat x = round(NSMidX(pill) - width / 2.0);
    CGFloat minX = NSMinX(visible) + kScreenInset;
    CGFloat maxX = NSMaxX(visible) - kScreenInset - width;
    return maxX < minX ? round(NSMidX(visible) - width / 2.0) : fmin(fmax(x, minX), maxX);
}

+ (NSRect)captionFrameWithWidth:(CGFloat)width pill:(NSRect)pill visibleFrame:(NSRect)visible {
    width = ceil(fmin(width, fmin(self.captionMaxWidth, NSWidth(visible) - 2 * kScreenInset)));
    CGFloat y = [self captionGoesBelowPill:pill visibleFrame:visible]
                    ? NSMinY(pill) - self.gap - self.captionHeight
                    : NSMaxY(pill) + self.gap;
    return NSMakeRect([self originXForWidth:width pill:pill visibleFrame:visible], y, width, self.captionHeight);
}

+ (NSRect)cardFrameWithContentHeight:(CGFloat)contentHeight
                                pill:(NSRect)pill
                        captionAbove:(BOOL)captionAbove
                        visibleFrame:(NSRect)visible {
    CGFloat width = fmin(self.cardWidth, NSWidth(visible) - 2 * kScreenInset);
    CGFloat bottom = NSMaxY(pill) + self.gap;
    if (captionAbove) bottom += self.captionHeight + self.gap;
    CGFloat room = NSMaxY(visible) - kScreenInset - bottom;
    CGFloat height = ceil(fmax(0.0, fmin(contentHeight, fmin(self.cardMaxHeight, room))));
    return NSMakeRect([self originXForWidth:width pill:pill visibleFrame:visible], bottom, width, height);
}

+ (NSString *)captionForBadgeText:(NSString *)badgeText {
    NSString *caption = badgeText;
    for (NSString *mark in @[ @"✓", @"↩︎", @"↩" ]) {
        if ([caption hasPrefix:mark]) {
            caption = [caption substringFromIndex:mark.length];
            break;
        }
    }
    return [caption stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceCharacterSet];
}

+ (SPOverlayBadgeIcon)iconForBadgeText:(NSString *)badgeText {
    if ([badgeText containsString:@"буфер"] || [badgeText containsString:@"Скопировано"]) {
        return SPOverlayBadgeIconClipboard;
    }
    return SPOverlayBadgeIconSuccess;
}

+ (NSTimeInterval)lingerForCaption:(NSString *)caption iconLinger:(NSTimeInterval)iconLinger {
    if (caption.length == 0) return iconLinger;
    NSTimeInterval reading = fmin(7.0, 1.2 + caption.length * 0.06);
    return fmax(iconLinger, reading);
}

@end
