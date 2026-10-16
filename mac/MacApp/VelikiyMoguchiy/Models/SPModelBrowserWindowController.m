#import "SPModelBrowserWindowController.h"
#import "SPAppPaths.h"
#import "SPDownloadMeter.h"
#import "SPHfModelPresenter.h"
#import "SPLocalModelCheck.h"
#import "SPModelCardPresenter.h"
#import "SPModelCardView.h"
#import "SPModelDownloads.h"
#import "SPRustBridge.h"
#import "velikiy_moguchiy_core.h"

static const CGFloat kWindowWidth = 760.0;
static const CGFloat kMargin = 20.0;
static const CGFloat kCardWidth = 700.0;
static const NSTimeInterval kSearchDelay = 0.45;

typedef NS_ENUM(NSInteger, SPModelTab) {
    SPModelTabRecommended,
    SPModelTabSearch,
    SPModelTabThisMac,
    SPModelTabMine,
};

/// Carries a block through the core's C callback; the core calls it once.
@interface SPModelCallbackBox : NSObject
@property (nonatomic, copy) void (^block)(NSDictionary *result);
@end

@implementation SPModelCallbackBox
@end

static void modelCallback(void *ctx, const char *json) {
    SPModelCallbackBox *box = (__bridge_transfer SPModelCallbackBox *)ctx;
    NSData *data = json ? [NSData dataWithBytes:json length:strlen(json)] : nil;
    id parsed = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL] : nil;
    NSDictionary *result = [parsed isKindOfClass:[NSDictionary class]] ? parsed : @{@"error" : @"Ядро не ответило."};
    dispatch_async(dispatch_get_main_queue(), ^{
        box.block(result);
    });
}

static void *retainedBox(void (^block)(NSDictionary *)) {
    SPModelCallbackBox *box = [[SPModelCallbackBox alloc] init];
    box.block = block;
    return (__bridge_retained void *)box;
}

/// Parses and frees a JSON string the core returned.
static id ownedJSON(char *json) {
    if (!json) return nil;
    NSData *data = [NSData dataWithBytes:json length:strlen(json)];
    sp_core_free_string(json);
    return [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL];
}

static NSString *configValue(NSString *key) {
    char *raw = sp_config_get(key.UTF8String);
    if (!raw) return @"";
    NSString *value = [NSString stringWithUTF8String:raw] ?: @"";
    sp_core_free_string(raw);
    return value;
}

static NSString *text(id value) {
    return [value isKindOfClass:[NSString class]] ? value : @"";
}

static NSArray *list(id value) {
    return [value isKindOfClass:[NSArray class]] ? value : @[];
}

static NSDictionary *dict(id value) {
    return [value isKindOfClass:[NSDictionary class]] ? value : @{};
}

@interface SPModelFlippedView : NSView
@end

@implementation SPModelFlippedView
- (BOOL)isFlipped {
    return YES;
}
@end

static SPModelBrowserWindowController *sharedController;

@interface SPModelBrowserWindowController () <NSWindowDelegate, NSSearchFieldDelegate>
@property (nonatomic, strong) SPRustBridge *bridge;
@property (nonatomic, weak) id<SPModelBrowserDelegate> delegate;

@property (nonatomic, strong) NSSegmentedControl *tabs;
@property (nonatomic, strong) NSTabView *tabView;
@property (nonatomic, strong) NSStackView *recommendedList;
@property (nonatomic, strong) NSStackView *searchList;
@property (nonatomic, strong) NSStackView *localList;
@property (nonatomic, strong) NSStackView *mineList;

@property (nonatomic, copy) NSArray<NSDictionary *> *recommended;
@property (nonatomic, copy) NSArray<NSDictionary *> *mine;

@property (nonatomic, strong) NSSearchField *searchField;
@property (nonatomic, strong) NSButton *russianOnlyBox;
@property (nonatomic, strong) NSProgressIndicator *searchSpinner;
@property (nonatomic, strong) NSTextField *searchStatus;
@property (nonatomic, strong) NSTimer *searchTimer;
@property (nonatomic, copy) NSArray<NSDictionary *> *searchItems;
@property (nonatomic, assign) NSUInteger searchGeneration;
@property (nonatomic, assign) BOOL searchedOnce;

@property (nonatomic, copy) NSString *detailsRepo;
@property (nonatomic, strong) NSDictionary *inspection;
@property (nonatomic, copy) NSString *detailsError;
@property (nonatomic, copy) NSString *detailsModelPath;
@property (nonatomic, assign) NSInteger detailsVariant;
@property (nonatomic, assign) NSUInteger detailsGeneration;

@property (nonatomic, strong) NSProgressIndicator *localSpinner;
@property (nonatomic, strong) NSTextField *localStatus;
@property (nonatomic, copy) NSArray<NSDictionary *> *finds;
@property (nonatomic, strong) NSMutableArray<NSDictionary *> *pickedFinds;
@property (nonatomic, strong) NSMutableDictionary<NSString *, NSNumber *> *variantChoice;
@property (nonatomic, assign) BOOL scanning;
@property (nonatomic, assign) BOOL scannedOnce;

/// A line under a card: key → @[text, color].
@property (nonatomic, strong) NSMutableDictionary<NSString *, NSArray *> *messages;
/// Keys with a request in flight.
@property (nonatomic, strong) NSMutableSet<NSString *> *busy;
@end

@implementation SPModelBrowserWindowController

#pragma mark - Entry point

+ (void)showWithBridge:(SPRustBridge *)bridge
              delegate:(id<SPModelBrowserDelegate>)delegate
          parentWindow:(NSWindow *)parent {
    if (!sharedController) {
        sharedController = [[self alloc] initWithBridge:bridge];
        if (parent) {
            NSRect frame = sharedController.window.frame;
            frame.origin.x = NSMidX(parent.frame) - frame.size.width / 2;
            frame.origin.y = NSMaxY(parent.frame) - frame.size.height - 40;
            [sharedController.window setFrameOrigin:frame.origin];
        } else {
            [sharedController.window center];
        }
    }
    sharedController.delegate = delegate;
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    [NSApp activateIgnoringOtherApps:YES];
    [sharedController.window makeKeyAndOrderFront:nil];
    [sharedController reloadVisibleTab];
}

- (instancetype)initWithBridge:(SPRustBridge *)bridge {
    NSWindow *window = [[NSWindow alloc]
        initWithContentRect:NSMakeRect(0, 0, kWindowWidth, 640)
                  styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable |
                            NSWindowStyleMaskResizable
                    backing:NSBackingStoreBuffered
                      defer:NO];
    self = [super initWithWindow:window];
    if (self) {
        _bridge = bridge;
        _pickedFinds = [NSMutableArray array];
        _variantChoice = [NSMutableDictionary dictionary];
        _messages = [NSMutableDictionary dictionary];
        _busy = [NSMutableSet set];
        _recommended = @[];
        _mine = @[];
        _searchItems = @[];
        _finds = @[];
        window.title = @"Модели распознавания";
        window.releasedWhenClosed = NO;
        window.delegate = self;
        window.contentMinSize = NSMakeSize(kWindowWidth, 440);
        window.contentMaxSize = NSMakeSize(kWindowWidth, 4000);
        [self buildContent];
        [[NSNotificationCenter defaultCenter] addObserver:self
                                                 selector:@selector(downloadsChanged:)
                                                     name:SPModelDownloadsDidChangeNotification
                                                   object:nil];
    }
    return self;
}

