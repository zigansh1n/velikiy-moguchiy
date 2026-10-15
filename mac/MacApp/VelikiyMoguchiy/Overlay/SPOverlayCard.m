#import "SPOverlayCard.h"
#import "SPOverlayLayout.h"
#import "SPOverlayPalette.h"

static const CGFloat kCardRadius = 10.0;
static const CGFloat kCardInset = 14.0;
static const CGFloat kTitleHeight = 16.0;
static const CGFloat kTitleGap = 6.0;
static const CGFloat kButtonGap = 8.0;
static const CGFloat kButtonHeight = 22.0;
static const CGFloat kTextFontSize = 13.0;
static const NSTimeInterval kFadeDuration = 0.15;
// The pointer resting on the card keeps it open this much longer each time.
static const NSTimeInterval kHoverExtension = 4.0;

/// Takes the first click of a window that is never key.
@interface SPOverlayCardButton : NSButton
@end

@implementation SPOverlayCardButton
- (BOOL)acceptsFirstMouse:(NSEvent *)event { return YES; }
@end

/// A click anywhere but the button closes the card.
@interface SPOverlayCardTextView : NSTextView
@property (nonatomic, copy) void (^clickHandler)(void);
@end

@implementation SPOverlayCardTextView
- (BOOL)acceptsFirstMouse:(NSEvent *)event { return YES; }
- (void)mouseDown:(NSEvent *)event {
    if (self.clickHandler) self.clickHandler();
}
@end

@interface SPOverlayCardView : NSView
@property (nonatomic, copy) void (^clickHandler)(void);
@end

@implementation SPOverlayCardView
- (BOOL)isFlipped { return YES; }
- (BOOL)acceptsFirstMouse:(NSEvent *)event { return YES; }
- (void)mouseDown:(NSEvent *)event {
    if (self.clickHandler) self.clickHandler();
}
- (void)drawRect:(NSRect)dirtyRect {
    SPOverlayDrawPlate(self.bounds, kCardRadius);
}
@end

@interface SPOverlayCard ()
@property (nonatomic, strong) NSPanel *panel;
@property (nonatomic, strong) SPOverlayCardView *cardView;
@property (nonatomic, strong) NSTextField *titleLabel;
@property (nonatomic, strong) NSScrollView *scrollView;
@property (nonatomic, strong) SPOverlayCardTextView *textView;
@property (nonatomic, strong) SPOverlayCardButton *actionButton;
@property (nonatomic, strong) NSTimer *closeTimer;
@property (nonatomic, copy) NSString *text;
@property (nonatomic, assign, readwrite, getter=isVisible) BOOL visible;
@end

@implementation SPOverlayCard

- (instancetype)init {
    self = [super init];
    if (self) [self setupPanel];
    return self;
}

- (void)setupPanel {
    NSRect rect = NSMakeRect(0, 0, SPOverlayLayout.cardWidth, 120);
    NSPanel *panel = [[NSPanel alloc] initWithContentRect:rect
                                                styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                                                  backing:NSBackingStoreBuffered
                                                    defer:YES];
    panel.level = NSStatusWindowLevel;
    panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                               NSWindowCollectionBehaviorStationary |
                               NSWindowCollectionBehaviorFullScreenAuxiliary;
    panel.backgroundColor = NSColor.clearColor;
    panel.opaque = NO;
    panel.hasShadow = YES;
    panel.hidesOnDeactivate = NO;
    panel.becomesKeyOnlyIfNeeded = YES;
    panel.alphaValue = 0.0;
    panel.appearance = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];

    __weak typeof(self) weakSelf = self;
    SPOverlayCardView *cardView = [[SPOverlayCardView alloc] initWithFrame:rect];
    cardView.clickHandler = ^{ [weakSelf close]; };
    cardView.accessibilityRole = NSAccessibilityGroupRole;
    panel.contentView = cardView;

    NSTextField *title = [NSTextField labelWithString:@""];
    title.font = [NSFont systemFontOfSize:11.0 weight:NSFontWeightSemibold];
    title.textColor = SPOverlayOchreColor();
    title.lineBreakMode = NSLineBreakByTruncatingTail;
    [cardView addSubview:title];

    NSScrollView *scroll = [[NSScrollView alloc] initWithFrame:NSZeroRect];
    scroll.drawsBackground = NO;
    scroll.hasVerticalScroller = YES;
    scroll.autohidesScrollers = YES;
    scroll.scrollerStyle = NSScrollerStyleOverlay;
    scroll.borderType = NSNoBorder;

    SPOverlayCardTextView *textView = [[SPOverlayCardTextView alloc] initWithFrame:NSZeroRect];
    textView.editable = NO;
    textView.selectable = NO;
    textView.drawsBackground = NO;
    textView.textContainerInset = NSZeroSize;
    textView.textContainer.lineFragmentPadding = 0;
    textView.textContainer.widthTracksTextView = YES;
    textView.verticallyResizable = YES;
    textView.maxSize = NSMakeSize(CGFLOAT_MAX, CGFLOAT_MAX);
    textView.horizontallyResizable = NO;
    textView.autoresizingMask = NSViewWidthSizable;
    textView.clickHandler = ^{ [weakSelf close]; };
    scroll.documentView = textView;
    [cardView addSubview:scroll];

    SPOverlayCardButton *button = [[SPOverlayCardButton alloc] initWithFrame:NSZeroRect];
    button.bordered = NO;
    button.attributedTitle = [[NSAttributedString alloc]
        initWithString:@"Копировать"
            attributes:@{
                NSFontAttributeName : [NSFont systemFontOfSize:12.0 weight:NSFontWeightMedium],
                NSForegroundColorAttributeName : SPOverlayOchreColor(),
            }];
    button.target = self;
    button.action = @selector(copyPressed:);
    [button sizeToFit];
    [cardView addSubview:button];

    self.panel = panel;
    self.cardView = cardView;
    self.titleLabel = title;
    self.scrollView = scroll;
    self.textView = textView;
    self.actionButton = button;
}

