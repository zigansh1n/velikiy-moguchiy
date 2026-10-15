#import "SPRecentDictationsWindowController.h"
#import "SPHistoryManager.h"
#import "SPRetranscriber.h"
#import "SPClipboardManager.h"
#import "SPRecordingManager.h"
#import "SPDictationsCSV.h"
#import "SPAddWordDialog.h"
#import <AVFoundation/AVFoundation.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>

static const NSUInteger kRecentLimit = 500;
static const NSUInteger kSearchLimit = 1000;
static const NSTimeInterval kToastDuration = 1.5;
static const NSTimeInterval kRetranscribeToastDuration = 8.0;
static const NSTimeInterval kExportToastDuration = 8.0;
static NSString *const kTimeColumn = @"time";
static NSString *const kDurationColumn = @"duration";
static NSString *const kTypeColumn = @"type";
static NSString *const kTextColumn = @"text";
static NSString *const kPlayColumn = @"play";
static NSString *const kBusyIdentifier = @"retranscribing";

static NSString *singleLine(NSString *text) {
    NSArray<NSString *> *parts = [text componentsSeparatedByCharactersInSet:
        [NSCharacterSet whitespaceAndNewlineCharacterSet]];
    NSMutableArray<NSString *> *words = [NSMutableArray array];
    for (NSString *part in parts) {
        if (part.length > 0) [words addObject:part];
    }
    return [words componentsJoinedByString:@" "];
}

static NSString *formatDuration(NSInteger durationMs) {
    NSInteger seconds = (durationMs + 500) / 1000;
    return [NSString stringWithFormat:@"%ld:%02ld", (long)(seconds / 60), (long)(seconds % 60)];
}

static NSString *formatTime(NSDate *date) {
    NSCalendar *calendar = [NSCalendar currentCalendar];
    NSDateFormatter *formatter = [[NSDateFormatter alloc] init];
    if ([calendar isDateInToday:date]) {
        formatter.dateFormat = @"'сегодня' HH:mm";
    } else if ([calendar isDateInYesterday:date]) {
        formatter.dateFormat = @"'вчера' HH:mm";
    } else if ([calendar component:NSCalendarUnitYear fromDate:date] ==
               [calendar component:NSCalendarUnitYear fromDate:[NSDate date]]) {
        formatter.dateFormat = @"dd.MM HH:mm";
    } else {
        formatter.dateFormat = @"dd.MM.yy HH:mm";
    }
    return [formatter stringFromDate:date];
}

/// The beginning of `text` on one line, cut to about `limit` characters.
static NSString *snippet(NSString *text, NSUInteger limit) {
    NSString *line = singleLine(text);
    if (line.length <= limit) return line;
    NSRange safe = [line rangeOfComposedCharacterSequencesForRange:NSMakeRange(0, limit)];
    return [[line substringWithRange:safe] stringByAppendingString:@"…"];
}

/// Table that builds its context menu for the row under the pointer and hands
/// Delete and Return on the selected row to its owner.
@interface SPDictationsTableView : NSTableView
@property (nonatomic, copy) NSMenu * (^menuForRow)(NSInteger row);
@property (nonatomic, copy) void (^deletePressed)(void);
@property (nonatomic, copy) void (^returnPressed)(void);
@end

@implementation SPDictationsTableView

- (NSMenu *)menuForEvent:(NSEvent *)event {
    NSInteger row = [self rowAtPoint:[self convertPoint:event.locationInWindow fromView:nil]];
    if (row < 0 || !self.menuForRow) return nil;
    [self selectRowIndexes:[NSIndexSet indexSetWithIndex:row] byExtendingSelection:NO];
    return self.menuForRow(row);
}

- (void)keyDown:(NSEvent *)event {
    NSString *key = event.charactersIgnoringModifiers;
    NSEventModifierFlags modifiers = event.modifierFlags & NSEventModifierFlagDeviceIndependentFlagsMask &
        ~(NSEventModifierFlagFunction | NSEventModifierFlagNumericPad);
    if (key.length == 1 && modifiers == 0 && self.selectedRow >= 0) {
        unichar ch = [key characterAtIndex:0];
        if ((ch == NSDeleteCharacter || ch == NSDeleteFunctionKey) && self.deletePressed) {
            self.deletePressed();
            return;
        }
        if ((ch == NSCarriageReturnCharacter || ch == NSEnterCharacter) && self.returnPressed) {
            self.returnPressed();
            return;
        }
    }
    [super keyDown:event];
}

@end

@interface SPRecentDictationsWindowController () <NSTableViewDataSource, NSTableViewDelegate,
                                                  NSSearchFieldDelegate, NSWindowDelegate,
                                                  AVAudioPlayerDelegate>
@property (nonatomic, strong) NSSearchField *searchField;
@property (nonatomic, strong) NSTableView *tableView;
@property (nonatomic, strong) NSTextField *statusLabel;
@property (nonatomic, strong) NSTextField *countLabel;
@property (nonatomic, strong) NSTextField *retentionLabel;
@property (nonatomic, strong) NSButton *textCopyButton;
@property (nonatomic, strong) NSButton *exportButton;
@property (nonatomic, strong) NSButton *clearButton;
@property (nonatomic, strong) NSProgressIndicator *spinner;
@property (nonatomic, strong) NSWindow *textSheet;
@property (nonatomic, strong) NSTextView *sheetTextView;
@property (nonatomic, copy) NSDictionary *sheetRow;
@property (nonatomic, copy) NSArray<NSDictionary *> *allRows;
@property (nonatomic, copy) NSArray<NSDictionary *> *rows;
@property (nonatomic, strong) AVAudioPlayer *player;
@property (nonatomic, strong) NSNumber *playingId;
@property (nonatomic, assign) NSUInteger toastGeneration;
@property (nonatomic, assign) BOOL exporting;
@property (nonatomic, assign) NSInteger totalCount;
@end

@implementation SPRecentDictationsWindowController

