#import "SPOverlayPanel.h"
#import "SPOverlayCard.h"
#import "SPOverlayLayout.h"
#import "SPOverlayPalette.h"
#import "SPWorkScreen.h"
#import "SPTheme.h"
#import "velikiy_moguchiy_core.h"
#import <QuartzCore/QuartzCore.h>

// ── Geometry ──────────────────────────────────────────────
static const CGFloat kPillWidth  = 76.0;
static const CGFloat kPillHeight = 30.0;
static const CGFloat kDefaultBottomMargin = 10.0;
static const CGFloat kMaxBottomMargin = 180.0;
// The caption next to the pill: one line of text on a small plate.
static const CGFloat kCaptionFontSize = 12.0;
static const CGFloat kCaptionInsetX = 10.0;

// Waveform bars
static const NSInteger kBarCount   = 6;
static const CGFloat   kBarWidth   = 3.0;
static const CGFloat   kBarSpacing = 3.0;
static const CGFloat   kBarMinH    = 3.0;
static const CGFloat   kBarMaxH    = 18.0;
static const NSInteger kMicLevelBuckets = 16;

// Processing: gilt lozenges like the clasp studs, lit one after another
static const NSInteger kLozengeCount   = 3;
static const CGFloat   kLozengeHalfW   = 2.6;
static const CGFloat   kLozengeHalfH   = 3.6;
static const CGFloat   kLozengeSpacing = 9.0;
static const NSTimeInterval kLozengePulsePeriod = 1.4;

// Animation
static const NSTimeInterval kAnimInterval      = 1.0 / 30.0;
static const NSTimeInterval kFadeInDuration    = 0.2;
static const NSTimeInterval kFadeOutDuration   = 0.3;
static const NSTimeInterval kPanelEntranceDuration = 0.22;
static const NSTimeInterval kPanelExitDuration = 0.18;
static const CGFloat kPanelEntranceLift = 10.0;
static const CGFloat kPanelExitDrop = 8.0;
static const CGFloat kBaseShadowOpacity = 0.11;
static const CGFloat kBaseShadowRadius = 4.0;
static const CGFloat kEntranceShadowOpacity = 0.17;
static const CGFloat kEntranceShadowRadius = 8.0;
static const NSTimeInterval kShakeDuration = 0.36;
static const CGFloat kShakeOffset = 5.0;

// How long each result icon stays before the pill hides
static const NSTimeInterval kSuccessLingerDuration   = 0.6;
static const NSTimeInterval kClipboardLingerDuration = 1.2;
static const NSTimeInterval kCancelledLingerDuration = 0.6;
static const NSTimeInterval kErrorLingerDuration     = 1.5;
static const NSTimeInterval kModeLetterLingerDuration = 1.2;
// A message stays long enough to be read: a base plus a little per character.
static const NSTimeInterval kMessageMinLinger = 3.0;
static const NSTimeInterval kMessageMaxLinger = 7.0;
static const NSTimeInterval kMessageLingerPerChar = 0.06;

static CGFloat SPOverlayClampBottomMargin(CGFloat bottomMargin) {
    return fmin(fmax(bottomMargin, 0.0), kMaxBottomMargin);
}

static CGFloat SPOverlayConfigCGFloat(const char *keyPath, CGFloat fallback) {
    char *raw = sp_config_get(keyPath);
    if (!raw) return fallback;

    NSString *value = [NSString stringWithUTF8String:raw];
    sp_core_free_string(raw);
    if (value.length == 0) return fallback;

    NSScanner *scanner = [NSScanner scannerWithString:value];
    double parsedValue = 0.0;
    if (![scanner scanDouble:&parsedValue] || !scanner.isAtEnd) {
        return fallback;
    }

    return parsedValue;
}

static BOOL SPOverlayShouldReduceMotion(void) {
    if (@available(macOS 10.15, *)) {
        return NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion;
    }
    return NO;
}

static NSRect SPOverlayPresentationStartFrame(NSRect finalFrame) {
    CGFloat insetX = fmin(10.0, floor(finalFrame.size.width * 0.018));
    CGFloat insetY = fmin(6.0, floor(finalFrame.size.height * 0.08));
    NSRect startFrame = NSInsetRect(finalFrame, insetX, insetY);
    startFrame.origin.y -= kPanelEntranceLift;
    return startFrame;
}

static NSRect SPOverlayDismissalEndFrame(NSRect currentFrame) {
    CGFloat insetX = fmin(8.0, floor(currentFrame.size.width * 0.012));
    CGFloat insetY = fmin(5.0, floor(currentFrame.size.height * 0.06));
    NSRect endFrame = NSInsetRect(currentFrame, insetX, insetY);
    endFrame.origin.y -= kPanelExitDrop;
    return endFrame;
}

// ── Animation mode ───────────────────────────────────────
typedef NS_ENUM(NSInteger, SPOverlayMode) {
    SPOverlayModeNone,
    SPOverlayModeWaveform,
    SPOverlayModeProcessing,
    SPOverlayModeSuccess,
    SPOverlayModeClipboard,
    SPOverlayModeCancelled,
    SPOverlayModeError,
    SPOverlayModeLetter,
    SPOverlayModeNotice,
};

static NSAttributedString *SPOverlayCaptionString(NSString *text) {
    NSMutableParagraphStyle *paragraph = [[NSMutableParagraphStyle alloc] init];
    paragraph.lineBreakMode = NSLineBreakByTruncatingTail;
    paragraph.alignment = NSTextAlignmentCenter;
    return [[NSAttributedString alloc] initWithString:text
                                           attributes:@{
        NSFontAttributeName : [NSFont systemFontOfSize:kCaptionFontSize weight:NSFontWeightMedium],
        NSForegroundColorAttributeName : SPOverlayInkColor(),
        NSParagraphStyleAttributeName : paragraph,
    }];
}

// ── Click-aware effect view ──────────────────────────────

@interface SPClickEffectView : NSVisualEffectView
@property (nonatomic, copy) void (^clickHandler)(void);
@end

@implementation SPClickEffectView

- (void)mouseDown:(NSEvent *)event {
    if (self.clickHandler) {
        self.clickHandler();
        return;
    }
    [super mouseDown:event];
}

@end

// ── Pill content ─────────────────────────────────────────

