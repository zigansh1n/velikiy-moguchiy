#import "SPFileTranscriptionWindowController.h"
#import "SPAudioFileDecoder.h"
#import "SPErrorExplainer.h"
#import "SPFileTranscriptionPolicy.h"
#import "velikiy_moguchiy_core.h"
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import <stdatomic.h>

static const NSTimeInterval kToastDuration = 1.5;

typedef NS_ENUM(NSInteger, SPFileTranscriptionState) {
    SPFileTranscriptionStateRunning,
    SPFileTranscriptionStateDone,
    SPFileTranscriptionStateFailed,
};

/// Open windows; a window keeps itself alive here until it closes.
static NSMutableSet<SPFileTranscriptionWindowController *> *openControllers(void) {
    static NSMutableSet *controllers;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        controllers = [NSMutableSet set];
    });
    return controllers;
}

@interface SPFileTranscriptionWindowController () <NSWindowDelegate> {
    atomic_bool _cancelRequested;
}
@property (nonatomic, strong) NSURL *fileURL;
@property (nonatomic, assign) SPFileTranscriptionState state;
@property (nonatomic, assign) uint64_t jobId;
@property (nonatomic, strong) SPFileTranscriptionOutcome *outcome;

@property (nonatomic, strong) NSTextField *subtitleLabel;
@property (nonatomic, strong) NSProgressIndicator *progressBar;
@property (nonatomic, strong) NSTextField *statusLabel;
@property (nonatomic, strong) NSScrollView *textScroll;
@property (nonatomic, strong) NSTextView *textView;
@property (nonatomic, strong) NSButton *cancelButton;
@property (nonatomic, strong) NSButton *retryButton;
@property (nonatomic, strong) NSButton *textCopyButton;
@property (nonatomic, strong) NSButton *saveTextButton;
@property (nonatomic, strong) NSButton *saveSrtButton;
@property (nonatomic, strong) NSTextField *toastLabel;
@property (nonatomic, assign) NSUInteger toastGeneration;
- (void)showProgressDone:(uint32_t)done total:(uint32_t)total;
- (void)finishWithOutcome:(SPFileTranscriptionOutcome *)outcome;
@end

static void fileProgressCallback(void *ctx, uint32_t done, uint32_t total) {
    SPFileTranscriptionWindowController *controller = (__bridge SPFileTranscriptionWindowController *)ctx;
    dispatch_async(dispatch_get_main_queue(), ^{
        [controller showProgressDone:done total:total];
    });
}

static void fileDoneCallback(void *ctx, const char *json) {
    // Balances the __bridge_retained of -startJob.
    SPFileTranscriptionWindowController *controller = (__bridge_transfer SPFileTranscriptionWindowController *)ctx;
    NSString *text = json ? [NSString stringWithUTF8String:json] : nil;
    dispatch_async(dispatch_get_main_queue(), ^{
        [controller finishWithOutcome:[SPFileTranscriptionPolicy outcomeFromJSON:text]];
    });
}

@implementation SPFileTranscriptionWindowController

#pragma mark - Entry points

+ (NSUInteger)transcribeURLs:(NSArray<NSURL *> *)urls {
    NSArray<NSURL *> *accepted = [SPFileTranscriptionPolicy transcribableURLs:urls];
    for (NSURL *url in accepted) {
        SPFileTranscriptionWindowController *controller = [[self alloc] initWithFileURL:url];
        [openControllers() addObject:controller];
        [controller present];
        [controller startJob];
    }
    return accepted.count;
}

+ (void)chooseFilesAndTranscribe {
    NSOpenPanel *panel = [NSOpenPanel openPanel];
    panel.title = @"Расшифровать файл";
    panel.message = @"Аудио, видео или голосовое сообщение из Telegram (.ogg)";
    panel.prompt = @"Расшифровать";
    panel.allowsMultipleSelection = YES;
    panel.canChooseDirectories = NO;
    NSMutableArray<UTType *> *types = [NSMutableArray arrayWithObjects:UTTypeAudio, UTTypeMovie,
                                                                       UTTypeAudiovisualContent, nil];
    for (NSString *extension in @[ @"ogg", @"oga", @"opus" ]) {
        UTType *type = [UTType typeWithFilenameExtension:extension];
        if (type) [types addObject:type];
    }
    panel.allowedContentTypes = types;
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    [NSApp activateIgnoringOtherApps:YES];
    if ([panel runModal] == NSModalResponseOK) {
        [self transcribeURLs:panel.URLs];
    } else {
        [self restoreAccessoryPolicyUnlessWindowsRemain:nil];
    }
}

+ (void)restoreAccessoryPolicyUnlessWindowsRemain:(NSWindow *)closing {
    for (NSWindow *window in NSApp.windows) {
        if (window != closing && window.visible && window.canBecomeKeyWindow) return;
    }
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
}