- (instancetype)init {
    NSWindow *window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 760, 520)
                                                   styleMask:NSWindowStyleMaskTitled |
                                                             NSWindowStyleMaskClosable |
                                                             NSWindowStyleMaskMiniaturizable |
                                                             NSWindowStyleMaskResizable
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
    self = [super initWithWindow:window];
    if (self) {
        window.title = @"Последние диктовки";
        window.releasedWhenClosed = NO;
        window.minSize = NSMakeSize(600, 320);
        window.delegate = self;
        [self buildContent];
        BOOL hadSavedFrame = [[NSUserDefaults standardUserDefaults]
            objectForKey:@"NSWindow Frame RecentDictationsWindow"] != nil;
        [window setFrameAutosaveName:@"RecentDictationsWindow"];
        if (!hadSavedFrame) [window center];
        [[NSNotificationCenter defaultCenter] addObserver:self
                                                 selector:@selector(historyDidChange:)
                                                     name:SPHistoryDidChangeNotification
                                                   object:nil];
        [[NSNotificationCenter defaultCenter] addObserver:self
                                                 selector:@selector(historyDidChange:)
                                                     name:SPRetranscriptionStateDidChangeNotification
                                                   object:nil];
    }
    return self;
}

- (void)dealloc {
    [[NSNotificationCenter defaultCenter] removeObserver:self];
}

#pragma mark - Layout

- (void)buildContent {
    NSView *content = self.window.contentView;

    self.searchField = [[NSSearchField alloc] init];
    self.searchField.placeholderString = @"Поиск по всей истории";
    self.searchField.sendsSearchStringImmediately = YES;
    self.searchField.target = self;
    self.searchField.action = @selector(searchChanged:);
    self.searchField.translatesAutoresizingMaskIntoConstraints = NO;

    SPDictationsTableView *table = [[SPDictationsTableView alloc] init];
    __weak typeof(self) weakSelf = self;
    table.menuForRow = ^NSMenu *(NSInteger row) {
        return [weakSelf contextMenuForRow:[weakSelf rowAtIndex:row]];
    };
    table.deletePressed = ^{
        [weakSelf confirmDeleteRow:[weakSelf rowAtIndex:weakSelf.tableView.selectedRow]];
    };
    table.returnPressed = ^{
        [weakSelf showFullTextOfRow:[weakSelf rowAtIndex:weakSelf.tableView.selectedRow]];
    };
    self.tableView = table;
    self.tableView.dataSource = self;
    self.tableView.delegate = self;
    self.tableView.rowHeight = 28;
    self.tableView.usesAlternatingRowBackgroundColors = YES;
    self.tableView.allowsMultipleSelection = NO;
    self.tableView.allowsColumnReordering = NO;
    self.tableView.columnAutoresizingStyle = NSTableViewUniformColumnAutoresizingStyle;
    self.tableView.target = self;
    self.tableView.doubleAction = @selector(rowDoubleClicked:);

    NSArray *columns = @[
        @[ kTimeColumn, @"Время", @110 ],
        @[ kDurationColumn, @"Длит.", @52 ],
        @[ kTypeColumn, @"Тип", @74 ],
        @[ kTextColumn, @"Текст", @300 ],
        @[ kPlayColumn, @"", @36 ],
    ];
    for (NSArray *spec in columns) {
        NSTableColumn *column = [[NSTableColumn alloc] initWithIdentifier:spec[0]];
        column.title = spec[1];
        column.width = [spec[2] doubleValue];
        if ([spec[0] isEqualToString:kTextColumn]) {
            column.minWidth = 120;
            column.maxWidth = 10000;
        } else {
            column.minWidth = column.width;
            column.maxWidth = column.width;
        }
        [self.tableView addTableColumn:column];
    }

    NSScrollView *scroll = [[NSScrollView alloc] init];
    scroll.documentView = self.tableView;
    scroll.hasVerticalScroller = YES;
    scroll.borderType = NSBezelBorder;
    scroll.translatesAutoresizingMaskIntoConstraints = NO;

    self.textCopyButton = [NSButton buttonWithTitle:@"Копировать" target:self action:@selector(copy:)];
    self.textCopyButton.toolTip = @"Скопировать текст выбранной диктовки (⌘C или двойной клик)";
    self.textCopyButton.enabled = NO;
    self.textCopyButton.translatesAutoresizingMaskIntoConstraints = NO;

    self.exportButton = [NSButton buttonWithTitle:@"Выгрузить…" target:self action:@selector(exportClicked:)];
    self.exportButton.translatesAutoresizingMaskIntoConstraints = NO;

    self.clearButton = [NSButton buttonWithTitle:@"Очистить историю…" target:self
                                          action:@selector(clearHistoryClicked:)];
    self.clearButton.translatesAutoresizingMaskIntoConstraints = NO;

    self.retentionLabel = [NSTextField labelWithString:@""];
    self.retentionLabel.lineBreakMode = NSLineBreakByTruncatingTail;
    self.retentionLabel.textColor = NSColor.secondaryLabelColor;
    self.retentionLabel.font = [NSFont systemFontOfSize:NSFont.smallSystemFontSize];
    self.retentionLabel.translatesAutoresizingMaskIntoConstraints = NO;

    self.spinner = [[NSProgressIndicator alloc] init];
    self.spinner.style = NSProgressIndicatorStyleSpinning;
    self.spinner.controlSize = NSControlSizeSmall;
    self.spinner.displayedWhenStopped = NO;
    self.spinner.translatesAutoresizingMaskIntoConstraints = NO;

    self.statusLabel = [NSTextField labelWithString:@""];
    self.statusLabel.textColor = NSColor.secondaryLabelColor;
    self.statusLabel.lineBreakMode = NSLineBreakByTruncatingMiddle;
    self.statusLabel.translatesAutoresizingMaskIntoConstraints = NO;
    [self.statusLabel setContentCompressionResistancePriority:NSLayoutPriorityDefaultLow
                                                forOrientation:NSLayoutConstraintOrientationHorizontal];

    self.countLabel = [NSTextField labelWithString:@""];
    self.countLabel.textColor = NSColor.tertiaryLabelColor;
    self.countLabel.translatesAutoresizingMaskIntoConstraints = NO;

    for (NSView *view in @[ self.searchField, scroll, self.retentionLabel, self.textCopyButton,
                            self.exportButton, self.clearButton, self.spinner, self.statusLabel,
                            self.countLabel ]) {
        [content addSubview:view];
    }

    [NSLayoutConstraint activateConstraints:@[
        [self.searchField.topAnchor constraintEqualToAnchor:content.topAnchor constant:12],
        [self.searchField.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:12],
        [self.searchField.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-12],

        [scroll.topAnchor constraintEqualToAnchor:self.searchField.bottomAnchor constant:10],
        [scroll.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:12],
        [scroll.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-12],
        [scroll.bottomAnchor constraintEqualToAnchor:self.retentionLabel.topAnchor constant:-6],

        [self.retentionLabel.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:14],
        [self.retentionLabel.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-14],
        [self.retentionLabel.bottomAnchor constraintEqualToAnchor:self.exportButton.topAnchor constant:-8],

        [self.textCopyButton.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:12],
        [self.textCopyButton.bottomAnchor constraintEqualToAnchor:content.bottomAnchor constant:-12],

        [self.exportButton.leadingAnchor constraintEqualToAnchor:self.textCopyButton.trailingAnchor constant:8],
        [self.exportButton.centerYAnchor constraintEqualToAnchor:self.textCopyButton.centerYAnchor],

        [self.clearButton.leadingAnchor constraintEqualToAnchor:self.exportButton.trailingAnchor constant:8],
        [self.clearButton.centerYAnchor constraintEqualToAnchor:self.textCopyButton.centerYAnchor],

        [self.spinner.leadingAnchor constraintEqualToAnchor:self.clearButton.trailingAnchor constant:8],
        [self.spinner.centerYAnchor constraintEqualToAnchor:self.exportButton.centerYAnchor],

        [self.statusLabel.leadingAnchor constraintEqualToAnchor:self.spinner.trailingAnchor constant:8],
        [self.statusLabel.centerYAnchor constraintEqualToAnchor:self.exportButton.centerYAnchor],
        [self.statusLabel.trailingAnchor constraintLessThanOrEqualToAnchor:self.countLabel.leadingAnchor
                                                                  constant:-12],

        [self.countLabel.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-12],
        [self.countLabel.centerYAnchor constraintEqualToAnchor:self.exportButton.centerYAnchor],
    ]];
}