- (void)dealloc {
    [[NSNotificationCenter defaultCenter] removeObserver:self];
}

#pragma mark - Building

- (NSTextField *)introLabel:(NSString *)string {
    NSTextField *label = [NSTextField wrappingLabelWithString:string];
    label.font = [NSFont systemFontOfSize:12];
    label.textColor = NSColor.secondaryLabelColor;
    label.preferredMaxLayoutWidth = kWindowWidth - 2 * kMargin;
    [label.widthAnchor constraintEqualToConstant:kWindowWidth - 2 * kMargin].active = YES;
    return label;
}

- (NSProgressIndicator *)spinner {
    NSProgressIndicator *spinner = [[NSProgressIndicator alloc] init];
    spinner.style = NSProgressIndicatorStyleSpinning;
    spinner.controlSize = NSControlSizeSmall;
    spinner.displayedWhenStopped = NO;
    return spinner;
}

/// A tab: `header` rows above a scrolling column of cards.
- (NSView *)tabWithHeader:(NSArray<NSView *> *)header list:(NSStackView **)listOut {
    NSView *tab = [[NSView alloc] init];
    NSStackView *top = [NSStackView stackViewWithViews:header];
    top.orientation = NSUserInterfaceLayoutOrientationVertical;
    top.alignment = NSLayoutAttributeLeading;
    top.spacing = 8;
    top.translatesAutoresizingMaskIntoConstraints = NO;

    NSStackView *cards = [[NSStackView alloc] init];
    cards.orientation = NSUserInterfaceLayoutOrientationVertical;
    cards.alignment = NSLayoutAttributeLeading;
    cards.spacing = 10;
    cards.edgeInsets = NSEdgeInsetsMake(2, 0, 16, 0);
    cards.translatesAutoresizingMaskIntoConstraints = NO;
    SPModelFlippedView *document = [[SPModelFlippedView alloc] init];
    document.translatesAutoresizingMaskIntoConstraints = NO;
    [document addSubview:cards];
    NSScrollView *scroll = [[NSScrollView alloc] init];
    scroll.hasVerticalScroller = YES;
    scroll.autohidesScrollers = YES;
    scroll.drawsBackground = NO;
    scroll.borderType = NSNoBorder;
    scroll.documentView = document;
    scroll.translatesAutoresizingMaskIntoConstraints = NO;

    [tab addSubview:top];
    [tab addSubview:scroll];
    NSView *clip = scroll.contentView;
    [NSLayoutConstraint activateConstraints:@[
        [top.topAnchor constraintEqualToAnchor:tab.topAnchor constant:4],
        [top.leadingAnchor constraintEqualToAnchor:tab.leadingAnchor constant:kMargin],
        [top.trailingAnchor constraintEqualToAnchor:tab.trailingAnchor constant:-kMargin],
        [scroll.topAnchor constraintEqualToAnchor:top.bottomAnchor constant:12],
        [scroll.leadingAnchor constraintEqualToAnchor:tab.leadingAnchor constant:kMargin],
        [scroll.trailingAnchor constraintEqualToAnchor:tab.trailingAnchor constant:-kMargin + 10],
        [scroll.bottomAnchor constraintEqualToAnchor:tab.bottomAnchor],
        [document.leadingAnchor constraintEqualToAnchor:clip.leadingAnchor],
        [document.topAnchor constraintEqualToAnchor:clip.topAnchor],
        [document.widthAnchor constraintEqualToAnchor:clip.widthAnchor],
        [cards.leadingAnchor constraintEqualToAnchor:document.leadingAnchor],
        [cards.trailingAnchor constraintEqualToAnchor:document.trailingAnchor],
        [cards.topAnchor constraintEqualToAnchor:document.topAnchor],
        [cards.bottomAnchor constraintEqualToAnchor:document.bottomAnchor],
    ]];
    *listOut = cards;
    return tab;
}

- (void)buildContent {
    NSView *content = self.window.contentView;
    self.tabs = [NSSegmentedControl segmentedControlWithLabels:@[ @"Рекомендуемые", @"Поиск", @"На этом Mac", @"Мои модели" ]
                                                  trackingMode:NSSegmentSwitchTrackingSelectOne
                                                        target:self
                                                        action:@selector(tabChanged:)];
    self.tabs.selectedSegment = SPModelTabRecommended;
    self.tabs.translatesAutoresizingMaskIntoConstraints = NO;

    self.tabView = [[NSTabView alloc] init];
    self.tabView.tabViewType = NSNoTabsNoBorder;
    self.tabView.translatesAutoresizingMaskIntoConstraints = NO;

    NSStackView *recommendedList;
    NSView *recommendedTab = [self
        tabWithHeader:@[ [self introLabel:@"Модели, которые мы проверили для русской речи. Каждая скачивается с "
                                          @"закреплённой версии, файлы сверяются по SHA-256. Распознавание "
                                          @"переключится на модель, только когда вы нажмёте «Сделать основной»."] ]
                 list:&recommendedList];
    self.recommendedList = recommendedList;

    self.searchField = [[NSSearchField alloc] init];
    self.searchField.placeholderString = @"Название модели или автора: whisper, gigaam, parakeet…";
    self.searchField.delegate = self;
    self.searchField.target = self;
    self.searchField.action = @selector(searchSubmitted:);
    self.searchField.sendsWholeSearchString = YES;
    [self.searchField.widthAnchor constraintEqualToConstant:440].active = YES;
    self.russianOnlyBox = [NSButton checkboxWithTitle:@"Только с русским" target:self action:@selector(runSearch)];
    self.russianOnlyBox.state = NSControlStateValueOn;
    self.searchSpinner = [self spinner];
    NSStackView *searchRow = [NSStackView stackViewWithViews:@[ self.searchField, self.russianOnlyBox, self.searchSpinner ]];
    searchRow.spacing = 12;
    self.searchStatus = [self introLabel:@""];
    NSStackView *searchList;
    NSView *searchTab = [self tabWithHeader:@[ searchRow, self.searchStatus ] list:&searchList];
    self.searchList = searchList;

    NSButton *chooseFolder = [NSButton buttonWithTitle:@"Выбрать папку…" target:self action:@selector(chooseFolder:)];
    NSButton *rescan = [NSButton buttonWithTitle:@"Искать снова" target:self action:@selector(scanLocal)];
    self.localSpinner = [self spinner];
    self.localStatus = [NSTextField labelWithString:@""];
    self.localStatus.font = [NSFont systemFontOfSize:12];
    self.localStatus.textColor = NSColor.secondaryLabelColor;
    NSStackView *localRow = [NSStackView stackViewWithViews:@[ chooseFolder, rescan, self.localSpinner, self.localStatus ]];
    localRow.spacing = 8;
    NSStackView *localList;
    NSView *localTab = [self
        tabWithHeader:@[
            [self introLabel:@"Модели, которые уже лежат на диске: кэш Hugging Face, папки whisper.cpp и sherpa-onnx. "
                             @"Приложение их только читает и использует на месте, ничего не копирует и не меняет."],
            localRow
        ]
                 list:&localList];
    self.localList = localList;

    NSStackView *mineList;
    NSView *mineTab = [self
        tabWithHeader:@[ [self introLabel:@"Модели, которыми можно диктовать на этом Mac. Основная — та, через которую "
                                          @"сейчас идёт распознавание."] ]
                 list:&mineList];
    self.mineList = mineList;

    for (NSView *view in @[ recommendedTab, searchTab, localTab, mineTab ]) {
        NSTabViewItem *item = [[NSTabViewItem alloc] init];
        item.view = view;
        [self.tabView addTabViewItem:item];
    }

    [content addSubview:self.tabs];
    [content addSubview:self.tabView];
    [NSLayoutConstraint activateConstraints:@[
        [self.tabs.topAnchor constraintEqualToAnchor:content.topAnchor constant:14],
        [self.tabs.centerXAnchor constraintEqualToAnchor:content.centerXAnchor],
        [self.tabView.topAnchor constraintEqualToAnchor:self.tabs.bottomAnchor constant:12],
        [self.tabView.leadingAnchor constraintEqualToAnchor:content.leadingAnchor],
        [self.tabView.trailingAnchor constraintEqualToAnchor:content.trailingAnchor],
        [self.tabView.bottomAnchor constraintEqualToAnchor:content.bottomAnchor],
    ]];
}

