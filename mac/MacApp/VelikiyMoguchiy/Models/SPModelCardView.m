#import "SPModelCardView.h"

static const CGFloat kPadding = 14.0;

@interface SPModelCardView ()
@property (nonatomic, strong) NSStackView *stack;
@property (nonatomic, strong) NSStackView *header;
@property (nonatomic, assign) CGFloat textWidth;
@end

@implementation SPModelCardView

- (instancetype)initWithWidth:(CGFloat)width {
    self = [super initWithFrame:NSMakeRect(0, 0, width, 60)];
    if (self) {
        self.wantsLayer = YES;
        self.layer.cornerRadius = 8;
        self.layer.borderWidth = 1;
        self.translatesAutoresizingMaskIntoConstraints = NO;
        [self.widthAnchor constraintEqualToConstant:width].active = YES;
        _key = @"";
        _textWidth = width - 2 * kPadding;

        _titleLabel = [NSTextField labelWithString:@""];
        _titleLabel.font = [NSFont systemFontOfSize:14 weight:NSFontWeightSemibold];
        _titleLabel.lineBreakMode = NSLineBreakByTruncatingMiddle;
        _titleLabel.selectable = YES;
        [_titleLabel setContentCompressionResistancePriority:NSLayoutPriorityDefaultLow
                                              forOrientation:NSLayoutConstraintOrientationHorizontal];
        _header = [NSStackView stackViewWithViews:@[ _titleLabel ]];
        _header.spacing = 6;
        _header.alignment = NSLayoutAttributeCenterY;
        [_header.widthAnchor constraintLessThanOrEqualToConstant:_textWidth].active = YES;

        _bodyLabel = [self wrappingLabel:12 color:NSColor.labelColor];
        _detailLabel = [self wrappingLabel:12 color:NSColor.secondaryLabelColor];
        _noteLabel = [self wrappingLabel:12 color:NSColor.systemOrangeColor];
        _resultLabel = [self wrappingLabel:12 color:NSColor.secondaryLabelColor];

        _progressBar = [[NSProgressIndicator alloc] init];
        _progressBar.style = NSProgressIndicatorStyleBar;
        _progressBar.indeterminate = NO;
        _progressBar.minValue = 0;
        _progressBar.maxValue = 1;
        _progressBar.hidden = YES;
        [_progressBar.widthAnchor constraintEqualToConstant:_textWidth].active = YES;
        _progressLabel = [self wrappingLabel:11 color:NSColor.secondaryLabelColor];
        _progressLabel.font = [NSFont monospacedDigitSystemFontOfSize:11 weight:NSFontWeightRegular];

        _extras = [[NSStackView alloc] init];
        _extras.spacing = 10;
        _extras.hidden = YES;
        _buttons = [[NSStackView alloc] init];
        _buttons.spacing = 8;
        _buttons.hidden = YES;

        _stack = [NSStackView stackViewWithViews:@[
            _header, _bodyLabel, _detailLabel, _noteLabel, _extras, _progressBar, _progressLabel, _resultLabel, _buttons
        ]];
        _stack.orientation = NSUserInterfaceLayoutOrientationVertical;
        _stack.alignment = NSLayoutAttributeLeading;
        _stack.spacing = 5;
        _stack.edgeInsets = NSEdgeInsetsMake(kPadding - 2, kPadding, kPadding, kPadding);
        [_stack setCustomSpacing:10 afterView:_resultLabel];
        [_stack setCustomSpacing:10 afterView:_noteLabel];
        _stack.translatesAutoresizingMaskIntoConstraints = NO;
        [self addSubview:_stack];
        [NSLayoutConstraint activateConstraints:@[
            [_stack.leadingAnchor constraintEqualToAnchor:self.leadingAnchor],
            [_stack.trailingAnchor constraintEqualToAnchor:self.trailingAnchor],
            [_stack.topAnchor constraintEqualToAnchor:self.topAnchor],
            [_stack.bottomAnchor constraintEqualToAnchor:self.bottomAnchor],
        ]];
    }
    return self;
}