@interface SPOverlayContentView : NSView
@property (nonatomic, assign) SPOverlayMode mode;
@property (nonatomic, assign) NSInteger     tick;  // animation counter
@property (nonatomic, copy)   NSString     *letter; // SPOverlayModeLetter
@property (nonatomic, copy)   NSString     *symbolName; // SPOverlayModeNotice
/// Zero the smoothed bar levels so a new recording does not start from the
/// previous one's tail.
- (void)resetLevels;
@end

@implementation SPOverlayContentView {
    float _smoothedLevels[16];
}

- (BOOL)isFlipped { return NO; }

- (void)resetLevels {
    memset(_smoothedLevels, 0, sizeof(_smoothedLevels));
}

- (void)drawRect:(NSRect)dirtyRect {
    NSRect bounds = self.bounds;

    [SPOverlaySurfaceTintColor() setFill];
    [NSBezierPath fillRect:bounds];

    CGFloat radius = self.layer.cornerRadius;
    NSBezierPath *rule = [NSBezierPath bezierPathWithRoundedRect:NSInsetRect(bounds, 1.5, 1.5)
                                                         xRadius:MAX(radius - 1.5, 0)
                                                         yRadius:MAX(radius - 1.5, 0)];
    rule.lineWidth = 1.2;
    [SPOverlayOchreColor() setStroke];
    [rule stroke];
    NSBezierPath *hair = [NSBezierPath bezierPathWithRoundedRect:NSInsetRect(bounds, 3.6, 3.6)
                                                         xRadius:MAX(radius - 3.6, 0)
                                                         yRadius:MAX(radius - 3.6, 0)];
    hair.lineWidth = 0.6;
    [[SPOverlayCinnabarColor() colorWithAlphaComponent:0.8] setStroke];
    [hair stroke];
    // gilded studs at the two ends, like the clasps of a book
    [SPOverlayOchreColor() setFill];
    CGFloat studY = NSMidY(bounds);
    for (int end = 0; end < 2; end++) {
        CGFloat cx = end ? NSMaxX(bounds) - 7.5 : NSMinX(bounds) + 7.5;
        NSBezierPath *stud = [NSBezierPath bezierPath];
        [stud moveToPoint:NSMakePoint(cx, studY + 3.2)];
        [stud lineToPoint:NSMakePoint(cx + 2.4, studY)];
        [stud lineToPoint:NSMakePoint(cx, studY - 3.2)];
        [stud lineToPoint:NSMakePoint(cx - 2.4, studY)];
        [stud closePath];
        [stud fill];
    }

    CGFloat centerX = NSMidX(bounds);
    CGFloat centerY = NSMidY(bounds);

    switch (self.mode) {
        case SPOverlayModeWaveform:
            [self drawWaveformAtX:centerX centerY:centerY];
            break;
        case SPOverlayModeProcessing:
            [self drawLozengesAtX:centerX centerY:centerY];
            break;
        case SPOverlayModeSuccess:
            [self drawCheckmarkAtX:centerX centerY:centerY];
            break;
        case SPOverlayModeClipboard:
            [self drawClipboardAtX:centerX centerY:centerY];
            break;
        case SPOverlayModeCancelled:
            [self drawCrossAtX:centerX centerY:centerY color:SPOverlayInkColor()];
            break;
        case SPOverlayModeError:
            [self drawCrossAtX:centerX centerY:centerY
                         color:SPOverlayCinnabarColor()];
            break;
        case SPOverlayModeLetter:
            [self drawLetterAtX:centerX centerY:centerY];
            break;
        case SPOverlayModeNotice:
            [self drawSymbol:self.symbolName atX:centerX centerY:centerY color:SPOverlayOchreColor()];
            break;
        default:
            break;
    }
}

#pragma mark - Waveform (recording)

- (void)drawWaveformAtX:(CGFloat)centerX centerY:(CGFloat)centerY {
    CGFloat totalW = kBarCount * kBarWidth + (kBarCount - 1) * kBarSpacing;
    CGFloat startX = centerX - totalW / 2.0;

    // Exponential smoothing and height curve from Handy's RecordingOverlay.tsx.
    float levels[kMicLevelBuckets] = {0};
    sp_core_get_mic_levels(levels, kMicLevelBuckets);
    for (NSInteger i = 0; i < kBarCount; i++) {
        _smoothedLevels[i] = _smoothedLevels[i] * 0.7f + levels[i] * 0.3f;
    }

    for (NSInteger i = 0; i < kBarCount; i++) {
        // Ochre at the edges, parchment toward the middle, like gilt on a page.
        CGFloat edge = fabs((CGFloat)i - (kBarCount - 1) / 2.0) / ((kBarCount - 1) / 2.0);
        [[SPOverlayInkColor() blendedColorWithFraction:edge * 0.8 ofColor:SPOverlayOchreColor()] setFill];
        CGFloat t = (CGFloat)powf(_smoothedLevels[i], 0.7f);
        CGFloat h = fmin(kBarMaxH, kBarMinH + t * (kBarMaxH - kBarMinH));
        CGFloat x = startX + i * (kBarWidth + kBarSpacing);
        CGFloat y = centerY - h / 2.0;
        NSBezierPath *bar = [NSBezierPath bezierPathWithRoundedRect:NSMakeRect(x, y, kBarWidth, h)
                                                             xRadius:kBarWidth / 2.0
                                                             yRadius:kBarWidth / 2.0];
        [bar fill];
    }
}

#pragma mark - Processing lozenges

- (void)drawLozengesAtX:(CGFloat)centerX centerY:(CGFloat)centerY {
    CGFloat startX = centerX - (kLozengeCount - 1) * kLozengeSpacing / 2.0;
    double seconds = (double)self.tick * kAnimInterval;
    BOOL still = SPOverlayShouldReduceMotion();

    for (NSInteger i = 0; i < kLozengeCount; i++) {
        double phase = seconds * 2.0 * M_PI / kLozengePulsePeriod - (double)i * 0.9;
        CGFloat glow = (CGFloat)(0.5 + 0.5 * sin(phase));
        // Reduce Motion keeps the glow but not the swelling.
        CGFloat scale = still ? 1.0 : 0.8 + 0.3 * glow;
        CGFloat halfW = kLozengeHalfW * scale;
        CGFloat halfH = kLozengeHalfH * scale;
        NSColor *gilt = [SPOverlayOchreColor() blendedColorWithFraction:glow * 0.35 ofColor:SPOverlayInkColor()];
        [[gilt colorWithAlphaComponent:0.3 + 0.7 * glow] setFill];

        CGFloat x = startX + i * kLozengeSpacing;
        NSBezierPath *lozenge = [NSBezierPath bezierPath];
        [lozenge moveToPoint:NSMakePoint(x, centerY + halfH)];
        [lozenge lineToPoint:NSMakePoint(x + halfW, centerY)];
        [lozenge lineToPoint:NSMakePoint(x, centerY - halfH)];
        [lozenge lineToPoint:NSMakePoint(x - halfW, centerY)];
        [lozenge closePath];
        [lozenge fill];
    }
}