#pragma mark - Window

- (instancetype)initWithFileURL:(NSURL *)url {
    NSWindow *window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 600, 460)
                                                   styleMask:NSWindowStyleMaskTitled |
                                                             NSWindowStyleMaskClosable |
                                                             NSWindowStyleMaskMiniaturizable |
                                                             NSWindowStyleMaskResizable
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
    self = [super initWithWindow:window];
    if (self) {
        _fileURL = url;
        window.title = url.lastPathComponent;
        window.representedURL = url;
        window.releasedWhenClosed = NO;
        window.minSize = NSMakeSize(420, 280);
        window.delegate = self;
        [self buildContent];
        [window center];
        // Several files at once open as a cascade, not a stack.
        NSWindow *front = NSApp.keyWindow;
        if ([front.windowController isKindOfClass:[SPFileTranscriptionWindowController class]]) {
            NSPoint topLeft = NSMakePoint(NSMinX(front.frame), NSMaxY(front.frame));
            [window setFrameTopLeftPoint:[window cascadeTopLeftFromPoint:topLeft]];
        }
    }
    return self;
}

- (void)present {
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    [NSApp activateIgnoringOtherApps:YES];
    [self.window makeKeyAndOrderFront:nil];
}

- (void)windowDidBecomeKey:(NSNotification *)notification {
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
}

- (void)windowWillClose:(NSNotification *)notification {
    if (self.state == SPFileTranscriptionStateRunning) [self cancelJob];
    [[self class] restoreAccessoryPolicyUnlessWindowsRemain:self.window];
    [openControllers() removeObject:self];
}

- (NSButton *)buttonWithTitle:(NSString *)title action:(SEL)action {
    NSButton *button = [NSButton buttonWithTitle:title target:self action:action];
    button.bezelStyle = NSBezelStyleRounded;
    button.translatesAutoresizingMaskIntoConstraints = NO;
    return button;
}

- (NSTextField *)labelWithFont:(NSFont *)font color:(NSColor *)color {
    NSTextField *label = [NSTextField wrappingLabelWithString:@""];
    label.font = font;
    label.textColor = color;
    label.selectable = YES;
    label.translatesAutoresizingMaskIntoConstraints = NO;
    return label;
}

- (void)buildContent {
    NSView *content = self.window.contentView;

    self.subtitleLabel = [self labelWithFont:[NSFont systemFontOfSize:12] color:NSColor.secondaryLabelColor];

    self.progressBar = [[NSProgressIndicator alloc] init];
    self.progressBar.style = NSProgressIndicatorStyleBar;
    self.progressBar.indeterminate = YES;
    self.progressBar.translatesAutoresizingMaskIntoConstraints = NO;

    self.statusLabel = [self labelWithFont:[NSFont systemFontOfSize:13] color:NSColor.labelColor];

    self.textView = [[NSTextView alloc] init];
    self.textView.font = [NSFont systemFontOfSize:14];
    self.textView.richText = NO;
    self.textView.allowsUndo = YES;
    self.textView.textContainerInset = NSMakeSize(6, 8);
    self.textView.autoresizingMask = NSViewWidthSizable;
    self.textView.verticallyResizable = YES;
    self.textView.textContainer.widthTracksTextView = YES;
    self.textScroll = [[NSScrollView alloc] init];
    self.textScroll.documentView = self.textView;
    self.textScroll.hasVerticalScroller = YES;
    self.textScroll.borderType = NSBezelBorder;
    self.textScroll.translatesAutoresizingMaskIntoConstraints = NO;
    self.textScroll.hidden = YES;

    self.cancelButton = [self buttonWithTitle:@"Отменить" action:@selector(cancelPressed:)];
    self.retryButton = [self buttonWithTitle:@"Повторить" action:@selector(retryPressed:)];
    self.textCopyButton = [self buttonWithTitle:@"Копировать" action:@selector(copyPressed:)];
    self.textCopyButton.keyEquivalent = @"c";
    self.textCopyButton.keyEquivalentModifierMask = NSEventModifierFlagCommand | NSEventModifierFlagShift;
    self.saveTextButton = [self buttonWithTitle:@"Сохранить как .txt" action:@selector(saveTextPressed:)];
    self.saveSrtButton = [self buttonWithTitle:@"Субтитры .srt" action:@selector(saveSrtPressed:)];
    self.saveSrtButton.toolTip = @"Текст с отметками времени для видеоплеера или монтажа";

    self.toastLabel = [self labelWithFont:[NSFont systemFontOfSize:12] color:NSColor.secondaryLabelColor];
    self.toastLabel.selectable = NO;

    NSStackView *buttons = [NSStackView stackViewWithViews:@[
        self.toastLabel, self.cancelButton, self.retryButton, self.saveSrtButton, self.saveTextButton, self.textCopyButton
    ]];
    buttons.orientation = NSUserInterfaceLayoutOrientationHorizontal;
    buttons.spacing = 8;
    buttons.translatesAutoresizingMaskIntoConstraints = NO;
    [buttons setHuggingPriority:NSLayoutPriorityDefaultLow forOrientation:NSLayoutConstraintOrientationHorizontal];
    [self.toastLabel setContentHuggingPriority:NSLayoutPriorityDefaultLow
                                forOrientation:NSLayoutConstraintOrientationHorizontal];

    for (NSView *view in @[ self.subtitleLabel, self.progressBar, self.statusLabel, self.textScroll, buttons ]) {
        [content addSubview:view];
    }
    CGFloat margin = 20;
    [NSLayoutConstraint activateConstraints:@[
        [self.subtitleLabel.topAnchor constraintEqualToAnchor:content.topAnchor constant:14],
        [self.subtitleLabel.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:margin],
        [self.subtitleLabel.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-margin],

        [self.progressBar.topAnchor constraintEqualToAnchor:self.subtitleLabel.bottomAnchor constant:18],
        [self.progressBar.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:margin],
        [self.progressBar.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-margin],

        [self.statusLabel.topAnchor constraintEqualToAnchor:self.progressBar.bottomAnchor constant:10],
        [self.statusLabel.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:margin],
        [self.statusLabel.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-margin],

        [self.textScroll.topAnchor constraintEqualToAnchor:self.subtitleLabel.bottomAnchor constant:10],
        [self.textScroll.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:margin],
        [self.textScroll.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-margin],
        [self.textScroll.bottomAnchor constraintEqualToAnchor:buttons.topAnchor constant:-14],

        [buttons.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:margin],
        [buttons.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-margin],
        [buttons.bottomAnchor constraintEqualToAnchor:content.bottomAnchor constant:-16],
    ]];
    [self applyState];
}