#pragma mark - Tabs

- (SPModelTab)currentTab {
    return (SPModelTab)self.tabs.selectedSegment;
}

- (void)tabChanged:(id)sender {
    [self.tabView selectTabViewItemAtIndex:self.tabs.selectedSegment];
    [self reloadVisibleTab];
}

- (void)showTab:(SPModelTab)tab {
    self.tabs.selectedSegment = tab;
    [self tabChanged:nil];
}

- (void)reloadVisibleTab {
    switch (self.currentTab) {
    case SPModelTabRecommended:
        [self loadRecommended];
        break;
    case SPModelTabSearch:
        if (!self.searchedOnce) [self runSearch];
        [self rebuildSearch];
        break;
    case SPModelTabThisMac:
        if (!self.scannedOnce) [self scanLocal];
        [self rebuildLocal];
        break;
    case SPModelTabMine:
        [self loadMine];
        break;
    }
    if (self.currentTab == SPModelTabSearch) [self.window makeFirstResponder:self.searchField];
}

- (void)rebuildVisibleTab {
    switch (self.currentTab) {
    case SPModelTabRecommended:
        [self rebuildRecommended];
        break;
    case SPModelTabSearch:
        [self rebuildSearch];
        break;
    case SPModelTabThisMac:
        [self rebuildLocal];
        break;
    case SPModelTabMine:
        [self rebuildMine];
        break;
    }
}

- (NSStackView *)visibleList {
    switch (self.currentTab) {
    case SPModelTabRecommended:
        return self.recommendedList;
    case SPModelTabSearch:
        return self.searchList;
    case SPModelTabThisMac:
        return self.localList;
    case SPModelTabMine:
        return self.mineList;
    }
    return self.recommendedList;
}

- (void)clear:(NSStackView *)stack {
    for (NSView *view in stack.arrangedSubviews.copy) {
        [stack removeArrangedSubview:view];
        [view removeFromSuperview];
    }
}

#pragma mark - Shared card parts

- (BOOL)isPrimary:(NSString *)modelPath provider:(NSString *)provider {
    NSString *configProvider = configValue(@"asr.provider");
    NSString *configModel =
        configProvider.length > 0 ? configValue([NSString stringWithFormat:@"asr.%@.model", configProvider]) : @"";
    return [SPModelCardPresenter isPrimaryModel:modelPath
                                       provider:provider
                                 configProvider:configProvider
                                    configModel:configModel];
}

- (void)addEngineBadge:(NSString *)provider primary:(BOOL)primary toCard:(SPModelCardView *)card {
    if (text(provider).length > 0) {
        [card addBadge:[SPModelCardPresenter engineNameForProvider:provider] color:NSColor.controlAccentColor];
    }
    if (primary) [card addBadge:@"основная" color:NSColor.systemGreenColor];
}

/// Progress, how the last download ended, or the card's message.
- (void)applyStateOfPath:(NSString *)path messageKey:(NSString *)key toCard:(SPModelCardView *)card {
    SPModelDownloads *downloads = [SPModelDownloads shared];
    SPDownloadMeter *meter = [downloads meterForPath:path];
    if (meter) {
        double fraction = meter.totalBytes > 0 ? (double)meter.downloadedBytes / meter.totalBytes : 0;
        [card setProgress:fraction text:[meter progressText]];
    } else {
        [card setProgress:-1 text:nil];
    }
    NSArray *message = self.messages[path ?: @""] ?: self.messages[key ?: @""];
    NSString *problem = [downloads problemForPath:path];
    if (meter) {
        [card setText:@"Скачиваю. Окно можно закрыть: загрузка продолжится." color:NSColor.secondaryLabelColor
              ofLabel:card.resultLabel];
    } else if (message) {
        [card setText:message[0] color:message[1] ofLabel:card.resultLabel];
    } else if (problem) {
        [card setText:problem color:NSColor.systemRedColor ofLabel:card.resultLabel];
    } else if ([downloads finishedForPath:path]) {
        [card setText:@"Скачано: все файлы совпали с контрольными суммами." color:NSColor.systemGreenColor
              ofLabel:card.resultLabel];
    }
}

- (NSButton *)addAction:(SPModelAction)action
                 toCard:(SPModelCardView *)card
             identifier:(NSString *)identifier
               selector:(SEL)selector {
    if (action == SPModelActionNone) return nil;
    NSButton *button = [card addButton:[SPModelCardPresenter titleForAction:action] target:self action:selector];
    button.tag = action;
    button.identifier = identifier;
    return button;
}

- (void)addCheckButtonToCard:(SPModelCardView *)card path:(NSString *)path {
    NSButton *check = [card addButton:@"Проверить" target:self action:@selector(checkPressed:)];
    check.identifier = path;
    check.toolTip = @"Распознать встроенную двухсекундную запись этой моделью";
    check.enabled = ![self.busy containsObject:[@"check:" stringByAppendingString:path]];
}

- (BOOL)hasRoomFor:(uint64_t)bytes {
    NSURL *url = [NSURL fileURLWithPath:SPAppDataDirectory()];
    NSNumber *value = nil;
    if (![url getResourceValue:&value forKey:NSURLVolumeAvailableCapacityForImportantUsageKey error:NULL] || !value) {
        return YES;
    }
    if ((uint64_t)value.longLongValue >= bytes) return YES;
    NSAlert *alert = [[NSAlert alloc] init];
    alert.messageText = @"Не хватает места на диске";
    alert.informativeText = [NSString stringWithFormat:@"Модели нужно %@, а свободно %@.",
                                                       [SPDownloadMeter stringForBytes:bytes],
                                                       [SPDownloadMeter stringForBytes:(uint64_t)value.longLongValue]];
    [alert beginSheetModalForWindow:self.window completionHandler:nil];
    return NO;
}