#pragma mark - Checkmark (delivered)

- (void)drawCheckmarkAtX:(CGFloat)centerX centerY:(CGFloat)centerY {
    CGFloat progress = fmin(1.0, (CGFloat)self.tick / 6.0);

    NSPoint p0 = NSMakePoint(centerX - 6, centerY + 1);
    NSPoint p1 = NSMakePoint(centerX - 1.5, centerY - 4);
    NSPoint p2 = NSMakePoint(centerX + 7, centerY + 5);

    NSBezierPath *path = [NSBezierPath bezierPath];
    path.lineWidth = 2.0;
    path.lineCapStyle = NSLineCapStyleRound;
    path.lineJoinStyle = NSLineJoinStyleRound;

    if (progress <= 0.4) {
        CGFloat t = progress / 0.4;
        NSPoint end = NSMakePoint(p0.x + (p1.x - p0.x) * t, p0.y + (p1.y - p0.y) * t);
        [path moveToPoint:p0];
        [path lineToPoint:end];
    } else {
        CGFloat t = (progress - 0.4) / 0.6;
        NSPoint end = NSMakePoint(p1.x + (p2.x - p1.x) * t, p1.y + (p2.y - p1.y) * t);
        [path moveToPoint:p0];
        [path lineToPoint:p1];
        [path lineToPoint:end];
    }

    [SPOverlayInkColor() setStroke];
    [path stroke];
}

#pragma mark - Clipboard (copied, not pasted)

- (void)drawClipboardAtX:(CGFloat)centerX centerY:(CGFloat)centerY {
    if (![self drawSymbol:@"doc.on.clipboard" atX:centerX centerY:centerY color:SPOverlayInkColor()]) {
        NSRect board = NSMakeRect(centerX - 6, centerY - 8, 12, 16);
        NSBezierPath *outline = [NSBezierPath bezierPathWithRoundedRect:board xRadius:2.5 yRadius:2.5];
        outline.lineWidth = 1.8;
        [SPOverlayInkColor() setStroke];
        [outline stroke];
    }
}

/// Draws an SF Symbol centred on the point; NO when the system has no such symbol.
- (BOOL)drawSymbol:(NSString *)name atX:(CGFloat)centerX centerY:(CGFloat)centerY color:(NSColor *)color {
    NSImage *symbol = nil;
    if (@available(macOS 11.0, *)) {
        if (name.length > 0) symbol = [NSImage imageWithSystemSymbolName:name accessibilityDescription:nil];
        symbol = [symbol imageWithSymbolConfiguration:
                     [NSImageSymbolConfiguration configurationWithPointSize:15.0
                                                                     weight:NSFontWeightMedium]];
    }
    if (!symbol) return NO;

    // Symbols are template images; fill them with the ink instead of the default black.
    NSImage *tinted = [NSImage imageWithSize:symbol.size
                                     flipped:NO
                              drawingHandler:^BOOL(NSRect dstRect) {
        [symbol drawInRect:dstRect];
        [color set];
        NSRectFillUsingOperation(dstRect, NSCompositingOperationSourceAtop);
        return YES;
    }];
    [tinted drawAtPoint:NSMakePoint(centerX - tinted.size.width / 2.0, centerY - tinted.size.height / 2.0)
               fromRect:NSZeroRect
              operation:NSCompositingOperationSourceOver
               fraction:1.0];
    return YES;
}

#pragma mark - Letter (text mode)

- (void)drawLetterAtX:(CGFloat)centerX centerY:(CGFloat)centerY {
    if (self.letter.length == 0) return;
    NSDictionary *attributes = @{
        NSFontAttributeName : [SPTheme bookHandFontOfSize:20.0],
        NSForegroundColorAttributeName : SPOverlayInkColor(),
    };
    NSSize size = [self.letter sizeWithAttributes:attributes];
    [self.letter drawAtPoint:NSMakePoint(centerX - size.width / 2.0, centerY - size.height / 2.0)
              withAttributes:attributes];
}

#pragma mark - Cross (cancelled / error)

- (void)drawCrossAtX:(CGFloat)centerX centerY:(CGFloat)centerY color:(NSColor *)color {
    CGFloat arm = 5.0;

    NSBezierPath *path = [NSBezierPath bezierPath];
    path.lineWidth = 2.0;
    path.lineCapStyle = NSLineCapStyleRound;

    [path moveToPoint:NSMakePoint(centerX - arm, centerY - arm)];
    [path lineToPoint:NSMakePoint(centerX + arm, centerY + arm)];
    [path moveToPoint:NSMakePoint(centerX + arm, centerY - arm)];
    [path lineToPoint:NSMakePoint(centerX - arm, centerY + arm)];

    [[color colorWithAlphaComponent:0.95] setStroke];
    [path stroke];
}

@end

// ── Caption ──────────────────────────────────────────────

@interface SPOverlayCaptionView : NSView
@property (nonatomic, copy) NSString *text;
@end

@implementation SPOverlayCaptionView

- (void)drawRect:(NSRect)dirtyRect {
    NSRect bounds = self.bounds;
    SPOverlayDrawPlate(bounds, NSHeight(bounds) / 2.0);
    NSAttributedString *line = SPOverlayCaptionString(self.text ?: @"");
    CGFloat lineHeight = ceil(line.size.height);
    NSRect textRect = NSMakeRect(kCaptionInsetX, floor(NSMidY(bounds) - lineHeight / 2.0),
                                 NSWidth(bounds) - 2 * kCaptionInsetX, lineHeight);
    [line drawWithRect:textRect
               options:NSStringDrawingUsesLineFragmentOrigin | NSStringDrawingTruncatesLastVisibleLine];
}

@end

// ── Main overlay controller ──────────────────────────────

@interface SPOverlayPanel ()

