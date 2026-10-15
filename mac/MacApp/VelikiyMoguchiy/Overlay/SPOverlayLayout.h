#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

typedef NS_ENUM(NSInteger, SPOverlayBadgeIcon) {
    /// The text is on the clipboard and not in the field.
    SPOverlayBadgeIconClipboard,
    /// The text reached the field (inserted, replaced, an insertion undone).
    SPOverlayBadgeIconSuccess,
};

/// Where the pill's caption, the result card and their timings go, apart from
/// AppKit so tests can pin them. Rects are screen coordinates, y going up.
/// The pill never widens: text goes into the caption next to it or the card above it.
@interface SPOverlayLayout : NSObject

/// The card closes by itself after this long, later while the pointer is on it.
@property (class, nonatomic, readonly) NSTimeInterval cardAutoCloseDelay;
@property (class, nonatomic, readonly) CGFloat cardWidth;
/// Taller text scrolls inside the card.
@property (class, nonatomic, readonly) CGFloat cardMaxHeight;
@property (class, nonatomic, readonly) CGFloat captionHeight;
@property (class, nonatomic, readonly) CGFloat captionMaxWidth;
/// Between the pill, its caption and the card.
@property (class, nonatomic, readonly) CGFloat gap;

/// Under the pill when it fits above the bottom of `visible`, else over it.
+ (BOOL)captionGoesBelowPill:(NSRect)pill visibleFrame:(NSRect)visible;
+ (NSRect)captionFrameWithWidth:(CGFloat)width pill:(NSRect)pill visibleFrame:(NSRect)visible;
/// Over the pill and over a caption that sits over it, centred on the pill,
/// inside `visible`. `contentHeight` is clipped to the room left and to cardMaxHeight.
+ (NSRect)cardFrameWithContentHeight:(CGFloat)contentHeight
                                pill:(NSRect)pill
                        captionAbove:(BOOL)captionAbove
                        visibleFrame:(NSRect)visible;

/// "✓ Скопировано" → "Скопировано": the pill draws the mark itself.
+ (NSString *)captionForBadgeText:(NSString *)badgeText;
+ (SPOverlayBadgeIcon)iconForBadgeText:(NSString *)badgeText;
/// How long a badge caption stays: never shorter than `iconLinger`, long enough to read.
+ (NSTimeInterval)lingerForCaption:(nullable NSString *)caption iconLinger:(NSTimeInterval)iconLinger;

@end

NS_ASSUME_NONNULL_END