- (void)startDownload:(NSString *)path totalBytes:(uint64_t)total {
    if (![self hasRoomFor:total]) return;
    [self.messages removeObjectForKey:path];
    [[SPModelDownloads shared] startModelAtPath:path totalBytes:total bridge:self.bridge];
    [self rebuildVisibleTab];
}

- (void)downloadsChanged:(NSNotification *)notification {
    NSString *path = notification.userInfo[@"path"];
    if ([[SPModelDownloads shared] isDownloading:path]) {
        for (NSView *view in self.visibleList.arrangedSubviews) {
            if ([view isKindOfClass:[SPModelCardView class]] && [((SPModelCardView *)view).key isEqualToString:path]) {
                [self applyStateOfPath:path messageKey:nil toCard:(SPModelCardView *)view];
            }
        }
        return;
    }
    [self.delegate modelBrowserDidChangeModels];
    if (self.window.visible) [self reloadVisibleTab];
}

#pragma mark - Actions every tab shares

- (void)checkPressed:(NSButton *)sender {
    NSString *path = sender.identifier;
    NSString *busyKey = [@"check:" stringByAppendingString:path];
    if (path.length == 0 || [self.busy containsObject:busyKey]) return;
    [self.busy addObject:busyKey];
    self.messages[path] = @[ @"Проверяю: распознаю встроенную запись этой моделью…", NSColor.secondaryLabelColor ];
    [self rebuildVisibleTab];
    __weak typeof(self) weakSelf = self;
    [SPLocalModelCheck checkModelAtPath:path
                             completion:^(NSDictionary *result) {
                                 typeof(self) strongSelf = weakSelf;
                                 if (!strongSelf) return;
                                 [strongSelf.busy removeObject:busyKey];
                                 strongSelf.messages[path] = @[
                                     [SPHfModelPresenter summaryForCheckResult:result],
                                     [result[@"ok"] boolValue] ? NSColor.systemGreenColor : NSColor.systemRedColor
                                 ];
                                 [strongSelf rebuildVisibleTab];
                             }];
}

- (void)makePrimary:(NSString *)path provider:(NSString *)provider {
    id<SPModelBrowserDelegate> delegate = self.delegate;
    if (!delegate) {
        self.messages[path] = @[ @"Выберите модель в Настройках → Распознавание.", NSColor.systemOrangeColor ];
    } else if ([delegate modelBrowserMakePrimaryModel:path provider:provider]) {
        self.messages[path] = @[ @"Готово: диктовка теперь идёт через эту модель.", NSColor.systemGreenColor ];
    } else {
        self.messages[path] = @[ @"Не удалось сохранить настройки.", NSColor.systemRedColor ];
    }
    [self reloadVisibleTab];
}

- (void)openURLString:(id)string {
    NSURL *url = [string isKindOfClass:[NSString class]] ? [NSURL URLWithString:string] : nil;
    if ([url.scheme isEqualToString:@"https"] || [url.scheme isEqualToString:@"http"]) {
        [[NSWorkspace sharedWorkspace] openURL:url];
    }
}

#pragma mark - Рекомендуемые

- (void)loadRecommended {
    id parsed = ownedJSON(sp_models_recommended_json());
    self.recommended = list(parsed);
    [self rebuildRecommended];
}

- (NSDictionary *)recommendedWithId:(NSString *)identifier {
    for (NSDictionary *entry in self.recommended) {
        if ([text(entry[@"id"]) isEqualToString:identifier]) return entry;
    }
    return nil;
}

- (void)rebuildRecommended {
    [self clear:self.recommendedList];
    for (NSDictionary *entry in self.recommended) {
        NSString *identifier = text(entry[@"id"]);
        NSString *path = text(entry[@"model_path"]);
        NSString *engine = text(entry[@"engine"]);
        BOOL installed = [entry[@"status"] integerValue] == 2;
        BOOL primary = installed && [self isPrimary:path provider:engine];
        BOOL downloading = [[SPModelDownloads shared] isDownloading:path];

        SPModelCardView *card = [[SPModelCardView alloc] initWithWidth:kCardWidth];
        card.key = path.length > 0 ? path : identifier;
        card.titleLabel.stringValue = text(entry[@"name"]);
        [self addEngineBadge:engine primary:primary toCard:card];
        if (installed && !primary) [card addBadge:@"скачана" color:NSColor.secondaryLabelColor];
        [card setText:text(entry[@"description"]) ofLabel:card.bodyLabel];
        [card setText:[SPModelCardPresenter detailLineForRecommended:entry] ofLabel:card.detailLabel];
        if (![entry[@"engine_ready"] boolValue]) [card setText:text(entry[@"engine_note"]) ofLabel:card.noteLabel];
        [self applyStateOfPath:path messageKey:identifier toCard:card];

        SPModelAction action = [SPModelCardPresenter actionForRecommended:entry downloading:downloading primary:primary];
        NSButton *button = [self addAction:action
                                    toCard:card
                                identifier:identifier
                                  selector:@selector(recommendedPressed:)];
        button.enabled = ![self.busy containsObject:identifier];
        if (installed && !downloading && [entry[@"engine_ready"] boolValue]) [self addCheckButtonToCard:card path:path];
        [self.recommendedList addArrangedSubview:card];
    }
}

- (void)recommendedPressed:(NSButton *)sender {
    NSDictionary *entry = [self recommendedWithId:sender.identifier];
    if (!entry) return;
    NSString *path = text(entry[@"model_path"]);
    uint64_t size = [entry[@"size"] unsignedLongLongValue];
    switch ((SPModelAction)sender.tag) {
    case SPModelActionStop:
        [[SPModelDownloads shared] stopModelAtPath:path bridge:self.bridge];
        break;
    case SPModelActionMakePrimary:
        [self makePrimary:path provider:text(entry[@"engine"])];
        break;
    case SPModelActionResume:
        [self startDownload:path totalBytes:size];
        break;
    case SPModelActionDownload:
        [self installRecommended:entry];
        break;
    default:
        break;
    }
}

- (void)installRecommended:(NSDictionary *)entry {
    NSString *identifier = text(entry[@"id"]);
    uint64_t size = [entry[@"size"] unsignedLongLongValue];
    if (![self hasRoomFor:size]) return;
    [self.busy addObject:identifier];
    self.messages[identifier] = @[ @"Готовлю модель…", NSColor.secondaryLabelColor ];
    [self rebuildRecommended];
    __weak typeof(self) weakSelf = self;
    sp_models_install_recommended_start(identifier.UTF8String, modelCallback, retainedBox(^(NSDictionary *result) {
                                            typeof(self) strongSelf = weakSelf;
                                            if (!strongSelf) return;
                                            [strongSelf.busy removeObject:identifier];
                                            NSString *path = text(result[@"model_path"]);
                                            if (path.length == 0) {
                                                strongSelf.messages[identifier] =
                                                    @[ text(result[@"error"]), NSColor.systemRedColor ];
                                                [strongSelf rebuildVisibleTab];
                                                return;
                                            }
                                            [strongSelf.messages removeObjectForKey:identifier];
                                            [strongSelf.delegate modelBrowserDidChangeModels];
                                            id parsed = ownedJSON(sp_models_recommended_json());
                                            strongSelf.recommended = list(parsed);
                                            [strongSelf startDownload:path totalBytes:size];
                                        }));
}