@property (nonatomic, strong) NSPanel *panel;
@property (nonatomic, strong) SPClickEffectView *effectView;
@property (nonatomic, strong) SPOverlayContentView *contentView;
@property (nonatomic, strong) NSTimer *animationTimer;
@property (nonatomic, strong) NSTimer *lingerTimer;
@property (nonatomic, copy)   NSString *currentState;
@property (nonatomic, assign) CGFloat bottomMargin;
@property (nonatomic, assign) CGFloat configuredBottomMargin;
@property (nonatomic, assign, getter=isPreviewActive) BOOL previewActive;
@property (nonatomic, assign) BOOL rawAsrFallbackClickEnabled;
@property (nonatomic, strong) NSPanel *captionPanel;
@property (nonatomic, strong) SPOverlayCaptionView *captionView;
/// The line in the caption; nil when there is none.
@property (nonatomic, copy)   NSString *caption;
/// A result caption ("Скопировано", "Отменено") lingers with its icon, not like a message.
@property (nonatomic, assign) BOOL captionIsBadge;
@property (nonatomic, strong) SPOverlayCard *card;
@property (nonatomic, strong) NSScreen *workScreen;

@end

@implementation SPOverlayPanel

- (instancetype)init {
    self = [super init];
    if (self) {
        _currentState = @"idle";
        _bottomMargin = kDefaultBottomMargin;
        _configuredBottomMargin = kDefaultBottomMargin;
        [self setupPanel];
        [self setupCaptionPanel];
        [self setupCard];
        [self reloadAppearanceFromConfig];
        [[NSWorkspace sharedWorkspace].notificationCenter
            addObserver:self
               selector:@selector(activeSpaceDidChange:)
                   name:NSWorkspaceActiveSpaceDidChangeNotification
                 object:nil];
    }
    return self;
}

#pragma mark - Spaces

/// Order a panel in on the Space the user is looking at.
///
/// The panel is CanJoinAllSpaces, but when a Space switch happens while it
/// is on screen (e.g. lingering after a paste, then the user swipes to a
/// full-screen app), WindowServer can leave it pinned to the previous Space.
/// orderFrontRegardless alone keeps it there — ordered in, opaque, and
/// invisible — until the user switches windows. Ordering it out first makes
/// the next order-in attach it to the active Space again.
static void SPOrderFrontOnActiveSpace(NSWindow *window) {
    if (!window) return;
    // A hidden app hides every window, this panel included, and a menu bar app
    // has no Dock icon to unhide it from: launched with `open -j`, or hidden
    // with ⌘H from the Settings window, it would dictate with no pill at all.
    if (NSApp.isHidden) {
        [NSApp unhideWithoutActivation];
    }
    if (window.isVisible && !window.isOnActiveSpace) {
        NSLog(@"[VelikiyMoguchiy] Overlay window was stranded on another Space; re-attaching to the active Space");
        [window orderOut:nil];
    }
    [window orderFrontRegardless];
}

- (void)activeSpaceDidChange:(NSNotification *)notification {
    if (self.panel.isVisible && self.panel.alphaValue > 0.01) {
        SPOrderFrontOnActiveSpace(self.panel);
        if (self.caption.length > 0) SPOrderFrontOnActiveSpace(self.captionPanel);
    }
}

- (CGFloat)overlayCornerRadius {
    return kPillHeight / 2.0;
}

- (void)resetMotionPresentationState {
    [self.contentView.layer removeAnimationForKey:@"overlayContentEntrance"];
    [self.contentView.layer removeAnimationForKey:@"overlayContentExit"];
    [self.effectView.layer removeAnimationForKey:@"overlayShadowEntrance"];
    [self.effectView.layer removeAnimationForKey:@"overlayShadowExit"];
    self.contentView.layer.transform = CATransform3DIdentity;
    self.contentView.layer.opacity = 1.0;
    self.effectView.layer.shadowOpacity = kBaseShadowOpacity;
    self.effectView.layer.shadowRadius = kBaseShadowRadius;
}

- (void)animateContentLayerFromTransform:(CATransform3D)fromTransform
                             toTransform:(CATransform3D)toTransform
                             fromOpacity:(CGFloat)fromOpacity
                               toOpacity:(CGFloat)toOpacity
                                duration:(CFTimeInterval)duration
                               animation:(NSString *)animationKey {
    if (!self.contentView.layer) return;

    [self.contentView.layer removeAnimationForKey:animationKey];
    self.contentView.layer.transform = toTransform;
    self.contentView.layer.opacity = toOpacity;

    CABasicAnimation *transformAnimation = [CABasicAnimation animationWithKeyPath:@"transform"];
    transformAnimation.fromValue = [NSValue valueWithCATransform3D:fromTransform];
    transformAnimation.toValue = [NSValue valueWithCATransform3D:toTransform];

    CABasicAnimation *opacityAnimation = [CABasicAnimation animationWithKeyPath:@"opacity"];
    opacityAnimation.fromValue = @(fromOpacity);
    opacityAnimation.toValue = @(toOpacity);

    CAAnimationGroup *group = [CAAnimationGroup animation];
    group.animations = @[transformAnimation, opacityAnimation];
    group.duration = duration;
    group.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseOut];
    [self.contentView.layer addAnimation:group forKey:animationKey];
}

- (void)animateShadowFromOpacity:(CGFloat)fromOpacity
                       toOpacity:(CGFloat)toOpacity
                      fromRadius:(CGFloat)fromRadius
                        toRadius:(CGFloat)toRadius
                        duration:(CFTimeInterval)duration
                       animation:(NSString *)animationKey {
    if (!self.effectView.layer) return;

    [self.effectView.layer removeAnimationForKey:animationKey];
    self.effectView.layer.shadowOpacity = toOpacity;
    self.effectView.layer.shadowRadius = toRadius;

    CABasicAnimation *opacityAnimation = [CABasicAnimation animationWithKeyPath:@"shadowOpacity"];
    opacityAnimation.fromValue = @(fromOpacity);
    opacityAnimation.toValue = @(toOpacity);

    CABasicAnimation *radiusAnimation = [CABasicAnimation animationWithKeyPath:@"shadowRadius"];
    radiusAnimation.fromValue = @(fromRadius);
    radiusAnimation.toValue = @(toRadius);

    CAAnimationGroup *group = [CAAnimationGroup animation];
    group.animations = @[opacityAnimation, radiusAnimation];
    group.duration = duration;
    group.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseOut];
    [self.effectView.layer addAnimation:group forKey:animationKey];
}