#pragma mark - Presenting

- (void)present {
    [self reloadRows];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    [NSApp activateIgnoringOtherApps:YES];
    [self.window makeKeyAndOrderFront:nil];
}

- (void)windowDidBecomeKey:(NSNotification *)notification {
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
}

- (void)windowWillClose:(NSNotification *)notification {
    [self stopPlayback];
    for (NSWindow *window in NSApp.windows) {
        if (window != self.window && window.visible && window.canBecomeKeyWindow) return;
    }
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
}

- (void)historyDidChange:(NSNotification *)notification {
    if (self.window.visible) [self reloadRows];
}

#pragma mark - Data

- (void)reloadRows {
    NSNumber *selectedId = nil;
    NSInteger selected = self.tableView.selectedRow;
    if (selected >= 0 && selected < (NSInteger)self.rows.count) selectedId = self.rows[selected][@"id"];

    self.allRows = [self decoratedRows:[[SPHistoryManager sharedManager] recentSessionsWithLimit:kRecentLimit]];
    self.totalCount = [[SPHistoryManager sharedManager] sessionCount];
    [self updateRetentionLabel];
    [self applyFilter];

    if (selectedId) {
        for (NSUInteger i = 0; i < self.rows.count; i++) {
            if ([self.rows[i][@"id"] isEqual:selectedId]) {
                [self.tableView selectRowIndexes:[NSIndexSet indexSetWithIndex:i] byExtendingSelection:NO];
                break;
            }
        }
    }
}

/// Session rows with `has_audio` and `retranscribing` added.
- (NSArray<NSDictionary *> *)decoratedRows:(NSArray<NSDictionary *> *)sessions {
    NSFileManager *fm = [NSFileManager defaultManager];
    NSMutableArray<NSDictionary *> *rows = [NSMutableArray arrayWithCapacity:sessions.count];
    for (NSDictionary *session in sessions) {
        NSMutableDictionary *row = [session mutableCopy];
        NSString *path = session[@"audio_path"];
        row[@"has_audio"] = @(path.length > 0 && [fm fileExistsAtPath:path]);
        row[@"retranscribing"] = @([[SPRetranscriber sharedRetranscriber]
            isRunningForSessionId:[session[@"id"] longLongValue]]);
        [rows addObject:row];
    }
    return rows;
}

- (void)updateRetentionLabel {
    self.retentionLabel.stringValue = [SPRecordingManager savingEnabled]
        ? [NSString stringWithFormat:@"Аудиозаписи хранятся %@, тексты — пока вы их не удалите.",
                                     [SPRecentDictationsWindowController daysPhrase:[SPRecordingManager retentionDays]]]
        : @"Аудиозаписи не сохраняются. Тексты хранятся, пока вы их не удалите.";
}

/// «1 день», «3 дня», «30 дней».
+ (NSString *)daysPhrase:(NSInteger)days {
    NSInteger tens = days % 100;
    NSInteger ones = days % 10;
    NSString *word = (tens >= 11 && tens <= 14) ? @"дней"
        : ones == 1 ? @"день"
        : (ones >= 2 && ones <= 4) ? @"дня" : @"дней";
    return [NSString stringWithFormat:@"%ld %@", (long)days, word];
}