#pragma mark - Поиск

- (void)controlTextDidChange:(NSNotification *)notification {
    if (notification.object != self.searchField) return;
    [self.searchTimer invalidate];
    self.searchTimer = [NSTimer scheduledTimerWithTimeInterval:kSearchDelay
                                                        target:self
                                                      selector:@selector(runSearch)
                                                      userInfo:nil
                                                       repeats:NO];
}

- (void)searchSubmitted:(id)sender {
    [self.searchTimer invalidate];
    [self runSearch];
}

- (void)runSearch {
    [self.searchTimer invalidate];
    self.searchedOnce = YES;
    self.detailsRepo = nil;
    self.detailsGeneration++;
    NSUInteger generation = ++self.searchGeneration;
    [self.searchSpinner startAnimation:nil];
    self.searchStatus.stringValue = @"Ищу на Hugging Face…";
    self.searchStatus.textColor = NSColor.secondaryLabelColor;
    NSString *query = self.searchField.stringValue ?: @"";
    BOOL russianOnly = self.russianOnlyBox.state == NSControlStateValueOn;
    __weak typeof(self) weakSelf = self;
    sp_hf_search_start(query.UTF8String, russianOnly, modelCallback, retainedBox(^(NSDictionary *result) {
                           typeof(self) strongSelf = weakSelf;
                           if (!strongSelf || strongSelf.searchGeneration != generation) return;
                           [strongSelf.searchSpinner stopAnimation:nil];
                           NSString *error = text(result[@"error"]);
                           if (error.length > 0) {
                               strongSelf.searchStatus.stringValue =
                                   strongSelf.searchItems.count > 0
                                       ? [error stringByAppendingString:@" Ниже — результаты прошлого поиска."]
                                       : error;
                               strongSelf.searchStatus.textColor = NSColor.systemRedColor;
                           } else {
                               strongSelf.searchItems = list(result[@"items"]);
                               strongSelf.searchStatus.stringValue = [SPModelCardPresenter summaryForSearchResults:result];
                               strongSelf.searchStatus.textColor = NSColor.secondaryLabelColor;
                           }
                           [strongSelf rebuildSearch];
                       }));
    [self rebuildSearch];
}

- (NSColor *)colorForVerdict:(NSString *)verdict {
    if ([verdict isEqualToString:@"fits"]) return NSColor.systemGreenColor;
    if ([verdict isEqualToString:@"needs_engine"]) return NSColor.systemOrangeColor;
    return NSColor.secondaryLabelColor;
}

- (void)rebuildSearch {
    [self clear:self.searchList];
    if (self.detailsRepo) {
        [self buildDetails];
        return;
    }
    for (NSDictionary *item in self.searchItems) {
        NSString *repo = text(item[@"repo"]);
        NSString *verdict = text(item[@"verdict"]);
        SPModelCardView *card = [[SPModelCardView alloc] initWithWidth:kCardWidth];
        card.key = repo;
        card.titleLabel.stringValue = repo;
        [card addBadge:text(item[@"badge"]) color:[self colorForVerdict:verdict]];
        [card setText:[SPModelCardPresenter detailLineForSearchItem:item] ofLabel:card.detailLabel];
        [card setText:text(item[@"reason"])
                color:[verdict isEqualToString:@"no"] ? NSColor.secondaryLabelColor : NSColor.systemOrangeColor
              ofLabel:card.noteLabel];
        NSButton *more = [card addButton:@"Подробнее" target:self action:@selector(showDetailsFor:)];
        more.identifier = repo;
        [self.searchList addArrangedSubview:card];
    }
}

- (void)showDetailsFor:(NSButton *)sender {
    [self showDetailsOfRepo:sender.identifier];
}

- (void)showDetailsOfRepo:(NSString *)repo {
    if (repo.length == 0) return;
    self.detailsRepo = repo;
    self.inspection = nil;
    self.detailsError = nil;
    self.detailsModelPath = nil;
    self.detailsVariant = 0;
    NSUInteger generation = ++self.detailsGeneration;
    [self rebuildSearch];
    __weak typeof(self) weakSelf = self;
    sp_hf_inspect_start(repo.UTF8String, "", modelCallback, retainedBox(^(NSDictionary *result) {
                            typeof(self) strongSelf = weakSelf;
                            if (!strongSelf || strongSelf.detailsGeneration != generation) return;
                            NSString *error = text(result[@"error"]);
                            if (error.length > 0) {
                                strongSelf.detailsError = error;
                            } else {
                                strongSelf.inspection = result;
                            }
                            [strongSelf rebuildSearch];
                        }));
}

- (void)backToResults:(id)sender {
    self.detailsRepo = nil;
    self.detailsGeneration++;
    [self rebuildSearch];
}

- (NSDictionary *)detailsVariantInfo {
    NSArray<NSDictionary *> *variants = [SPHfModelPresenter variantsOfInspection:self.inspection ?: @{}];
    if (variants.count == 0) return nil;
    NSInteger index = self.detailsVariant;
    return variants[index >= 0 && index < (NSInteger)variants.count ? (NSUInteger)index : 0];
}