- (void)updateMainPanelMask {
    if (!self.effectView) return;

    CGFloat cornerRadius = [self overlayCornerRadius];
    NSImage *mask = [NSImage imageWithSize:NSMakeSize(cornerRadius * 2 + 1, kPillHeight)
                                   flipped:NO
                            drawingHandler:^BOOL(NSRect dstRect) {
        [[NSColor blackColor] setFill];
        [[NSBezierPath bezierPathWithRoundedRect:dstRect
                                         xRadius:cornerRadius
                                         yRadius:cornerRadius] fill];
        return YES;
    }];
    mask.capInsets = NSEdgeInsetsMake(cornerRadius, cornerRadius, cornerRadius, cornerRadius);
    mask.resizingMode = NSImageResizingModeStretch;
    self.effectView.maskImage = mask;
}

- (void)setupPanel {
    NSRect rect = NSMakeRect(0, 0, kPillWidth, kPillHeight);

    NSPanel *panel = [[NSPanel alloc] initWithContentRect:rect
                                                 styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                                                   backing:NSBackingStoreBuffered
                                                     defer:YES];
    panel.level = NSStatusWindowLevel;
    panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                               NSWindowCollectionBehaviorStationary |
                               NSWindowCollectionBehaviorFullScreenAuxiliary;
    panel.backgroundColor = [NSColor clearColor];
    panel.opaque = NO;
    panel.hasShadow = NO;
    panel.ignoresMouseEvents = YES;
    panel.hidesOnDeactivate = NO;
    panel.alphaValue = 0.0;

    // Visual effect background (HUD material for contrast on any desktop)
    SPClickEffectView *effectView = [[SPClickEffectView alloc] initWithFrame:rect];
    effectView.material     = NSVisualEffectMaterialHUDWindow;
    effectView.blendingMode = NSVisualEffectBlendingModeBehindWindow;
    effectView.state        = NSVisualEffectStateActive;
    effectView.appearance   = [NSAppearance appearanceNamed:NSAppearanceNameVibrantDark];
    effectView.wantsLayer   = YES;
    effectView.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;

    __weak typeof(self) weakSelf = self;
    effectView.clickHandler = ^{
        [weakSelf handleMainPanelClick];
    };

    // Light glow shadow (visible on dark backgrounds)
    effectView.layer.shadowColor   = SPOverlayShadowColor().CGColor;
    effectView.layer.shadowOpacity = kBaseShadowOpacity;
    effectView.layer.shadowRadius  = kBaseShadowRadius;
    effectView.layer.shadowOffset  = CGSizeMake(0, -1.0);

    panel.contentView = effectView;
    self.effectView = effectView;

    // Content drawn on top of the effect view
    self.contentView = [[SPOverlayContentView alloc] initWithFrame:rect];
    self.contentView.wantsLayer = YES;
    self.contentView.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    self.contentView.layer.cornerRadius = [self overlayCornerRadius];
    self.contentView.layer.masksToBounds = YES;
    [effectView addSubview:self.contentView];
    [self updateMainPanelMask];

    self.panel = panel;
}

- (void)setupCaptionPanel {
    NSRect rect = NSMakeRect(0, 0, kPillWidth, SPOverlayLayout.captionHeight);
    NSPanel *panel = [[NSPanel alloc] initWithContentRect:rect
                                                styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                                                  backing:NSBackingStoreBuffered
                                                    defer:YES];
    panel.level = NSStatusWindowLevel;
    panel.collectionBehavior = self.panel.collectionBehavior;
    panel.backgroundColor = NSColor.clearColor;
    panel.opaque = NO;
    panel.hasShadow = NO;
    panel.ignoresMouseEvents = YES;
    panel.hidesOnDeactivate = NO;
    panel.alphaValue = 0.0;
    self.captionView = [[SPOverlayCaptionView alloc] initWithFrame:rect];
    self.captionView.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    panel.contentView = self.captionView;
    self.captionPanel = panel;
}

- (void)setupCard {
    self.card = [[SPOverlayCard alloc] init];
    __weak typeof(self) weakSelf = self;
    self.card.copyHandler = ^(NSString *text) {
        __strong typeof(weakSelf) strongSelf = weakSelf;
        if ([strongSelf.delegate respondsToSelector:@selector(overlayPanel:didCopyCardText:)]) {
            [strongSelf.delegate overlayPanel:strongSelf didCopyCardText:text];
        }
    };
    self.card.closeHandler = ^{
        __strong typeof(weakSelf) strongSelf = weakSelf;
        if ([strongSelf.delegate respondsToSelector:@selector(overlayPanelDidCloseCard:)]) {
            [strongSelf.delegate overlayPanelDidCloseCard:strongSelf];
        }
    };
}

- (void)setMainPanelInteractive:(BOOL)interactive {
    self.panel.ignoresMouseEvents = !interactive;
}

- (void)handleMainPanelClick {
    if (!self.rawAsrFallbackClickEnabled) return;
    if ([self.delegate respondsToSelector:@selector(overlayPanelDidRequestRawAsrFallback:)]) {
        [self.delegate overlayPanelDidRequestRawAsrFallback:self];
    }
}

- (void)clearLingerTimer {
    [self.lingerTimer invalidate];
    self.lingerTimer = nil;
}

- (void)dismissToIdle {
    [self clearLingerTimer];
    [self setRawAsrFallbackClickEnabled:NO];
    self.currentState = @"idle";
    self.caption = nil;
    [self hide];

    if ([self.delegate respondsToSelector:@selector(overlayPanelDidDismiss:)]) {
        [self.delegate overlayPanelDidDismiss:self];
    }
}

- (void)scheduleDismissAfter:(NSTimeInterval)duration {
    [self.lingerTimer invalidate];

    __weak typeof(self) weakSelf = self;
    self.lingerTimer = [NSTimer scheduledTimerWithTimeInterval:duration
                                                       repeats:NO
                                                         block:^(NSTimer *timer) {
        __strong typeof(weakSelf) strongSelf = weakSelf;
        if (!strongSelf) return;
        strongSelf.lingerTimer = nil;
        [strongSelf dismissToIdle];
    }];
}

/// How long the icon of the current mode stays on screen before the pill hides.
- (NSTimeInterval)lingerDurationForCurrentMode {
    if (self.caption.length > 0 && !self.captionIsBadge) {
        return fmin(kMessageMaxLinger,
                    fmax(kMessageMinLinger, self.caption.length * kMessageLingerPerChar + 1.5));
    }
    NSTimeInterval iconLinger;
    switch (self.contentView.mode) {
        case SPOverlayModeClipboard: iconLinger = kClipboardLingerDuration; break;
        case SPOverlayModeCancelled: iconLinger = kCancelledLingerDuration; break;
        case SPOverlayModeError:     iconLinger = kErrorLingerDuration; break;
        case SPOverlayModeLetter:    iconLinger = kModeLetterLingerDuration; break;
        default:                     iconLinger = kSuccessLingerDuration; break;
    }
    return [SPOverlayLayout lingerForCaption:self.caption iconLinger:iconLinger];
}

