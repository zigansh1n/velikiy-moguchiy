#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

/// A card above the pill with a text to read: an Ask answer, a translation that
/// was not pasted, what an error means. Long text scrolls. It never takes the
/// keyboard focus; a click on it, -close (the app's Esc) or
/// SPOverlayLayout.cardAutoCloseDelay closes it.
@interface SPOverlayCard : NSObject

/// «Копировать» was pressed; the card closes right after.
@property (nonatomic, copy, nullable) void (^copyHandler)(NSString *text);
/// The card closed, for any reason.
@property (nonatomic, copy, nullable) void (^closeHandler)(void);
@property (nonatomic, readonly, getter=isVisible) BOOL visible;

/// `frameForHeight` places a card whose content is that tall.
- (void)showTitle:(NSString *)title
              text:(NSString *)text
          copyable:(BOOL)copyable
    frameForHeight:(NSRect (^)(CGFloat contentHeight))frameForHeight;
- (void)close;

@end

NS_ASSUME_NONNULL_END