- (void)applyFilter {
    NSString *query = [self.searchField.stringValue
        stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
    if (query.length == 0) {
        self.rows = self.allRows;
        self.countLabel.stringValue = self.totalCount > (NSInteger)self.allRows.count
            ? [NSString stringWithFormat:@"%lu из %ld", (unsigned long)self.allRows.count,
                                         (long)self.totalCount]
            : [NSString stringWithFormat:@"%lu", (unsigned long)self.allRows.count];
    } else {
        self.rows = [self decoratedRows:[[SPHistoryManager sharedManager] sessionsMatchingQuery:query
                                                                                         limit:kSearchLimit]];
        self.countLabel.stringValue = self.rows.count >= kSearchLimit
            ? [NSString stringWithFormat:@"найдено больше %lu", (unsigned long)kSearchLimit]
            : [NSString stringWithFormat:@"найдено %lu", (unsigned long)self.rows.count];
    }
    [self.tableView reloadData];
    [self updateButtons];
}

- (void)updateButtons {
    self.textCopyButton.enabled = [[self rowAtIndex:self.tableView.selectedRow][@"text"] length] > 0;
    self.clearButton.enabled = self.totalCount > 0;
}

- (void)tableViewSelectionDidChange:(NSNotification *)notification {
    [self updateButtons];
}

- (void)searchChanged:(id)sender {
    [self applyFilter];
}

- (NSDictionary *)rowAtIndex:(NSInteger)index {
    return index >= 0 && index < (NSInteger)self.rows.count ? self.rows[index] : nil;
}

#pragma mark - Table

- (NSInteger)numberOfRowsInTableView:(NSTableView *)tableView {
    return (NSInteger)self.rows.count;
}

- (NSTableCellView *)labelCellWithIdentifier:(NSString *)identifier monospaced:(BOOL)monospaced {
    NSTableCellView *cell = [self.tableView makeViewWithIdentifier:identifier owner:self];
    if (cell) return cell;
    cell = [[NSTableCellView alloc] init];
    cell.identifier = identifier;
    NSTextField *field = [NSTextField labelWithString:@""];
    field.translatesAutoresizingMaskIntoConstraints = NO;
    field.lineBreakMode = NSLineBreakByTruncatingTail;
    field.usesSingleLineMode = YES;
    field.font = monospaced
        ? [NSFont monospacedDigitSystemFontOfSize:13 weight:NSFontWeightRegular]
        : [NSFont systemFontOfSize:13];
    [cell addSubview:field];
    cell.textField = field;
    [NSLayoutConstraint activateConstraints:@[
        [field.leadingAnchor constraintEqualToAnchor:cell.leadingAnchor constant:4],
        [field.trailingAnchor constraintEqualToAnchor:cell.trailingAnchor constant:-4],
        [field.centerYAnchor constraintEqualToAnchor:cell.centerYAnchor],
    ]];
    return cell;
}

- (NSView *)tableView:(NSTableView *)tableView viewForTableColumn:(NSTableColumn *)column row:(NSInteger)rowIndex {
    NSDictionary *row = [self rowAtIndex:rowIndex];
    if (!row) return nil;
    NSString *identifier = column.identifier;

    if ([identifier isEqualToString:kTimeColumn]) {
        NSTableCellView *cell = [self labelCellWithIdentifier:identifier monospaced:YES];
        cell.textField.stringValue = formatTime(row[@"timestamp"]);
        cell.textField.textColor = NSColor.labelColor;
        return cell;
    }
    if ([identifier isEqualToString:kDurationColumn]) {
        NSTableCellView *cell = [self labelCellWithIdentifier:identifier monospaced:YES];
        cell.textField.stringValue = formatDuration([row[@"duration_ms"] integerValue]);
        cell.textField.textColor = NSColor.secondaryLabelColor;
        return cell;
    }
    if ([identifier isEqualToString:kTypeColumn]) {
        NSTableCellView *cell = [self labelCellWithIdentifier:identifier monospaced:NO];
        cell.textField.stringValue = [SPHistoryManager titleForPurpose:row[@"purpose"]];
        cell.textField.textColor = NSColor.secondaryLabelColor;
        return cell;
    }
    if ([identifier isEqualToString:kTextColumn]) {
        NSTableCellView *cell = [self labelCellWithIdentifier:identifier monospaced:NO];
        NSString *text = row[@"text"];
        NSString *status = row[@"status"];
        NSString *error = row[@"error"];
        NSString *tooltip = nil;
        if ([row[@"retranscribing"] boolValue]) {
            cell.textField.stringValue = @"распознаю заново…";
            cell.textField.textColor = NSColor.secondaryLabelColor;
        } else if (text.length > 0) {
            NSString *context = [SPRecentDictationsWindowController contextPrefixForRow:row];
            if (context.length > 0) {
                NSMutableAttributedString *value = [[NSMutableAttributedString alloc]
                    initWithString:context
                        attributes:@{NSForegroundColorAttributeName : NSColor.secondaryLabelColor}];
                [value appendAttributedString:[[NSAttributedString alloc]
                    initWithString:singleLine(text)
                        attributes:@{NSForegroundColorAttributeName : NSColor.labelColor}]];
                cell.textField.attributedStringValue = value;
                tooltip = [SPRecentDictationsWindowController sheetBodyForRow:row];
            } else {
                cell.textField.stringValue = singleLine(text);
                cell.textField.textColor = NSColor.labelColor;
            }
        } else {
            NSString *reason = [SPHistoryManager shortErrorReason:error];
            if ([status isEqualToString:@"cancelled"]) {
                cell.textField.stringValue = @"отменено";
            } else if ([status isEqualToString:@"error"]) {
                cell.textField.stringValue = reason.length > 0
                    ? [NSString stringWithFormat:@"ошибка: %@", singleLine(reason)] : @"ошибка";
            } else {
                cell.textField.stringValue = @"без текста";
            }
            cell.textField.textColor = NSColor.secondaryLabelColor;
            if (error.length > 0) tooltip = error;
        }
        cell.toolTip = tooltip;
        cell.textField.toolTip = tooltip;
        return cell;
    }

    NSTableCellView *cell = [tableView makeViewWithIdentifier:identifier owner:self];
    if (!cell) {
        cell = [[NSTableCellView alloc] init];
        cell.identifier = identifier;
        NSButton *button = [NSButton buttonWithImage:[NSImage imageWithSystemSymbolName:@"play.fill"
                                                               accessibilityDescription:@"Воспроизвести"]
                                              target:self
                                              action:@selector(playClicked:)];
        button.bordered = NO;
        button.imagePosition = NSImageOnly;
        button.translatesAutoresizingMaskIntoConstraints = NO;
        button.tag = 1;
        [cell addSubview:button];

        NSProgressIndicator *busy = [[NSProgressIndicator alloc] init];
        busy.style = NSProgressIndicatorStyleSpinning;
        busy.controlSize = NSControlSizeSmall;
        busy.displayedWhenStopped = NO;
        busy.translatesAutoresizingMaskIntoConstraints = NO;
        busy.identifier = kBusyIdentifier;
        [cell addSubview:busy];
        [NSLayoutConstraint activateConstraints:@[
            [button.centerXAnchor constraintEqualToAnchor:cell.centerXAnchor],
            [button.centerYAnchor constraintEqualToAnchor:cell.centerYAnchor],
            [busy.centerXAnchor constraintEqualToAnchor:cell.centerXAnchor],
            [busy.centerYAnchor constraintEqualToAnchor:cell.centerYAnchor],
        ]];
    }
    NSButton *button = [cell viewWithTag:1];
    NSProgressIndicator *busy = nil;
    for (NSView *subview in cell.subviews) {
        if ([subview.identifier isEqualToString:kBusyIdentifier]) busy = (NSProgressIndicator *)subview;
    }
    BOOL retranscribing = [row[@"retranscribing"] boolValue];
    if (retranscribing) {
        [busy startAnimation:nil];
    } else {
        [busy stopAnimation:nil];
    }
    BOOL hasAudio = [row[@"has_audio"] boolValue];
    BOOL playing = [self.playingId isEqual:row[@"id"]];
    button.hidden = !hasAudio || retranscribing;
    button.image = [NSImage imageWithSystemSymbolName:playing ? @"stop.fill" : @"play.fill"
                             accessibilityDescription:playing ? @"Остановить" : @"Воспроизвести"];
    button.toolTip = playing ? @"Остановить" : @"Воспроизвести";
    return cell;
}

/// What an Ask answered or an Edit changed, shown before the result:
/// «По „…“: » or «„…“ → ».
+ (NSString *)contextPrefixForRow:(NSDictionary *)row {
    NSString *original = row[@"original_text"];
    if (original.length == 0) return nil;
    NSString *purpose = row[@"purpose"];
    if ([purpose isEqualToString:SPHistoryPurposeAsk]) {
        return [NSString stringWithFormat:@"по «%@»: ", snippet(original, 40)];
    }
    if ([purpose isEqualToString:SPHistoryPurposeEdit]) {
        return [NSString stringWithFormat:@"«%@» → ", snippet(original, 40)];
    }
    return nil;
}

#pragma mark - Copy

- (void)copyRow:(NSDictionary *)row {
    NSString *text = row[@"text"];
    if (text.length == 0) return;
    NSPasteboard *pasteboard = [NSPasteboard generalPasteboard];
    [pasteboard clearContents];
    [pasteboard setString:text forType:NSPasteboardTypeString];
    [self showStatus:@"Скопировано" duration:kToastDuration];
}

- (void)showStatus:(NSString *)message duration:(NSTimeInterval)duration {
    self.statusLabel.stringValue = message;
    NSUInteger generation = ++self.toastGeneration;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(duration * NSEC_PER_SEC)),
                   dispatch_get_main_queue(), ^{
        if (generation == self.toastGeneration) self.statusLabel.stringValue = @"";
    });
}