#pragma mark - Public

- (BOOL)isShowingMessage {
    return self.caption.length > 0 && [self.currentState isEqualToString:@"message"];
}

/// Puts `message` (or nothing) in the caption; the pill keeps its size.
- (void)applyMessage:(NSString *)message {
    self.caption = message.length > 0 ? message : nil;
    self.captionIsBadge = NO;
    [self refreshCaption];
}

/// Sizes and places the caption for the current text, shown while the pill is.
- (void)refreshCaption {
    if (self.caption.length == 0) {
        self.captionPanel.alphaValue = 0.0;
        [self.captionPanel orderOut:nil];
        return;
    }
    self.captionView.text = self.caption;
    [self.captionView setNeedsDisplay:YES];
    [self positionCaption];
    if (self.panel.isVisible && self.panel.alphaValue > 0.01) [self showCaption];
}

- (void)positionCaption {
    if (self.caption.length == 0) return;
    CGFloat width = ceil(SPOverlayCaptionString(self.caption).size.width) + 2 * kCaptionInsetX;
    NSRect frame = [SPOverlayLayout captionFrameWithWidth:width
                                                     pill:[self pillFrame]
                                             visibleFrame:[self pillScreen].visibleFrame];
    [self.captionPanel setFrame:frame display:YES];
}

- (void)showCaption {
    if (self.caption.length == 0) return;
    [self positionCaption];
    SPOrderFrontOnActiveSpace(self.captionPanel);
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *ctx) {
        ctx.duration = kFadeInDuration;
        self.captionPanel.animator.alphaValue = 1.0;
    }];
}

- (void)hideCaption {
    if (!self.captionPanel.isVisible) return;
    NSPanel *captionPanel = self.captionPanel;
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *ctx) {
        ctx.duration = kPanelExitDuration;
        captionPanel.animator.alphaValue = 0.0;
    } completionHandler:^{
        if (captionPanel.alphaValue <= 0.01) [captionPanel orderOut:nil];
    }];
}

- (void)showCardWithTitle:(NSString *)title text:(NSString *)text copyable:(BOOL)copyable {
    if (text.length == 0) return;
    [self refreshWorkScreenUnlessShown];
    NSRect pill = [self pillFrame];
    NSRect visible = [self pillScreen].visibleFrame;
    BOOL captionAbove = self.caption.length > 0 && ![SPOverlayLayout captionGoesBelowPill:pill visibleFrame:visible];
    [self.card showTitle:title
                    text:text
                copyable:copyable
          frameForHeight:^NSRect(CGFloat contentHeight) {
        return [SPOverlayLayout cardFrameWithContentHeight:contentHeight
                                                      pill:pill
                                              captionAbove:captionAbove
                                              visibleFrame:visible];
    }];
    if ([self.delegate respondsToSelector:@selector(overlayPanelDidShowCard:)]) {
        [self.delegate overlayPanelDidShowCard:self];
    }
}

- (void)dismissCard {
    [self.card close];
}

- (BOOL)isShowingCard {
    return self.card.isVisible;
}

- (void)showMessage:(NSString *)text icon:(SPOverlayMessageIcon)icon {
    if (text.length == 0) return;
    if (self.isPreviewActive) [self restoreConfiguredAppearanceIfNeeded];
    [self clearLingerTimer];
    [self setRawAsrFallbackClickEnabled:NO];
    [self stopAnimation];
    self.currentState = @"message";
    switch (icon) {
        case SPOverlayMessageIconError:
            self.contentView.mode = SPOverlayModeError;
            break;
        case SPOverlayMessageIconClipboard:
            self.contentView.mode = SPOverlayModeClipboard;
            break;
        case SPOverlayMessageIconSuccess:
            self.contentView.mode = SPOverlayModeSuccess;
            break;
        case SPOverlayMessageIconNoSpeech:
            self.contentView.mode = SPOverlayModeNotice;
            self.contentView.symbolName = @"ear";
            break;
        case SPOverlayMessageIconNotice:
            self.contentView.mode = SPOverlayModeNotice;
            self.contentView.symbolName = @"exclamationmark.circle";
            break;
    }
    [self applyMessage:text];
    self.contentView.tick = 0;
    [self positionPanel];
    [self.contentView setNeedsDisplay:YES];
    [self show];
    if (icon == SPOverlayMessageIconSuccess) [self startAnimation];
    [self scheduleDismissAfter:[self lingerDurationForCurrentMode]];
}

- (void)setRecordingHint:(NSString *)hint {
    if (![self.currentState hasPrefix:@"recording"] || self.contentView.mode != SPOverlayModeWaveform) return;
    [self applyHint:hint];
}

- (void)setProcessingHint:(NSString *)hint {
    if (self.isShowingMessage || self.contentView.mode != SPOverlayModeProcessing) return;
    [self applyHint:hint];
}

- (void)applyHint:(NSString *)hint {
    NSString *current = self.caption ?: @"";
    if ([current isEqualToString:hint ?: @""]) return;
    [self applyMessage:hint];
    [self positionPanel];
    [self.contentView setNeedsDisplay:YES];
}