- (void)updateLayer {
    self.layer.backgroundColor = NSColor.controlBackgroundColor.CGColor;
    self.layer.borderColor = NSColor.separatorColor.CGColor;
}

- (BOOL)wantsUpdateLayer {
    return YES;
}

- (NSTextField *)wrappingLabel:(CGFloat)size color:(NSColor *)color {
    NSTextField *label = [NSTextField wrappingLabelWithString:@""];
    label.font = [NSFont systemFontOfSize:size];
    label.textColor = color;
    label.selectable = YES;
    label.preferredMaxLayoutWidth = self.textWidth;
    [label.widthAnchor constraintEqualToConstant:self.textWidth].active = YES;
    label.hidden = YES;
    return label;
}

- (void)addBadge:(NSString *)text color:(NSColor *)color {
    if (text.length == 0) return;
    NSTextField *label = [NSTextField labelWithString:text];
    label.font = [NSFont systemFontOfSize:10.5 weight:NSFontWeightMedium];
    label.textColor = color;
    label.translatesAutoresizingMaskIntoConstraints = NO;
    NSView *badge = [[NSView alloc] init];
    badge.wantsLayer = YES;
    badge.layer.cornerRadius = 4;
    badge.layer.borderWidth = 1;
    badge.layer.borderColor = [color colorWithAlphaComponent:0.55].CGColor;
    badge.layer.backgroundColor = [color colorWithAlphaComponent:0.08].CGColor;
    [badge addSubview:label];
    [NSLayoutConstraint activateConstraints:@[
        [label.leadingAnchor constraintEqualToAnchor:badge.leadingAnchor constant:5],
        [label.trailingAnchor constraintEqualToAnchor:badge.trailingAnchor constant:-5],
        [label.topAnchor constraintEqualToAnchor:badge.topAnchor constant:1],
        [label.bottomAnchor constraintEqualToAnchor:badge.bottomAnchor constant:-1],
    ]];
    [badge setContentHuggingPriority:NSLayoutPriorityRequired forOrientation:NSLayoutConstraintOrientationHorizontal];
    [badge setContentCompressionResistancePriority:NSLayoutPriorityRequired
                                    forOrientation:NSLayoutConstraintOrientationHorizontal];
    [self.header addArrangedSubview:badge];
}

- (void)setText:(NSString *)text ofLabel:(NSTextField *)label {
    label.stringValue = text ?: @"";
    label.hidden = label.stringValue.length == 0;
}

- (void)setText:(NSString *)text color:(NSColor *)color ofLabel:(NSTextField *)label {
    label.textColor = color;
    [self setText:text ofLabel:label];
}

- (void)setProgress:(double)fraction text:(NSString *)text {
    BOOL shown = fraction >= 0;
    self.progressBar.hidden = !shown;
    self.progressBar.doubleValue = shown ? MIN(fraction, 1.0) : 0;
    [self setText:shown ? text : nil ofLabel:self.progressLabel];
}

- (NSButton *)addButton:(NSString *)title target:(id)target action:(SEL)action {
    NSButton *button = [NSButton buttonWithTitle:title target:target action:action];
    button.bezelStyle = NSBezelStyleRounded;
    button.identifier = self.key;
    [self.buttons addArrangedSubview:button];
    self.buttons.hidden = NO;
    return button;
}

- (NSButton *)addLink:(NSString *)title target:(id)target action:(SEL)action {
    NSButton *button = [NSButton buttonWithTitle:title target:target action:action];
    button.bordered = NO;
    button.identifier = self.key;
    button.attributedTitle = [[NSAttributedString alloc]
        initWithString:title
            attributes:@{NSForegroundColorAttributeName : NSColor.linkColor, NSFontAttributeName : [NSFont systemFontOfSize:12]}];
    [self.extras addArrangedSubview:button];
    self.extras.hidden = NO;
    return button;
}

@end