- (void)applyState {
    BOOL running = self.state == SPFileTranscriptionStateRunning;
    BOOL done = self.state == SPFileTranscriptionStateDone;
    BOOL failed = self.state == SPFileTranscriptionStateFailed;
    self.progressBar.hidden = !running;
    self.statusLabel.hidden = done;
    self.textScroll.hidden = !done;
    self.cancelButton.hidden = !running;
    self.retryButton.hidden = !failed;
    self.textCopyButton.hidden = !done;
    self.saveTextButton.hidden = !done;
    self.saveSrtButton.hidden = !done || self.outcome.srt.length == 0;
    if (running) {
        [self.progressBar startAnimation:nil];
    } else {
        [self.progressBar stopAnimation:nil];
    }
}

#pragma mark - Job

- (void)startJob {
    self.state = SPFileTranscriptionStateRunning;
    self.outcome = nil;
    self.jobId = 0;
    atomic_store(&_cancelRequested, false);
    self.subtitleLabel.stringValue = @"Распознавание выбранной в настройках моделью";
    self.statusLabel.stringValue = [SPFileTranscriptionPolicy progressTextForDone:0 total:0];
    self.progressBar.indeterminate = YES;
    [self applyState];

    NSURL *url = self.fileURL;
    BOOL ogg = [SPFileTranscriptionPolicy isOggOpusURL:url];
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        BOOL scoped = [url startAccessingSecurityScopedResource];
        NSData *pcm = nil;
        NSError *error = nil;
        if (!ogg) {
            pcm = [SPAudioFileDecoder pcmFromURL:url
                                     isCancelled:^BOOL {
                                         return atomic_load(&self->_cancelRequested);
                                     }
                                           error:&error];
        }
        if (scoped) [url stopAccessingSecurityScopedResource];
        dispatch_async(dispatch_get_main_queue(), ^{
            if (atomic_load(&self->_cancelRequested)) {
                [self finishWithOutcome:[SPFileTranscriptionPolicy outcomeFromJSON:@"{\"cancelled\":true}"]];
                return;
            }
            if (!ogg && !pcm) {
                [self showDecodeError:error];
                return;
            }
            [self startCoreWithPCM:pcm];
        });
    });
}

- (void)startCoreWithPCM:(NSData *)pcm {
    void *ctx = (__bridge_retained void *)self;
    uint64_t job = sp_file_transcribe_start(pcm ? NULL : self.fileURL.path.fileSystemRepresentation,
                                            pcm ? (const int16_t *)pcm.bytes : NULL,
                                            pcm.length / sizeof(int16_t),
                                            fileProgressCallback,
                                            fileDoneCallback,
                                            ctx);
    if (job == 0) {
        CFRelease(ctx);
        [self showDecodeError:nil];
        return;
    }
    self.jobId = job;
}