- (void)updateState:(NSString *)state {
    // The session ending quietly must not cut short a message or a caption being read.
    BOOL captionLingering = self.captionIsBadge && self.caption.length > 0 && self.lingerTimer;
    if ((self.isShowingMessage || captionLingering) &&
        ([state isEqualToString:@"idle"] || [state isEqualToString:@"completed"])) {
        return;
    }
    BOOL isPreviewState = [state hasSuffix:@"_preview"];
    if (self.isPreviewActive && !isPreviewState) {
        [self restoreConfiguredAppearanceIfNeeded];
    }

    SPOverlayMode mode;
    if ([state hasPrefix:@"recording"]) {
        mode = SPOverlayModeWaveform;
    } else if ([state hasPrefix:@"preparing_paste"] || [state isEqualToString:@"pasting"]) {
        mode = SPOverlayModeSuccess;
    } else if ([state isEqualToString:@"cancelled"]) {
        mode = SPOverlayModeCancelled;
    } else if ([state isEqualToString:@"error"] || [state isEqualToString:@"failed"]) {
        mode = SPOverlayModeError;
    } else {
        mode = SPOverlayModeProcessing;
    }

    // Processing reported again by a later stage (finalizing, correcting)
    // keeps the pulse and the hint running instead of restarting them.
    if (mode == SPOverlayModeProcessing && self.contentView.mode == SPOverlayModeProcessing &&
        self.animationTimer && !self.isShowingMessage) {
        self.currentState = state;
        return;
    }

    [self clearLingerTimer];
    [self setRawAsrFallbackClickEnabled:NO];

    self.currentState = state;
    [self stopAnimation];

    if ([state isEqualToString:@"idle"] || [state isEqualToString:@"completed"]) {
        self.caption = nil;
        [self hide];
        return;
    }

    if (mode == SPOverlayModeWaveform) {
        [(SPOverlayContentView *)self.contentView resetLevels];
        // A new recording: the previous answer is done with.
        if (!isPreviewState) [self.card close];
    }
    self.contentView.mode = mode;
    if (mode == SPOverlayModeCancelled) {
        self.caption = @"Отменено";
        self.captionIsBadge = YES;
        [self refreshCaption];
    } else {
        [self applyMessage:nil];
    }
    self.contentView.tick = 0;
    [self positionPanel];
    [self.contentView setNeedsDisplay:YES];
    [self show];
    [self startAnimation];

    if (mode == SPOverlayModeCancelled || mode == SPOverlayModeError) {
        [self scheduleDismissAfter:[self lingerDurationForCurrentMode]];
    }
}

/// The pill shows no text; transcripts and errors are delivered elsewhere
/// (the focused field, the clipboard, notifications).
- (void)updateInterimText:(NSString *)text {
}

- (void)updateDisplayText:(NSString *)text {
}

- (void)setRawAsrFallbackClickEnabled:(BOOL)enabled {
    _rawAsrFallbackClickEnabled = enabled;
    [self setMainPanelInteractive:enabled];
}

- (void)showModeIndicator:(NSString *)letter {
    if (letter.length == 0) return;
    [self clearLingerTimer];
    [self setRawAsrFallbackClickEnabled:NO];
    [self stopAnimation];
    self.currentState = @"mode_indicator";
    self.contentView.letter = letter;
    self.contentView.mode = SPOverlayModeLetter;
    [self applyMessage:nil];
    self.contentView.tick = 0;
    [self positionPanel];
    [self.contentView setNeedsDisplay:YES];
    [self show];
    [self scheduleDismissAfter:kModeLetterLingerDuration];
}

- (void)showResultBadge:(NSString *)badgeText {
    if (badgeText.length == 0) return;
    [self stopAnimation];
    SPOverlayBadgeIcon icon = [SPOverlayLayout iconForBadgeText:badgeText];
    self.contentView.mode = icon == SPOverlayBadgeIconClipboard ? SPOverlayModeClipboard : SPOverlayModeSuccess;
    self.contentView.tick = 0;
    [self positionPanel];
    [self.contentView setNeedsDisplay:YES];
    self.caption = [SPOverlayLayout captionForBadgeText:badgeText];
    self.captionIsBadge = YES;
    [self refreshCaption];
    if (icon == SPOverlayBadgeIconSuccess) [self startAnimation];
}

- (void)shake {
    if (!self.panel.isVisible || self.panel.alphaValue <= 0.01 || SPOverlayShouldReduceMotion()) return;
    NSPoint origin = self.panel.frame.origin;
    CGMutablePathRef path = CGPathCreateMutable();
    CGPathMoveToPoint(path, NULL, origin.x, origin.y);
    for (NSInteger i = 0; i < 4; i++) {
        CGFloat offset = kShakeOffset * (1.0 - i * 0.25) * (i % 2 == 0 ? -1.0 : 1.0);
        CGPathAddLineToPoint(path, NULL, origin.x + offset, origin.y);
    }
    CGPathAddLineToPoint(path, NULL, origin.x, origin.y);
    CAKeyframeAnimation *animation = [CAKeyframeAnimation animation];
    animation.path = path;
    animation.duration = kShakeDuration;
    CGPathRelease(path);

    // The animation is installed only for this move: the pill's own frame
    // changes (entrance, widening) keep their default animations.
    self.panel.animations = @{ @"frameOrigin" : animation };
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *ctx) {
        ctx.duration = kShakeDuration;
        [self.panel.animator setFrameOrigin:origin];
    } completionHandler:^{
        self.panel.animations = @{};
    }];
}

- (void)reloadAppearanceFromConfig {
    self.configuredBottomMargin = SPOverlayClampBottomMargin(
        SPOverlayConfigCGFloat("overlay.bottom_margin", kDefaultBottomMargin));

    if (!self.isPreviewActive) {
        self.bottomMargin = self.configuredBottomMargin;
        [self positionPanel];
    }
}

- (void)restoreConfiguredAppearanceIfNeeded {
    if (!self.isPreviewActive) return;
    self.previewActive = NO;
    self.bottomMargin = self.configuredBottomMargin;
}

/// Settings preview of the pill position; the text and typography arguments
/// belong to the old text overlay and are ignored.
- (void)showPreviewWithText:(NSString *)text
                   fontSize:(CGFloat)fontSize
                 fontFamily:(NSString *)fontFamily
               bottomMargin:(CGFloat)bottomMargin
          limitVisibleLines:(BOOL)limitVisibleLines
            maxVisibleLines:(NSInteger)maxVisibleLines {
    if (!self.isPreviewActive &&
        self.currentState.length > 0 &&
        ![self.currentState isEqualToString:@"idle"] &&
        ![self.currentState isEqualToString:@"completed"]) {
        return;
    }

    self.previewActive = YES;
    self.bottomMargin = SPOverlayClampBottomMargin(bottomMargin);
    [self updateState:@"recording_preview"];
}

- (void)hidePreview {
    if (!self.isPreviewActive) return;

    [self clearLingerTimer];
    [self setMainPanelInteractive:NO];
    [self restoreConfiguredAppearanceIfNeeded];
    self.currentState = @"idle";
    [self hide];
}

- (void)lingerAndDismiss {
    [self lingerAndDismissWithDuration:0];
}

/// The duration is fixed by the icon on screen; callers' durations were sized
/// for transcript text and are ignored.
- (void)lingerAndDismissWithDuration:(NSTimeInterval)duration {
    [self clearLingerTimer];
    // Keep the pill click-through during linger — it should not block
    // clicks on the app underneath.
    [self setMainPanelInteractive:NO];
    [self scheduleDismissAfter:[self lingerDurationForCurrentMode]];
}