- (void)buildDetails {
    NSButton *back = [NSButton buttonWithTitle:@"← К результатам" target:self action:@selector(backToResults:)];
    back.bezelStyle = NSBezelStyleRecessed;
    [self.searchList addArrangedSubview:back];

    NSDictionary *inspection = self.inspection;
    NSDictionary *variant = [self detailsVariantInfo];
    NSString *path = self.detailsModelPath ?: @"";
    SPModelCardView *card = [[SPModelCardView alloc] initWithWidth:kCardWidth];
    card.key = path.length > 0 ? path : @"details";
    card.titleLabel.stringValue = self.detailsRepo;
    [self.searchList addArrangedSubview:card];

    if (!inspection) {
        [card setText:self.detailsError ?: @"Читаю репозиторий на Hugging Face…"
                color:self.detailsError ? NSColor.systemRedColor : NSColor.secondaryLabelColor
              ofLabel:card.resultLabel];
        return;
    }
    NSString *provider = text(variant[@"backend"]);
    BOOL installed = path.length > 0 && [self.bridge modelStatus:path mode:SPModelVerifyCacheOnly] == 2;
    BOOL primary = installed && [self isPrimary:path provider:provider];
    [self addEngineBadge:provider primary:primary toCard:card];

    NSArray<NSDictionary *> *variants = [SPHfModelPresenter variantsOfInspection:inspection];
    if (variants.count > 1) {
        NSPopUpButton *popup = [[NSPopUpButton alloc] initWithFrame:NSZeroRect pullsDown:NO];
        for (NSDictionary *option in variants) [popup addItemWithTitle:[SPHfModelPresenter menuTitleForVariant:option]];
        [popup selectItemAtIndex:MIN(self.detailsVariant, (NSInteger)variants.count - 1)];
        popup.target = self;
        popup.action = @selector(detailsVariantChanged:);
        popup.enabled = ![[SPModelDownloads shared] isDownloading:path];
        [card.extras addArrangedSubview:popup];
        card.extras.hidden = NO;
    }
    NSMutableArray<NSString *> *rows = [NSMutableArray array];
    for (NSArray<NSString *> *row in [SPHfModelPresenter rowsForInspection:inspection variant:variant]) {
        [rows addObject:[NSString stringWithFormat:@"%@: %@", row[0], row[1]]];
    }
    [card setText:[rows componentsJoinedByString:@"\n"] ofLabel:card.bodyLabel];

    NSMutableArray<NSString *> *notes = [NSMutableArray array];
    NSString *warnings = [SPHfModelPresenter warningsTextForInspection:inspection];
    if (warnings.length > 0) [notes addObject:warnings];
    if (variant && ![variant[@"engine_ready"] boolValue]) [notes addObject:text(variant[@"engine_note"])];
    NSString *problem = [SPHfModelPresenter problemTextForInspection:inspection];
    if (problem) [notes addObject:problem];
    [card setText:[notes componentsJoinedByString:@"\n\n"]
            color:problem ? NSColor.systemRedColor : NSColor.systemOrangeColor
          ofLabel:card.noteLabel];

    [card addLink:@"Открыть на Hugging Face ↗" target:self action:@selector(openDetailsPage:)];
    if (text(dict(inspection[@"license"])[@"link"]).length > 0) {
        [card addLink:@"Условия лицензии ↗" target:self action:@selector(openDetailsLicense:)];
    }
    NSString *phrase = text(inspection[@"search_phrase"]);
    if (problem && phrase.length > 0 && ![dict(inspection[@"problem"])[@"code"] isEqual:@"gated"]) {
        [card addLink:[NSString stringWithFormat:@"Искать «%@» ↗", phrase] target:self action:@selector(openDetailsSearch:)];
    }
    if (problem) {
        for (NSString *alternative in list(inspection[@"alternatives"])) {
            if (![alternative isKindOfClass:[NSString class]]) continue;
            NSButton *link = [card addLink:alternative target:self action:@selector(showAlternative:)];
            link.identifier = alternative;
        }
    }

    [self applyStateOfPath:path messageKey:@"details" toCard:card];
    if (!variant || ![variant[@"engine_ready"] boolValue]) return;
    BOOL downloading = [[SPModelDownloads shared] isDownloading:path];
    SPModelAction action;
    if (downloading) {
        action = SPModelActionStop;
    } else if (installed) {
        action = primary ? SPModelActionNone : SPModelActionMakePrimary;
    } else if (path.length > 0 && [[SPModelDownloads shared] problemForPath:path]) {
        action = SPModelActionResume;
    } else {
        action = SPModelActionDownload;
    }
    NSButton *button = [self addAction:action toCard:card identifier:@"details" selector:@selector(detailsPressed:)];
    button.enabled = ![self.busy containsObject:@"details"];
    if (installed) [self addCheckButtonToCard:card path:path];
}

- (void)detailsVariantChanged:(NSPopUpButton *)sender {
    self.detailsVariant = sender.indexOfSelectedItem;
    self.detailsModelPath = nil;
    [self rebuildSearch];
}

- (void)openDetailsPage:(id)sender {
    [self openURLString:self.inspection[@"page_url"]];
}

- (void)openDetailsLicense:(id)sender {
    [self openURLString:dict(self.inspection[@"license"])[@"link"]];
}

- (void)openDetailsSearch:(id)sender {
    [self openURLString:self.inspection[@"search_url"]];
}

- (void)showAlternative:(NSButton *)sender {
    [self showDetailsOfRepo:sender.identifier];
}

- (void)detailsPressed:(NSButton *)sender {
    NSDictionary *variant = [self detailsVariantInfo];
    NSString *path = self.detailsModelPath;
    uint64_t total = [variant[@"total_size"] unsignedLongLongValue];
    switch ((SPModelAction)sender.tag) {
    case SPModelActionStop:
        [[SPModelDownloads shared] stopModelAtPath:path bridge:self.bridge];
        return;
    case SPModelActionMakePrimary:
        [self makePrimary:path provider:text(variant[@"backend"])];
        return;
    case SPModelActionResume:
        [self startDownload:path totalBytes:total];
        return;
    case SPModelActionDownload:
        break;
    default:
        return;
    }
    if (![self hasRoomFor:total]) return;
    NSUInteger generation = self.detailsGeneration;
    [self.busy addObject:@"details"];
    self.messages[@"details"] = @[ @"Готовлю папку модели…", NSColor.secondaryLabelColor ];
    [self rebuildSearch];
    __weak typeof(self) weakSelf = self;
    uint64_t token = [self.inspection[@"token"] unsignedLongLongValue];
    sp_hf_install_start(token, text(variant[@"id"]).UTF8String, modelCallback, retainedBox(^(NSDictionary *result) {
                            typeof(self) strongSelf = weakSelf;
                            if (!strongSelf) return;
                            [strongSelf.busy removeObject:@"details"];
                            if (strongSelf.detailsGeneration != generation) return;
                            NSString *installed = text(result[@"model_path"]);
                            if (installed.length == 0) {
                                strongSelf.messages[@"details"] = @[
                                    text(result[@"error"]).length > 0 ? text(result[@"error"]) : @"Не удалось подготовить модель.",
                                    NSColor.systemRedColor
                                ];
                                [strongSelf rebuildSearch];
                                return;
                            }
                            [strongSelf.messages removeObjectForKey:@"details"];
                            strongSelf.detailsModelPath = installed;
                            [strongSelf.delegate modelBrowserDidChangeModels];
                            [strongSelf startDownload:installed totalBytes:total];
                        }));
}

#pragma mark - На этом Mac

- (void)scanLocal {
    if (self.scanning) return;
    self.scanning = YES;
    self.scannedOnce = YES;
    [self.localSpinner startAnimation:nil];
    self.localStatus.stringValue = @"Ищу модели на этом Mac…";
    __weak typeof(self) weakSelf = self;
    sp_models_scan_local_start(modelCallback, retainedBox(^(NSDictionary *result) {
                                   typeof(self) strongSelf = weakSelf;
                                   if (!strongSelf) return;
                                   strongSelf.scanning = NO;
                                   [strongSelf.localSpinner stopAnimation:nil];
                                   strongSelf.finds = list(result[@"finds"]);
                                   [strongSelf updateLocalStatus];
                                   [strongSelf rebuildLocal];
                               }));
}

- (void)updateLocalStatus {
    NSUInteger count = [self allFinds].count;
    self.localStatus.stringValue = count > 0 ? [NSString stringWithFormat:@"Найдено: %lu.", (unsigned long)count]
                                             : @"Моделей распознавания не нашлось. Если модель лежит в другой "
                                               @"папке, выберите её.";
}

- (void)chooseFolder:(id)sender {
    NSOpenPanel *panel = [NSOpenPanel openPanel];
    panel.canChooseDirectories = YES;
    panel.canChooseFiles = NO;
    panel.allowsMultipleSelection = NO;
    panel.prompt = @"Выбрать";
    panel.message = @"Выберите папку, в которой лежит модель распознавания (или папку с несколькими моделями).";
    __weak typeof(self) weakSelf = self;
    [panel beginSheetModalForWindow:self.window
                  completionHandler:^(NSModalResponse response) {
                      NSString *path = panel.URL.path;
                      if (response == NSModalResponseOK && path.length > 0) [weakSelf inspectFolder:path];
                  }];
}

