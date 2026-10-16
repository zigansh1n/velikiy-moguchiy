#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

/// One model in «Модели распознавания»: title and badges, a few lines of
/// text, a progress row and buttons. Empty lines hide themselves.
@interface SPModelCardView : NSView

- (instancetype)initWithWidth:(CGFloat)width;

/// The model path, catalog id or repository the card shows.
@property (nonatomic, copy) NSString *key;

@property (nonatomic, readonly) NSTextField *titleLabel;
@property (nonatomic, readonly) NSTextField *bodyLabel;
@property (nonatomic, readonly) NSTextField *detailLabel;
@property (nonatomic, readonly) NSTextField *noteLabel;
@property (nonatomic, readonly) NSTextField *resultLabel;
@property (nonatomic, readonly) NSProgressIndicator *progressBar;
@property (nonatomic, readonly) NSTextField *progressLabel;
/// Pop-ups and links between the text and the buttons.
@property (nonatomic, readonly) NSStackView *extras;
@property (nonatomic, readonly) NSStackView *buttons;

- (void)addBadge:(NSString *)text color:(NSColor *)color;
- (void)setText:(nullable NSString *)text ofLabel:(NSTextField *)label;
- (void)setText:(nullable NSString *)text color:(NSColor *)color ofLabel:(NSTextField *)label;
/// Shows the bar at `fraction` (0…1) with `text` under it, or hides both for a negative fraction.
- (void)setProgress:(double)fraction text:(nullable NSString *)text;
- (NSButton *)addButton:(NSString *)title target:(id)target action:(SEL)action;
- (NSButton *)addLink:(NSString *)title target:(id)target action:(SEL)action;

@end

NS_ASSUME_NONNULL_END