- (void)copy:(id)sender {
    [self copyRow:[self rowAtIndex:self.tableView.selectedRow]];
}

/// Edit ▸ Delete.
- (void)delete:(id)sender {
    [self confirmDeleteRow:[self rowAtIndex:self.tableView.selectedRow]];
}

- (BOOL)validateUserInterfaceItem:(id<NSValidatedUserInterfaceItem>)item {
    if (item.action == @selector(delete:)) {
        return [self rowAtIndex:self.tableView.selectedRow] != nil;
    }
    if (item.action == @selector(copy:)) {
        return [[self rowAtIndex:self.tableView.selectedRow][@"text"] length] > 0;
    }
    return YES;
}

- (void)rowDoubleClicked:(id)sender {
    NSDictionary *row = [self rowAtIndex:self.tableView.clickedRow];
    if ([row[@"text"] length] > 0) {
        [self copyRow:row];
    } else {
        [self showFullTextOfRow:row];
    }
}

#pragma mark - Full text sheet

- (void)showFullTextOfRow:(NSDictionary *)row {
    NSString *error = row[@"error"];
    BOOL hasText = [row[@"text"] length] > 0;
    if ((!hasText && error.length == 0) || self.textSheet) return;

    NSWindow *sheet = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 560, 360)
                                                  styleMask:NSWindowStyleMaskTitled
                                                    backing:NSBackingStoreBuffered
                                                      defer:NO];
    NSView *content = sheet.contentView;

    NSTextField *header = [NSTextField labelWithString:
        [NSString stringWithFormat:@"%@  ·  %@  ·  %@", formatTime(row[@"timestamp"]),
                                   formatDuration([row[@"duration_ms"] integerValue]),
                                   [SPHistoryManager titleForPurpose:row[@"purpose"]]]];
    header.textColor = NSColor.secondaryLabelColor;
    header.translatesAutoresizingMaskIntoConstraints = NO;

    NSScrollView *scroll = [NSTextView scrollableTextView];
    scroll.translatesAutoresizingMaskIntoConstraints = NO;
    scroll.hasVerticalScroller = YES;
    scroll.borderType = NSBezelBorder;
    NSTextView *textView = scroll.documentView;
    textView.editable = NO;
    textView.selectable = YES;
    textView.font = [NSFont systemFontOfSize:14];
    textView.textContainerInset = NSMakeSize(6, 8);
    textView.string = [SPRecentDictationsWindowController sheetBodyForRow:row];

    NSButton *addWord = [NSButton buttonWithTitle:@"В словарь…" target:self action:@selector(sheetAddWord:)];
    addWord.toolTip = @"Добавить выделенное слово в словарь";
    addWord.enabled = hasText;
    addWord.translatesAutoresizingMaskIntoConstraints = NO;
    NSButton *copy = [NSButton buttonWithTitle:@"Копировать" target:self action:@selector(sheetCopy:)];
    copy.enabled = hasText;
    copy.translatesAutoresizingMaskIntoConstraints = NO;
    copy.identifier = row[@"id"] ? [row[@"id"] stringValue] : @"";
    NSButton *close = [NSButton buttonWithTitle:@"Закрыть" target:self action:@selector(sheetClose:)];
    close.keyEquivalent = @"\e";
    close.translatesAutoresizingMaskIntoConstraints = NO;

    for (NSView *view in @[ header, scroll, addWord, copy, close ]) [content addSubview:view];
    [NSLayoutConstraint activateConstraints:@[
        [header.topAnchor constraintEqualToAnchor:content.topAnchor constant:14],
        [header.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:16],
        [scroll.topAnchor constraintEqualToAnchor:header.bottomAnchor constant:8],
        [scroll.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:16],
        [scroll.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-16],
        [scroll.bottomAnchor constraintEqualToAnchor:close.topAnchor constant:-12],
        [close.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-16],
        [close.bottomAnchor constraintEqualToAnchor:content.bottomAnchor constant:-14],
        [copy.trailingAnchor constraintEqualToAnchor:close.leadingAnchor constant:-8],
        [copy.centerYAnchor constraintEqualToAnchor:close.centerYAnchor],
        [addWord.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:16],
        [addWord.centerYAnchor constraintEqualToAnchor:close.centerYAnchor],
    ]];

    self.textSheet = sheet;
    self.sheetTextView = textView;
    self.sheetRow = row;
    [self.window beginSheet:sheet completionHandler:^(NSModalResponse response) {
        self.textSheet = nil;
        self.sheetTextView = nil;
    }];
}