- (void)inspectFolder:(NSString *)path {
    [self.localSpinner startAnimation:nil];
    self.localStatus.stringValue = @"Читаю папку…";
    __weak typeof(self) weakSelf = self;
    sp_models_inspect_folder_start(path.UTF8String, modelCallback, retainedBox(^(NSDictionary *result) {
                                       typeof(self) strongSelf = weakSelf;
                                       if (!strongSelf) return;
                                       [strongSelf.localSpinner stopAnimation:nil];
                                       NSString *error = text(result[@"error"]);
                                       if (error.length > 0) {
                                           strongSelf.localStatus.stringValue = error;
                                           return;
                                       }
                                       NSArray *found = list(result[@"finds"]);
                                       for (NSDictionary *find in found.reverseObjectEnumerator) {
                                           [strongSelf replaceFind:find inArray:strongSelf.pickedFinds insert:YES];
                                       }
                                       [strongSelf updateLocalStatus];
                                       [strongSelf rebuildLocal];
                                   }));
}

/// Picked folders first, then what the scan found; one card per folder.
- (NSArray<NSDictionary *> *)allFinds {
    NSMutableArray *all = [NSMutableArray arrayWithArray:self.pickedFinds];
    NSMutableSet *seen = [NSMutableSet set];
    for (NSDictionary *find in self.pickedFinds) [seen addObject:text(find[@"location"])];
    for (NSDictionary *find in self.finds) {
        if (![seen containsObject:text(find[@"location"])]) [all addObject:find];
    }
    return all;
}

- (void)replaceFind:(NSDictionary *)find inArray:(NSMutableArray *)array insert:(BOOL)insert {
    NSString *location = text(find[@"location"]);
    for (NSUInteger i = 0; i < array.count; i++) {
        if ([text(array[i][@"location"]) isEqualToString:location]) {
            array[i] = find;
            return;
        }
    }
    if (insert) [array insertObject:find atIndex:0];
}

- (NSDictionary *)findAtLocation:(NSString *)location {
    for (NSDictionary *find in [self allFinds]) {
        if ([text(find[@"location"]) isEqualToString:location]) return find;
    }
    return nil;
}

- (NSDictionary *)chosenVariantOfFind:(NSDictionary *)find {
    NSArray *variants = list(find[@"variants"]);
    if (variants.count == 0) return nil;
    NSInteger index = [self.variantChoice[text(find[@"location"])] integerValue];
    return dict(variants[index >= 0 && index < (NSInteger)variants.count ? (NSUInteger)index : 0]);
}

- (void)rebuildLocal {
    [self clear:self.localList];
    NSString *home = NSHomeDirectory();
    for (NSDictionary *find in [self allFinds]) {
        NSString *location = text(find[@"location"]);
        NSArray *variants = list(find[@"variants"]);
        NSDictionary *variant = [self chosenVariantOfFind:find];
        NSString *provider = text(variant[@"backend"]);
        NSString *added = [SPModelCardPresenter addedModelPathForFind:find variant:variant];
        BOOL primary = added && [self isPrimary:added provider:provider];

        SPModelCardView *card = [[SPModelCardView alloc] initWithWidth:kCardWidth];
        card.key = added ?: location;
        card.titleLabel.stringValue = text(find[@"name"]);
        [self addEngineBadge:provider primary:primary toCard:card];
        if (added && !primary) [card addBadge:@"в моих моделях" color:NSColor.secondaryLabelColor];
        [card setText:[SPModelCardPresenter detailLineForFind:find variant:variant] ofLabel:card.bodyLabel];
        [card setText:[SPModelCardPresenter sourceLineForFind:find home:home] ofLabel:card.detailLabel];
        card.detailLabel.toolTip = location;

        if (variants.count > 1) {
            NSPopUpButton *popup = [[NSPopUpButton alloc] initWithFrame:NSZeroRect pullsDown:NO];
            for (NSDictionary *option in variants) {
                [popup addItemWithTitle:[SPHfModelPresenter menuTitleForVariant:dict(option)]];
            }
            [popup selectItemAtIndex:MIN([self.variantChoice[location] integerValue], (NSInteger)variants.count - 1)];
            popup.identifier = location;
            popup.target = self;
            popup.action = @selector(localVariantChanged:);
            [card.extras addArrangedSubview:popup];
            card.extras.hidden = NO;
        }

        NSMutableArray<NSString *> *notes = [NSMutableArray array];
        for (NSString *warning in list(find[@"warnings"])) {
            if ([warning isKindOfClass:[NSString class]]) [notes addObject:warning];
        }
        if (variant && ![variant[@"engine_ready"] boolValue]) [notes addObject:text(variant[@"engine_note"])];
        if (!variant) [notes addObject:text(dict(find[@"problem"])[@"message"])];
        [card setText:[notes componentsJoinedByString:@"\n"] ofLabel:card.noteLabel];

        NSDictionary *alternative = [self catalogEntry:text(find[@"alternative"])];
        if (alternative) {
            NSButton *link =
                [card addLink:[NSString stringWithFormat:@"Та же модель для этой сборки: «%@» в «Рекомендуемых»",
                                                         text(alternative[@"name"])]
                       target:self
                       action:@selector(showRecommendedTab:)];
            link.identifier = text(alternative[@"id"]);
        }

        if (added) {
            [self applyStateOfPath:added messageKey:location toCard:card];
            if (!self.messages[added]) {
                [card setText:@"Добавлена в «Мои модели». Файлы остаются в этой папке." color:NSColor.secondaryLabelColor
                      ofLabel:card.resultLabel];
            }
        } else {
            [self applyStateOfPath:location messageKey:location toCard:card];
        }
        SPModelAction action = [SPModelCardPresenter actionForFind:find variant:variant primary:primary];
        [self addAction:action
                 toCard:card
             identifier:action == SPModelActionMakePrimary ? added : location
               selector:@selector(localPressed:)];
        if (added) [self addCheckButtonToCard:card path:added];
        [self.localList addArrangedSubview:card];
    }
}

/// A catalog entry even when «Рекомендуемые» has not been opened yet.
- (NSDictionary *)catalogEntry:(NSString *)identifier {
    if (identifier.length == 0) return nil;
    if (self.recommended.count == 0) self.recommended = list(ownedJSON(sp_models_recommended_json()));
    return [self recommendedWithId:identifier];
}

- (void)showRecommendedTab:(id)sender {
    [self showTab:SPModelTabRecommended];
}

- (void)localVariantChanged:(NSPopUpButton *)sender {
    self.variantChoice[sender.identifier] = @(sender.indexOfSelectedItem);
    [self rebuildLocal];
}