- (NSAttributedString *)attributedText:(NSString *)text {
    NSMutableParagraphStyle *paragraph = [[NSMutableParagraphStyle alloc] init];
    paragraph.lineSpacing = 2.0;
    return [[NSAttributedString alloc] initWithString:text
                                           attributes:@{
        NSFontAttributeName : [NSFont systemFontOfSize:kTextFontSize],
        NSForegroundColorAttributeName : SPOverlayInkColor(),
        NSParagraphStyleAttributeName : paragraph,
    }];
}

- (CGFloat)chromeHeightCopyable:(BOOL)copyable {
    return kCardInset + kTitleHeight + kTitleGap + (copyable ? kButtonGap + kButtonHeight : 0.0) + kCardInset;
}

- (void)showTitle:(NSString *)title
              text:(NSString *)text
          copyable:(BOOL)copyable
    frameForHeight:(NSRect (^)(CGFloat contentHeight))frameForHeight {
    self.text = text;
    NSAttributedString *body = [self attributedText:text];
    CGFloat textWidth = SPOverlayLayout.cardWidth - 2 * kCardInset;
    CGFloat textHeight = ceil([body boundingRectWithSize:NSMakeSize(textWidth, CGFLOAT_MAX)
                                                 options:NSStringDrawingUsesLineFragmentOrigin |
                                                         NSStringDrawingUsesFontLeading].size.height);
    CGFloat chrome = [self chromeHeightCopyable:copyable];
    NSRect frame = frameForHeight(chrome + textHeight);

    self.titleLabel.stringValue = title;
    [self.textView.textStorage setAttributedString:body];
    self.actionButton.hidden = !copyable;
    [self layoutForFrame:frame copyable:copyable];
    [self.scrollView.contentView scrollToPoint:NSZeroPoint];
    [self.scrollView reflectScrolledClipView:self.scrollView.contentView];
    self.cardView.accessibilityLabel = title;

    [self.panel setFrame:frame display:YES];
    if (NSApp.isHidden) [NSApp unhideWithoutActivation];
    [self.panel orderFrontRegardless];
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *ctx) {
        ctx.duration = kFadeDuration;
        self.panel.animator.alphaValue = 1.0;
    }];
    self.visible = YES;
    [self scheduleCloseAfter:SPOverlayLayout.cardAutoCloseDelay];
}

- (void)layoutForFrame:(NSRect)frame copyable:(BOOL)copyable {
    CGFloat width = NSWidth(frame);
    CGFloat height = NSHeight(frame);
    self.cardView.frame = NSMakeRect(0, 0, width, height);
    self.titleLabel.frame = NSMakeRect(kCardInset, kCardInset, width - 2 * kCardInset, kTitleHeight);
    CGFloat textTop = kCardInset + kTitleHeight + kTitleGap;
    CGFloat textHeight = fmax(0.0, height - [self chromeHeightCopyable:copyable]);
    self.scrollView.frame = NSMakeRect(kCardInset, textTop, width - 2 * kCardInset, textHeight);
    self.textView.minSize = NSMakeSize(0, textHeight);
    self.textView.frame = NSMakeRect(0, 0, NSWidth(self.scrollView.frame), textHeight);
    [self.textView sizeToFit];
    if (copyable) {
        NSSize size = self.actionButton.frame.size;
        self.actionButton.frame = NSMakeRect(width - kCardInset - size.width,
                                           height - kCardInset - kButtonHeight,
                                           size.width,
                                           kButtonHeight);
    }
}

- (void)scheduleCloseAfter:(NSTimeInterval)delay {
    [self.closeTimer invalidate];
    __weak typeof(self) weakSelf = self;
    self.closeTimer = [NSTimer scheduledTimerWithTimeInterval:delay
                                                      repeats:NO
                                                        block:^(NSTimer *timer) {
        __strong typeof(weakSelf) strongSelf = weakSelf;
        if (!strongSelf) return;
        strongSelf.closeTimer = nil;
        if (NSPointInRect(NSEvent.mouseLocation, strongSelf.panel.frame)) {
            [strongSelf scheduleCloseAfter:kHoverExtension];
        } else {
            [strongSelf close];
        }
    }];
}

- (void)copyPressed:(id)sender {
    NSString *text = self.text;
    if (self.copyHandler && text.length > 0) self.copyHandler(text);
    [self close];
}

- (void)close {
    [self.closeTimer invalidate];
    self.closeTimer = nil;
    if (!self.visible) return;
    self.visible = NO;
    NSPanel *panel = self.panel;
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *ctx) {
        ctx.duration = kFadeDuration;
        panel.animator.alphaValue = 0.0;
    } completionHandler:^{
        if (!self.visible) [panel orderOut:nil];
    }];
    if (self.closeHandler) self.closeHandler();
}

- (void)dealloc {
    [_closeTimer invalidate];
}

@end