/// The text; for a row without one, the reason it has none. A text that was
/// replaced by a re-transcription is listed below.
+ (NSString *)sheetBodyForRow:(NSDictionary *)row {
    NSString *text = row[@"text"];
    NSString *error = row[@"error"];
    NSString *original = row[@"original_text"];
    NSMutableString *body = [NSMutableString string];
    if (text.length > 0) {
        [body appendString:text];
    } else if (error.length > 0) {
        [body appendFormat:@"%@: %@", [row[@"status"] isEqualToString:@"error"] ? @"Ошибка" : @"Без текста", error];
    }
    if (original.length > 0) {
        NSString *purpose = row[@"purpose"];
        NSString *title = [purpose isEqualToString:SPHistoryPurposeAsk]
            ? @"Вопрос задан голосом о выделенном тексте"
            : [purpose isEqualToString:SPHistoryPurposeEdit] ? @"Текст до правки"
            : @"Исходный текст (до повторного распознавания)";
        [body appendFormat:@"\n\n— %@ —\n%@", title, original];
    }
    return body;
}

- (void)sheetCopy:(NSButton *)sender {
    NSString *identifier = sender.identifier;
    for (NSDictionary *row in [self.rows arrayByAddingObjectsFromArray:self.allRows]) {
        if ([[row[@"id"] stringValue] isEqualToString:identifier]) {
            [self copyRow:row];
            break;
        }
    }
    sender.title = @"Скопировано";
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(kToastDuration * NSEC_PER_SEC)),
                   dispatch_get_main_queue(), ^{
        sender.title = @"Копировать";
    });
}

- (void)sheetAddWord:(id)sender {
    NSDictionary *row = self.sheetRow;
    NSString *word = nil;
    NSRange selection = self.sheetTextView.selectedRange;
    if (selection.length > 0 && NSMaxRange(selection) <= [row[@"text"] length]) {
        word = [[self.sheetTextView.string substringWithRange:selection]
            stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
    }
    [self sheetClose:nil];
    // The dialog is a sheet too; it waits for this one to finish closing.
    dispatch_async(dispatch_get_main_queue(), ^{
        [self addWordFromRow:row word:word.length > 0 ? word : nil];
    });
}

- (void)addWordFromRow:(NSDictionary *)row word:(NSString *)word {
    if ([row[@"text"] length] == 0) return;
    [SPAddWordDialog runWithText:row[@"text"] word:word window:self.window
                      completion:^(NSString *term, NSString *message) {
        if (message.length > 0) [self showStatus:message duration:kToastDuration * 2];
    }];
}

- (void)sheetClose:(id)sender {
    if (self.textSheet) [self.window endSheet:self.textSheet];
}

#pragma mark - Playback

- (void)playClicked:(NSButton *)sender {
    NSInteger index = [self.tableView rowForView:sender];
    NSDictionary *row = [self rowAtIndex:index];
    if (row) [self togglePlaybackForRow:row];
}

- (void)togglePlaybackForRow:(NSDictionary *)row {
    NSNumber *rowId = row[@"id"];
    BOOL wasPlayingThis = [self.playingId isEqual:rowId];
    [self stopPlayback];
    if (wasPlayingThis) return;

    NSError *error = nil;
    AVAudioPlayer *player = [[AVAudioPlayer alloc] initWithContentsOfURL:[NSURL fileURLWithPath:row[@"audio_path"]]
                                                                   error:&error];
    if (!player || ![player play]) {
        NSLog(@"[VelikiyMoguchiy] Cannot play recording %@: %@", row[@"audio_path"], error);
        NSBeep();
        [self showStatus:@"Не удалось воспроизвести запись" duration:kToastDuration];
        return;
    }
    player.delegate = self;
    self.player = player;
    self.playingId = rowId;
    [self reloadPlaybackIndicatorForId:rowId];
}

- (void)stopPlayback {
    NSNumber *previous = self.playingId;
    [self.player stop];
    self.player = nil;
    self.playingId = nil;
    if (previous) [self reloadPlaybackIndicatorForId:previous];
}

- (void)reloadPlaybackIndicatorForId:(NSNumber *)rowId {
    for (NSUInteger i = 0; i < self.rows.count; i++) {
        if ([self.rows[i][@"id"] isEqual:rowId]) {
            [self.tableView reloadDataForRowIndexes:[NSIndexSet indexSetWithIndex:i]
                                      columnIndexes:[NSIndexSet indexSetWithIndex:
                                          [self.tableView columnWithIdentifier:kPlayColumn]]];
            return;
        }
    }
}

- (void)audioPlayerDidFinishPlaying:(AVAudioPlayer *)player successfully:(BOOL)flag {
    if (player == self.player) [self stopPlayback];
}

#pragma mark - Context menu

- (NSMenu *)contextMenuForRow:(NSDictionary *)row {
    if (!row) return nil;
    NSMenu *menu = [[NSMenu alloc] init];
    menu.autoenablesItems = NO;

    BOOL hasText = [row[@"text"] length] > 0;
    BOOL hasAudio = [row[@"has_audio"] boolValue];
    BOOL playing = [self.playingId isEqual:row[@"id"]];

    NSMenuItem *copy = [menu addItemWithTitle:@"Копировать текст" action:@selector(menuCopy:) keyEquivalent:@"c"];
    copy.enabled = hasText;
    NSMenuItem *open = [menu addItemWithTitle:@"Показать полностью…" action:@selector(menuShowFull:)
                                keyEquivalent:@"\r"];
    open.keyEquivalentModifierMask = 0;
    open.enabled = hasText || [row[@"error"] length] > 0;
    NSMenuItem *addWord = [menu addItemWithTitle:@"Добавить слово в словарь…" action:@selector(menuAddWord:)
                                   keyEquivalent:@""];
    addWord.enabled = hasText;
    NSMenuItem *play = [menu addItemWithTitle:playing ? @"Остановить" : @"Воспроизвести"
                                       action:@selector(menuPlay:) keyEquivalent:@""];
    play.enabled = hasAudio;
    NSMenuItem *reveal = [menu addItemWithTitle:@"Показать в Finder" action:@selector(menuReveal:) keyEquivalent:@""];
    reveal.enabled = hasAudio;
    BOOL retranscribing = [row[@"retranscribing"] boolValue];
    NSMenuItem *retranscribe = [menu addItemWithTitle:retranscribing ? @"Распознаю заново…" : @"Распознать заново"
                                               action:@selector(menuRetranscribe:) keyEquivalent:@""];
    retranscribe.enabled = hasAudio && !retranscribing;
    [menu addItem:[NSMenuItem separatorItem]];
    NSMenuItem *delete = [menu addItemWithTitle:@"Удалить…" action:@selector(menuDelete:)
                                  keyEquivalent:[NSString stringWithFormat:@"%C", (unichar)NSBackspaceCharacter]];
    delete.keyEquivalentModifierMask = 0;
    for (NSMenuItem *item in menu.itemArray) {
        item.target = self;
        item.representedObject = row;
    }
    return menu;
}

- (void)menuCopy:(NSMenuItem *)item {
    [self copyRow:item.representedObject];
}

- (void)menuAddWord:(NSMenuItem *)item {
    [self addWordFromRow:item.representedObject word:nil];
}

- (void)menuShowFull:(NSMenuItem *)item {
    [self showFullTextOfRow:item.representedObject];
}

- (void)menuPlay:(NSMenuItem *)item {
    [self togglePlaybackForRow:item.representedObject];
}

- (void)menuReveal:(NSMenuItem *)item {
    NSString *path = item.representedObject[@"audio_path"];
    [[NSWorkspace sharedWorkspace] activateFileViewerSelectingURLs:@[ [NSURL fileURLWithPath:path] ]];
}

- (void)menuRetranscribe:(NSMenuItem *)item {
    [self retranscribeRow:item.representedObject];
}

- (void)retranscribeRow:(NSDictionary *)row {
    __weak typeof(self) weakSelf = self;
    BOOL started = [[SPRetranscriber sharedRetranscriber]
        retranscribeSession:row
                 completion:^(SPRetranscriptionResult *result) {
        [weakSelf finishRetranscribingRow:row result:result];
    }];
    if (started) [self showStatus:@"Распознаю заново…" duration:kRetranscribeToastDuration];
}

- (void)finishRetranscribingRow:(NSDictionary *)row result:(SPRetranscriptionResult *)result {
    if ([result.status isEqualToString:@"ok"]) {
        [self.clipboardManager cancelPendingRestore];
        [self.clipboardManager writeText:result.text];
        [self showStatus:@"Распознано заново — в буфере" duration:kRetranscribeToastDuration];
    } else if ([result.status isEqualToString:@"empty"]) {
        [self showStatus:@"Распознано заново: в записи не было речи" duration:kRetranscribeToastDuration];
    } else {
        [self showStatus:[NSString stringWithFormat:@"Не удалось распознать заново: %@",
                          singleLine([SPHistoryManager shortErrorReason:result.error])]
                duration:kRetranscribeToastDuration];
    }
}

- (void)menuDelete:(NSMenuItem *)item {
    [self confirmDeleteRow:item.representedObject];
}

- (void)confirmDeleteRow:(NSDictionary *)row {
    if (!row || self.window.attachedSheet) return;
    NSAlert *alert = [[NSAlert alloc] init];
    alert.alertStyle = NSAlertStyleWarning;
    alert.messageText = @"Удалить диктовку?";
    alert.informativeText = [row[@"has_audio"] boolValue]
        ? @"Текст и аудиозапись будут удалены без возможности восстановления."
        : @"Запись будет удалена из истории без возможности восстановления.";
    NSButton *delete = [alert addButtonWithTitle:@"Удалить"];
    delete.hasDestructiveAction = YES;
    [alert addButtonWithTitle:@"Отмена"];
    [alert beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse response) {
        if (response != NSAlertFirstButtonReturn) return;
        [self deleteRow:row];
    }];
}