- (void)localPressed:(NSButton *)sender {
    if (sender.tag == SPModelActionMakePrimary) {
        for (NSDictionary *find in [self allFinds]) {
            NSDictionary *variant = [self chosenVariantOfFind:find];
            if ([[SPModelCardPresenter addedModelPathForFind:find variant:variant] isEqualToString:sender.identifier]) {
                [self makePrimary:sender.identifier provider:text(variant[@"backend"])];
                return;
            }
        }
        return;
    }
    NSDictionary *find = [self findAtLocation:sender.identifier];
    NSDictionary *variant = [self chosenVariantOfFind:find];
    if (!find || !variant) return;
    NSString *location = text(find[@"location"]);
    NSDictionary *result = dict(ownedJSON(sp_models_add_external_json(location.UTF8String, text(variant[@"id"]).UTF8String)));
    NSString *path = text(result[@"model_path"]);
    if (path.length == 0) {
        self.messages[location] =
            @[ text(result[@"error"]).length > 0 ? text(result[@"error"]) : @"Не удалось добавить модель.", NSColor.systemRedColor ];
        [self rebuildLocal];
        return;
    }
    NSMutableDictionary *updated = [find mutableCopy];
    NSMutableDictionary *added = [dict(find[@"added"]) mutableCopy];
    added[text(variant[@"id"])] = path;
    updated[@"added"] = added;
    NSMutableArray *finds = [self.finds mutableCopy];
    [self replaceFind:updated inArray:finds insert:NO];
    self.finds = finds;
    [self replaceFind:updated inArray:self.pickedFinds insert:NO];
    [self.messages removeObjectForKey:location];
    [self.delegate modelBrowserDidChangeModels];
    [self rebuildLocal];
}

#pragma mark - Мои модели

- (void)loadMine {
    self.mine = list(ownedJSON(sp_models_list_json()));
    [self rebuildMine];
}

- (NSDictionary *)mineWithPath:(NSString *)path {
    for (NSDictionary *model in self.mine) {
        if ([text(model[@"model_path"]) isEqualToString:path]) return model;
    }
    return nil;
}

- (void)rebuildMine {
    [self clear:self.mineList];
    if (self.mine.count == 0) {
        NSTextField *empty = [self introLabel:@"Пока здесь пусто. Скачайте модель во вкладке «Рекомендуемые», найдите "
                                              @"её в «Поиске» или подключите ту, что уже есть «На этом Mac»."];
        [self.mineList addArrangedSubview:empty];
        return;
    }
    NSString *home = NSHomeDirectory();
    for (NSDictionary *model in self.mine) {
        NSString *path = text(model[@"model_path"]);
        BOOL primary = [model[@"primary"] boolValue];
        BOOL downloading = [[SPModelDownloads shared] isDownloading:path];
        BOOL missing = [model[@"missing"] boolValue];
        BOOL ready = [model[@"status"] integerValue] == 2 && [model[@"engine_ready"] boolValue] && !missing;

        SPModelCardView *card = [[SPModelCardView alloc] initWithWidth:kCardWidth];
        card.key = path;
        card.titleLabel.stringValue = text(model[@"title"]);
        [self addEngineBadge:text(model[@"provider"]) primary:primary toCard:card];
        if ([model[@"status"] integerValue] == 1 && !downloading) {
            [card addBadge:@"скачана не полностью" color:NSColor.systemOrangeColor];
        }
        [card setText:[SPModelCardPresenter detailLineForModel:model] ofLabel:card.bodyLabel];
        [card setText:[SPModelCardPresenter originLineForModel:model home:home] ofLabel:card.detailLabel];
        card.detailLabel.toolTip = text(model[@"location"]).length > 0 ? text(model[@"location"]) : nil;
        if (missing) {
            [card setText:@"Папки с моделью больше нет. Верните её на место или уберите модель из списка."
                    color:NSColor.systemRedColor
                  ofLabel:card.noteLabel];
        } else if (![model[@"engine_ready"] boolValue]) {
            [card setText:text(model[@"engine_note"]) ofLabel:card.noteLabel];
        }
        [self applyStateOfPath:path messageKey:path toCard:card];

        SPModelAction action = [SPModelCardPresenter actionForModel:model downloading:downloading primary:primary];
        [self addAction:action toCard:card identifier:path selector:@selector(minePressed:)];
        if (ready && !downloading) [self addCheckButtonToCard:card path:path];
        NSButton *remove = [card addButton:[SPModelCardPresenter removalTitleForModel:model]
                                    target:self
                                    action:@selector(removePressed:)];
        remove.identifier = path;
        remove.enabled = !downloading;
        [self.mineList addArrangedSubview:card];
    }
}

- (void)minePressed:(NSButton *)sender {
    NSDictionary *model = [self mineWithPath:sender.identifier];
    if (!model) return;
    switch ((SPModelAction)sender.tag) {
    case SPModelActionStop:
        [[SPModelDownloads shared] stopModelAtPath:sender.identifier bridge:self.bridge];
        break;
    case SPModelActionMakePrimary:
        [self makePrimary:sender.identifier provider:text(model[@"provider"])];
        break;
    case SPModelActionResume:
        [self startDownload:sender.identifier totalBytes:[model[@"size"] unsignedLongLongValue]];
        break;
    default:
        break;
    }
}

- (void)removePressed:(NSButton *)sender {
    NSString *path = sender.identifier;
    NSDictionary *model = [self mineWithPath:path];
    if (!model) return;
    NSAlert *alert = [[NSAlert alloc] init];
    alert.messageText = [SPModelCardPresenter removalQuestionForModel:model];
    alert.informativeText = [SPModelCardPresenter removalExplanationForModel:model primary:[model[@"primary"] boolValue]];
    [alert addButtonWithTitle:[SPModelCardPresenter removalTitleForModel:model]];
    [alert addButtonWithTitle:@"Отмена"];
    alert.alertStyle = NSAlertStyleWarning;
    __weak typeof(self) weakSelf = self;
    [alert beginSheetModalForWindow:self.window
                  completionHandler:^(NSModalResponse response) {
                      typeof(self) strongSelf = weakSelf;
                      if (!strongSelf || response != NSAlertFirstButtonReturn) return;
                      NSDictionary *result = dict(ownedJSON(sp_models_remove_json(path.UTF8String)));
                      NSString *error = text(result[@"error"]);
                      if (error.length > 0 || result.count == 0) {
                          strongSelf.messages[path] =
                              @[ error.length > 0 ? error : @"Ядро не ответило.", NSColor.systemRedColor ];
                          [strongSelf rebuildMine];
                          return;
                      }
                      [strongSelf.messages removeObjectForKey:path];
                      [[SPModelDownloads shared] forgetPath:path];
                      strongSelf.finds = @[];
                      strongSelf.scannedOnce = NO;
                      [strongSelf.delegate modelBrowserDidChangeModels];
                      [strongSelf loadMine];
                  }];
}

#pragma mark - Closing

- (void)windowWillClose:(NSNotification *)notification {
    [self.searchTimer invalidate];
    BOOL othersRemain = NO;
    for (NSWindow *window in NSApp.windows) {
        if (window != self.window && window.visible && window.canBecomeKeyWindow) othersRemain = YES;
    }
    if (!othersRemain) [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
    // Released after AppKit has finished closing the window it owns.
    dispatch_async(dispatch_get_main_queue(), ^{
        if (sharedController == self) sharedController = nil;
    });
}

@end
