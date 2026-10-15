#import "SPDictionaryOfferPanel.h"
#import "SPAddWordDialog.h"
#import "velikiy_moguchiy_core.h"

static const NSTimeInterval kOfferDuration = 15.0;
static const NSTimeInterval kConfirmationDuration = 1.6;
static const CGFloat kCardHeight = 38.0;
static const CGFloat kCardMaxWidth = 460.0;
/// The pill's own height and default distance from the bottom of the screen
/// (Overlay/SPOverlayPanel): the card sits just above it.
static const CGFloat kPillHeight = 30.0;
static const CGFloat kDefaultPillMargin = 10.0;
static const CGFloat kGapAbovePill = 8.0;

static NSColor *cardColor(void) {
    return [NSColor colorWithSRGBRed:0x0E / 255.0 green:0x0A / 255.0 blue:0x08 / 255.0 alpha:0.96];
}

static NSColor *inkColor(void) {
    return [NSColor colorWithSRGBRed:0xE8 / 255.0 green:0xDC / 255.0 blue:0xC8 / 255.0 alpha:1.0];
}

static CGFloat pillBottomMargin(void) {
    char *raw = sp_config_get("overlay.bottom_margin");
    if (!raw) return kDefaultPillMargin;
    NSString *value = [NSString stringWithUTF8String:raw];
    sp_core_free_string(raw);
    return value.length > 0 ? MAX(0.0, value.doubleValue) : kDefaultPillMargin;
}

/// A panel that can be clicked without becoming key, so the field the user
/// is typing in keeps the focus.
@interface SPOfferPanelWindow : NSPanel
@end

@implementation SPOfferPanelWindow
- (BOOL)canBecomeKeyWindow {
    return NO;
}
- (BOOL)canBecomeMainWindow {
    return NO;
}
@end

/// Takes the click that would otherwise only bring the panel forward: the
/// user is in another app, and one click must answer.
@interface SPOfferButton : NSButton
@end

@implementation SPOfferButton
- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    return YES;
}
@end

@interface SPDictionaryOfferPanel ()
@property (nonatomic, strong, nullable) SPOfferPanelWindow *panel;
@property (nonatomic, strong, nullable) NSTextField *label;
@property (nonatomic, strong, nullable) NSButton *yesButton;
@property (nonatomic, strong, nullable) NSButton *noButton;
@property (nonatomic, copy, nullable) NSString *term;
@property (nonatomic, copy, nullable) NSString *heard;
@property (nonatomic, strong) NSMutableSet<NSString *> *declined;
@property (nonatomic, assign) NSUInteger generation;
@end

@implementation SPDictionaryOfferPanel

- (instancetype)init {
    self = [super init];
    if (self) {
        _declined = [NSMutableSet set];
    }
    return self;
}

- (void)buildPanel {
    SPOfferPanelWindow *panel = [[SPOfferPanelWindow alloc]
        initWithContentRect:NSMakeRect(0, 0, 320, kCardHeight)
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

    NSView *content = [[NSView alloc] initWithFrame:panel.contentLayoutRect];
    content.wantsLayer = YES;
    content.layer.backgroundColor = cardColor().CGColor;
    content.layer.cornerRadius = kCardHeight / 2.0;
    content.appearance = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];
    panel.contentView = content;

    self.label = [NSTextField labelWithString:@""];
    self.label.font = [NSFont systemFontOfSize:12.5 weight:NSFontWeightMedium];
    self.label.textColor = inkColor();
    self.label.lineBreakMode = NSLineBreakByTruncatingMiddle;
    [content addSubview:self.label];

    self.yesButton = [SPOfferButton buttonWithTitle:@"Да" target:self action:@selector(accept:)];
    self.noButton = [SPOfferButton buttonWithTitle:@"Нет" target:self action:@selector(decline:)];
    for (NSButton *button in @[ self.yesButton, self.noButton ]) {
        button.controlSize = NSControlSizeSmall;
        button.font = [NSFont systemFontOfSize:NSFont.smallSystemFontSize];
        button.bezelStyle = NSBezelStyleRounded;
        button.refusesFirstResponder = YES;
        [button sizeToFit];
        [content addSubview:button];
    }
    self.yesButton.toolTip = @"Добавить в словарь: распознавание будет писать так";
    self.panel = panel;
}

- (void)offerTerm:(NSString *)term heard:(NSString *)heard {
    if (term.length == 0 || [self.declined containsObject:term.lowercaseString]) return;
    if (!self.panel) [self buildPanel];
    self.term = term;
    self.heard = heard ?: @"";
    self.label.stringValue = [NSString stringWithFormat:@"Запомнить «%@» в словаре?", term];
    self.yesButton.hidden = NO;
    self.noButton.hidden = NO;
    [self layoutAndShow];
    [self dismissAfter:kOfferDuration];
}

- (void)layoutAndShow {
    CGFloat inset = 14.0;
    CGFloat gap = 6.0;
    [self.label sizeToFit];
    CGFloat buttonsW = self.yesButton.hidden ? 0.0
        : NSWidth(self.yesButton.frame) + gap + NSWidth(self.noButton.frame) + gap * 2;
    CGFloat labelW = MIN(NSWidth(self.label.frame), kCardMaxWidth - 2 * inset - buttonsW);
    CGFloat width = inset + labelW + buttonsW + (self.yesButton.hidden ? inset : inset - 4.0);

    self.label.frame = NSMakeRect(inset, floor((kCardHeight - NSHeight(self.label.frame)) / 2.0),
                                  labelW, NSHeight(self.label.frame));
    CGFloat x = inset + labelW + gap * 2;
    for (NSButton *button in @[ self.yesButton, self.noButton ]) {
        button.frame = NSMakeRect(x, floor((kCardHeight - NSHeight(button.frame)) / 2.0),
                                  NSWidth(button.frame), NSHeight(button.frame));
        x += NSWidth(button.frame) + gap;
    }

    NSRect visible = [NSScreen mainScreen].visibleFrame;
    NSRect frame = NSMakeRect(round(NSMidX(visible) - width / 2.0),
                              NSMinY(visible) + pillBottomMargin() + kPillHeight + kGapAbovePill,
                              width, kCardHeight);
    [self.panel setFrame:frame display:YES];
    [self.panel orderFrontRegardless];
}

- (void)dismissAfter:(NSTimeInterval)delay {
    NSUInteger generation = ++self.generation;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(delay * NSEC_PER_SEC)),
                   dispatch_get_main_queue(), ^{
        if (generation == self.generation) [self dismiss];
    });
}

- (void)dismiss {
    self.generation++;
    [self.panel orderOut:nil];
    self.term = nil;
}

- (void)accept:(id)sender {
    NSString *term = self.term;
    if (term.length == 0) return;
    int result = sp_dictionary_add(term.UTF8String, self.heard.UTF8String);
    if (result < 0) {
        self.label.stringValue = @"Не удалось записать словарь";
    } else {
        self.label.stringValue = [NSString stringWithFormat:@"✓ «%@» в словаре", term];
        [[NSNotificationCenter defaultCenter] postNotificationName:SPDictionaryDidChangeNotification
                                                            object:self];
        if (self.onAdded) self.onAdded(term);
    }
    self.term = nil;
    self.yesButton.hidden = YES;
    self.noButton.hidden = YES;
    [self layoutAndShow];
    [self dismissAfter:kConfirmationDuration];
}

- (void)decline:(id)sender {
    if (self.term.length > 0) [self.declined addObject:self.term.lowercaseString];
    [self dismiss];
}

@end