- (void)deleteRow:(NSDictionary *)row {
    if ([self.playingId isEqual:row[@"id"]]) [self stopPlayback];

    NSString *path = row[@"audio_path"];
    NSString *root = [[SPRecordingManager recordingsDirectory] stringByAppendingString:@"/"];
    if (path.length > 0 && [path.stringByStandardizingPath hasPrefix:root]) {
        [[NSFileManager defaultManager] removeItemAtPath:path error:nil];
    }
    [[SPHistoryManager sharedManager] deleteSessionWithId:[row[@"id"] longLongValue]];
    [self reloadRows];
}

- (void)clearHistoryClicked:(id)sender {
    if (self.window.attachedSheet || self.exporting) return;
    NSAlert *alert = [[NSAlert alloc] init];
    alert.alertStyle = NSAlertStyleWarning;
    alert.messageText = @"Очистить всю историю?";
    alert.informativeText = [NSString stringWithFormat:
        @"Будут удалены все диктовки (%ld) и их аудиозаписи без возможности восстановления. "
        @"Словарь и настройки останутся.", (long)self.totalCount];
    NSButton *clear = [alert addButtonWithTitle:@"Очистить"];
    clear.hasDestructiveAction = YES;
    NSButton *cancel = [alert addButtonWithTitle:@"Отмена"];
    cancel.keyEquivalent = @"\r";
    clear.keyEquivalent = @"";
    [alert beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse response) {
        if (response != NSAlertFirstButtonReturn) return;
        [self clearHistory];
    }];
}

- (void)clearHistory {
    [self stopPlayback];
    [[SPHistoryManager sharedManager] deleteAllSessions];
    self.searchField.stringValue = @"";
    [self reloadRows];
    [[SPRecordingManager sharedManager] deleteAllRecordingsWithCompletion:^{
        [self showStatus:@"История очищена" duration:kToastDuration];
    }];
}

#pragma mark - Export

- (void)exportClicked:(id)sender {
    if (self.exporting) return;

    NSAlert *alert = [[NSAlert alloc] init];
    alert.messageText = @"Выгрузить диктовки";
    alert.informativeText = @"В архив попадут тексты (dictations.csv) и аудиозаписи за выбранный период.";
    NSPopUpButton *popup = [[NSPopUpButton alloc] initWithFrame:NSMakeRect(0, 0, 200, 26) pullsDown:NO];
    [popup addItemsWithTitles:@[ @"7 дней", @"30 дней", @"всё" ]];
    alert.accessoryView = popup;
    [alert addButtonWithTitle:@"Выгрузить…"];
    [alert addButtonWithTitle:@"Отмена"];
    [alert beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse response) {
        if (response != NSAlertFirstButtonReturn) return;
        NSInteger days = popup.indexOfSelectedItem == 0 ? 7 : popup.indexOfSelectedItem == 1 ? 30 : 0;
        [self chooseDestinationForPeriodDays:days];
    }];
}

- (void)chooseDestinationForPeriodDays:(NSInteger)days {
    NSDateFormatter *formatter = [[NSDateFormatter alloc] init];
    formatter.locale = [NSLocale localeWithLocaleIdentifier:@"en_US_POSIX"];
    formatter.dateFormat = @"yyyy-MM-dd";

    NSSavePanel *panel = [NSSavePanel savePanel];
    panel.allowedContentTypes = @[ UTTypeZIP ];
    panel.nameFieldStringValue = [NSString stringWithFormat:@"velikiy-moguchiy-dictations-%@.zip",
                                  [formatter stringFromDate:[NSDate date]]];
    panel.canCreateDirectories = YES;
    [panel beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse response) {
        if (response != NSModalResponseOK || !panel.URL) return;
        [self exportPeriodDays:days toURL:panel.URL];
    }];
}