- (void)cancelJob {
    atomic_store(&_cancelRequested, true);
    if (self.jobId != 0) sp_file_transcribe_cancel(self.jobId);
}

- (void)showProgressDone:(uint32_t)done total:(uint32_t)total {
    if (self.state != SPFileTranscriptionStateRunning) return;
    self.statusLabel.stringValue = [SPFileTranscriptionPolicy progressTextForDone:done total:total];
    if (total > 0) {
        self.progressBar.indeterminate = NO;
        self.progressBar.maxValue = total;
        self.progressBar.doubleValue = done;
    }
}

- (void)showDecodeError:(NSError *)error {
    self.state = SPFileTranscriptionStateFailed;
    NSString *reason = error.localizedDescription ?: @"файл не удалось прочитать";
    self.statusLabel.stringValue = [NSString stringWithFormat:@"Не удалось прочитать звук: %@.", reason];
    self.subtitleLabel.stringValue = @"";
    [self applyState];
}

- (void)finishWithOutcome:(SPFileTranscriptionOutcome *)outcome {
    self.jobId = 0;
    self.outcome = outcome;
    if ([outcome.status isEqualToString:@"cancelled"]) {
        self.state = SPFileTranscriptionStateFailed;
        self.statusLabel.stringValue = @"Расшифровка отменена.";
        self.subtitleLabel.stringValue = @"";
    } else if ([outcome.status isEqualToString:@"error"]) {
        self.state = SPFileTranscriptionStateFailed;
        SPErrorExplanation *explanation = [SPErrorExplainer explanationForMessage:outcome.error recordingKept:NO];
        self.subtitleLabel.stringValue = @"";
        self.statusLabel.stringValue =
            [NSString stringWithFormat:@"%@. %@ (код %@)", explanation.shortText, explanation.detail, explanation.code];
    } else if (outcome.text.length == 0) {
        self.state = SPFileTranscriptionStateFailed;
        self.statusLabel.stringValue = @"В файле не нашлось речи.";
        self.subtitleLabel.stringValue = [SPFileTranscriptionPolicy durationText:outcome.duration];
    } else {
        self.state = SPFileTranscriptionStateDone;
        self.textView.string = outcome.text;
        self.subtitleLabel.stringValue = [NSString stringWithFormat:@"%@ · распознано: %@",
                                                   [SPFileTranscriptionPolicy durationText:outcome.duration],
                                                   [SPFileTranscriptionPolicy providerTitle:outcome.provider]];
        [self.window makeFirstResponder:self.textView];
    }
    [self applyState];
}

#pragma mark - Actions

- (void)cancelPressed:(id)sender {
    self.cancelButton.enabled = NO;
    [self cancelJob];
}

- (void)retryPressed:(id)sender {
    self.cancelButton.enabled = YES;
    [self startJob];
}

- (void)copyPressed:(id)sender {
    NSPasteboard *pasteboard = [NSPasteboard generalPasteboard];
    [pasteboard clearContents];
    [pasteboard setString:self.textView.string forType:NSPasteboardTypeString];
    [self showToast:@"Скопировано"];
}

- (void)saveTextPressed:(id)sender {
    [self saveString:self.textView.string extension:@"txt" type:UTTypePlainText];
}

- (void)saveSrtPressed:(id)sender {
    UTType *srt = [UTType typeWithFilenameExtension:@"srt"] ?: UTTypePlainText;
    [self saveString:self.outcome.srt extension:@"srt" type:srt];
}

- (void)saveString:(NSString *)string extension:(NSString *)extension type:(UTType *)type {
    NSSavePanel *panel = [NSSavePanel savePanel];
    panel.nameFieldStringValue = [SPFileTranscriptionPolicy saveNameForSourceName:self.fileURL.lastPathComponent
                                                                         extension:extension];
    panel.directoryURL = self.fileURL.URLByDeletingLastPathComponent;
    panel.allowedContentTypes = @[ type ];
    [panel beginSheetModalForWindow:self.window
                  completionHandler:^(NSModalResponse result) {
                      if (result != NSModalResponseOK || !panel.URL) return;
                      NSError *error = nil;
                      if ([string writeToURL:panel.URL atomically:YES encoding:NSUTF8StringEncoding error:&error]) {
                          [self showToast:@"Сохранено"];
                      } else {
                          [[NSAlert alertWithError:error] beginSheetModalForWindow:self.window
                                                                 completionHandler:nil];
                      }
                  }];
}

- (void)showToast:(NSString *)message {
    NSUInteger generation = ++self.toastGeneration;
    self.toastLabel.stringValue = message;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(kToastDuration * NSEC_PER_SEC)),
                   dispatch_get_main_queue(), ^{
                       if (generation == self.toastGeneration) self.toastLabel.stringValue = @"";
                   });
}

@end