#pragma mark - Template buttons (removed)

- (void)showTemplateButtons:(NSArray<NSDictionary *> *)templates {
    [self lingerAndDismiss];
}

- (void)showTemplateButtons:(NSArray<NSDictionary *> *)templates lingerDuration:(NSTimeInterval)lingerDuration {
    [self lingerAndDismiss];
}

- (void)hideTemplateButtons {
}

- (BOOL)handleNumberKey:(NSInteger)number {
    return NO;
}

#pragma mark - Layout

/// Taken when the pill appears and kept while it is up, so the pill does not
/// jump between displays in the middle of a dictation.
- (void)refreshWorkScreenUnlessShown {
    if (self.screenOverride || (self.panel.isVisible && self.panel.alphaValue > 0.01)) return;
    self.workScreen = [SPWorkScreen currentScreen];
}

- (NSScreen *)pillScreen {
    if (self.screenOverride) return self.screenOverride;
    // A display unplugged since leaves a stale NSScreen behind.
    if (self.workScreen && [NSScreen.screens containsObject:self.workScreen]) return self.workScreen;
    return NSScreen.mainScreen;
}

/// Where the pill sits when shown, whether or not it is on screen now.
- (NSRect)pillFrame {
    NSRect visible = [self pillScreen].visibleFrame;
    return NSMakeRect(round(NSMidX(visible) - kPillWidth / 2.0),
                      NSMinY(visible) + self.bottomMargin,
                      kPillWidth,
                      kPillHeight);
}

- (void)positionPanel {
    [self refreshWorkScreenUnlessShown];
    [self.panel setFrame:[self pillFrame] display:YES];
    [self positionCaption];
}

#pragma mark - Show / Hide

- (void)show {
    BOOL wasVisible = self.panel.isVisible && self.panel.alphaValue > 0.01;
    [self resetMotionPresentationState];
    SPOrderFrontOnActiveSpace(self.panel);
    [self showCaption];

    if (wasVisible || SPOverlayShouldReduceMotion()) {
        if (!wasVisible) {
            self.panel.alphaValue = 0.0;
        }
        [NSAnimationContext runAnimationGroup:^(NSAnimationContext *ctx) {
            ctx.duration = SPOverlayShouldReduceMotion() ? kFadeInDuration : 0.12;
            ctx.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseOut];
            self.panel.animator.alphaValue = 1.0;
        }];
        return;
    }

    NSRect finalFrame = self.panel.frame;
    NSRect startFrame = SPOverlayPresentationStartFrame(finalFrame);
    [self.panel setFrame:startFrame display:YES];
    self.panel.alphaValue = 0.0;

    CATransform3D startTransform = CATransform3DMakeScale(0.982, 0.982, 1.0);
    startTransform = CATransform3DTranslate(startTransform, 0.0, -6.0, 0.0);
    [self animateContentLayerFromTransform:startTransform
                               toTransform:CATransform3DIdentity
                               fromOpacity:0.94
                                 toOpacity:1.0
                                  duration:kPanelEntranceDuration
                                 animation:@"overlayContentEntrance"];
    [self animateShadowFromOpacity:0.05
                         toOpacity:kEntranceShadowOpacity
                        fromRadius:2.0
                          toRadius:kEntranceShadowRadius
                          duration:kPanelEntranceDuration
                         animation:@"overlayShadowEntrance"];

    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)((kPanelEntranceDuration * 0.55) * NSEC_PER_SEC)),
                   dispatch_get_main_queue(), ^{
        if (!self.panel.isVisible || self.panel.alphaValue <= 0.0) return;
        [self animateShadowFromOpacity:kEntranceShadowOpacity
                             toOpacity:kBaseShadowOpacity
                            fromRadius:kEntranceShadowRadius
                              toRadius:kBaseShadowRadius
                              duration:0.18
                             animation:@"overlayShadowEntranceSettle"];
    });

    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *ctx) {
        ctx.duration = kPanelEntranceDuration;
        ctx.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseOut];
        [[self.panel animator] setFrame:finalFrame display:YES];
        self.panel.animator.alphaValue = 1.0;
    }];
}

- (void)hide {
    [self stopAnimation];
    [self setRawAsrFallbackClickEnabled:NO];
    [self setMainPanelInteractive:NO];
    [self hideCaption];

    if (!self.panel.isVisible || self.panel.alphaValue <= 0.01) {
        [self.panel orderOut:nil];
        return;
    }

    NSRect currentFrame = self.panel.frame;
    BOOL reduceMotion = SPOverlayShouldReduceMotion();
    NSRect endFrame = reduceMotion ? currentFrame : SPOverlayDismissalEndFrame(currentFrame);

    if (!reduceMotion) {
        CATransform3D endTransform = CATransform3DMakeScale(0.992, 0.992, 1.0);
        endTransform = CATransform3DTranslate(endTransform, 0.0, -4.0, 0.0);
        [self animateContentLayerFromTransform:CATransform3DIdentity
                                   toTransform:endTransform
                                   fromOpacity:1.0
                                     toOpacity:0.96
                                      duration:kPanelExitDuration
                                     animation:@"overlayContentExit"];
        [self animateShadowFromOpacity:kBaseShadowOpacity
                             toOpacity:0.04
                            fromRadius:kBaseShadowRadius
                              toRadius:2.0
                              duration:kPanelExitDuration
                             animation:@"overlayShadowExit"];
    }

    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *ctx) {
        ctx.duration = reduceMotion ? kFadeOutDuration : kPanelExitDuration;
        ctx.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseOut];
        [[self.panel animator] setFrame:endFrame display:YES];
        self.panel.animator.alphaValue = 0.0;
    } completionHandler:^{
        [self resetMotionPresentationState];
        if ([self.currentState isEqualToString:@"idle"] || [self.currentState isEqualToString:@"completed"]) {
            [self.panel orderOut:nil];
        }
    }];
}

#pragma mark - Animation Timer

- (void)startAnimation {
    self.contentView.tick = 0;
    self.animationTimer = [NSTimer scheduledTimerWithTimeInterval:kAnimInterval
                                                         repeats:YES
                                                           block:^(NSTimer *timer) {
        self.contentView.tick++;
        [self.contentView setNeedsDisplay:YES];
    }];
}

- (void)stopAnimation {
    [self.animationTimer invalidate];
    self.animationTimer = nil;
}

- (void)dealloc {
    [[NSWorkspace sharedWorkspace].notificationCenter removeObserver:self];
    [self stopAnimation];
}

@end