- (void)exportPeriodDays:(NSInteger)days toURL:(NSURL *)destination {
    NSDate *since = days > 0 ? [NSDate dateWithTimeIntervalSinceNow:-days * 86400.0] : nil;
    NSMutableArray<NSDictionary *> *sessions = [NSMutableArray array];
    for (NSDictionary *session in [[SPHistoryManager sharedManager] recentSessionsWithLimit:1000000]) {
        if (!since || [(NSDate *)session[@"timestamp"] compare:since] != NSOrderedAscending) {
            [sessions addObject:session];
        }
    }

    self.exporting = YES;
    self.exportButton.enabled = NO;
    [self.spinner startAnimation:nil];
    self.statusLabel.stringValue = @"Готовлю архив…";
    self.toastGeneration++;

    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSError *error = nil;
        NSInteger count = [SPRecentDictationsWindowController buildArchiveFromSessions:sessions
                                                                                 toURL:destination
                                                                                 error:&error];
        dispatch_async(dispatch_get_main_queue(), ^{
            self.exporting = NO;
            self.exportButton.enabled = YES;
            [self.spinner stopAnimation:nil];
            self.statusLabel.stringValue = @"";
            if (count < 0) {
                NSAlert *alert = [[NSAlert alloc] init];
                alert.alertStyle = NSAlertStyleWarning;
                alert.messageText = @"Не удалось создать архив";
                alert.informativeText = error.localizedDescription ?: @"Неизвестная ошибка";
                [alert beginSheetModalForWindow:self.window completionHandler:nil];
                return;
            }
            [self showStatus:[NSString stringWithFormat:@"Выгружено: %ld записей", (long)count]
                    duration:kExportToastDuration];
            [[NSWorkspace sharedWorkspace] activateFileViewerSelectingURLs:@[ destination ]];
        });
    });
}

/// Returns the number of exported dictations, or -1 on failure.
+ (NSInteger)buildArchiveFromSessions:(NSArray<NSDictionary *> *)sessions
                                toURL:(NSURL *)destination
                                error:(NSError **)error {
    NSFileManager *fm = [NSFileManager defaultManager];
    NSString *tempRoot = [NSTemporaryDirectory()
        stringByAppendingPathComponent:[NSString stringWithFormat:@"velikiy-moguchiy-export-%@", NSUUID.UUID.UUIDString]];
    NSString *folder = [tempRoot stringByAppendingPathComponent:@"velikiy-moguchiy-dictations"];
    if (![fm createDirectoryAtPath:folder withIntermediateDirectories:YES attributes:nil error:error]) return -1;

    NSDateFormatter *dateFormatter = [[NSDateFormatter alloc] init];
    dateFormatter.locale = [NSLocale localeWithLocaleIdentifier:@"en_US_POSIX"];
    dateFormatter.dateFormat = @"yyyy-MM-dd HH:mm:ss";
    NSDateFormatter *nameFormatter = [[NSDateFormatter alloc] init];
    nameFormatter.locale = dateFormatter.locale;
    nameFormatter.dateFormat = @"yyyy-MM-dd_HH-mm-ss";

    NSMutableString *csv = [NSMutableString stringWithString:[SPDictationsCSV headerLine]];
    NSMutableSet<NSString *> *usedNames = [NSMutableSet set];
    for (NSDictionary *session in sessions) {
        NSDate *date = session[@"timestamp"];
        NSString *audioPath = session[@"audio_path"];
        NSString *fileName = @"";
        if (audioPath.length > 0 && [fm fileExistsAtPath:audioPath]) {
            NSString *base = [nameFormatter stringFromDate:date];
            NSString *candidate = [base stringByAppendingPathExtension:@"m4a"];
            for (int suffix = 2; [usedNames containsObject:candidate]; suffix++) {
                candidate = [[NSString stringWithFormat:@"%@_%d", base, suffix]
                             stringByAppendingPathExtension:@"m4a"];
            }
            if ([fm copyItemAtPath:audioPath
                            toPath:[folder stringByAppendingPathComponent:candidate]
                             error:nil]) {
                [usedNames addObject:candidate];
                fileName = candidate;
            }
        }
        [csv appendString:[SPDictationsCSV rowWithDate:[dateFormatter stringFromDate:date]
                                            durationMs:[session[@"duration_ms"] doubleValue]
                                                  type:[SPHistoryManager titleForPurpose:session[@"purpose"]]
                                                status:session[@"status"]
                                                  text:session[@"text"]
                                              original:session[@"original_text"]
                                                  file:fileName]];
    }

    NSMutableData *csvData = [NSMutableData dataWithBytes:"\xEF\xBB\xBF" length:3];
    [csvData appendData:[csv dataUsingEncoding:NSUTF8StringEncoding]];
    NSString *csvPath = [folder stringByAppendingPathComponent:@"dictations.csv"];
    if (![csvData writeToFile:csvPath options:NSDataWritingAtomic error:error]) {
        [fm removeItemAtPath:tempRoot error:nil];
        return -1;
    }

    [fm removeItemAtURL:destination error:nil];
    NSTask *task = [[NSTask alloc] init];
    task.executableURL = [NSURL fileURLWithPath:@"/usr/bin/ditto"];
    task.arguments = @[ @"-c", @"-k", @"--sequesterRsrc", @"--keepParent", folder, destination.path ];
    NSPipe *stderrPipe = [NSPipe pipe];
    task.standardError = stderrPipe;
    NSInteger result = (NSInteger)sessions.count;
    if (![task launchAndReturnError:error]) {
        result = -1;
    } else {
        NSData *errData = [stderrPipe.fileHandleForReading readDataToEndOfFile];
        [task waitUntilExit];
        if (task.terminationStatus != 0) {
            result = -1;
            if (error) {
                NSString *message = [[NSString alloc] initWithData:errData encoding:NSUTF8StringEncoding];
                *error = [NSError errorWithDomain:@"SPRecentDictations" code:task.terminationStatus
                                         userInfo:@{NSLocalizedDescriptionKey :
                                             message.length > 0 ? message : @"ditto завершился с ошибкой"}];
            }
        }
    }
    [fm removeItemAtPath:tempRoot error:nil];
    return result;
}

@end
