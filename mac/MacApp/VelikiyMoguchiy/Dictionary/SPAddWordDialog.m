#import "SPAddWordDialog.h"
#import "velikiy_moguchiy_core.h"

NSNotificationName const SPDictionaryDidChangeNotification = @"SPDictionaryDidChangeNotification";

static const CGFloat kDialogWidth = 420.0;
static const NSUInteger kMaxSelectionChars = 40;

@interface SPAddWordDialog () <NSTextViewDelegate>
@property (nonatomic, strong) NSTextField *termField;
@property (nonatomic, strong) NSTextField *variantsField;
@end

@implementation SPAddWordDialog

+ (NSString *)guessedTermInText:(NSString *)text {
    NSCharacterSet *separators = [NSCharacterSet characterSetWithCharactersInString:@" \t\n\r,;:!?«»\"()"];
    NSCharacterSet *latinOrDigit = [NSCharacterSet characterSetWithCharactersInString:
        @"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"];
    for (NSString *part in [text ?: @"" componentsSeparatedByCharactersInSet:separators]) {
        NSString *word = [part stringByTrimmingCharactersInSet:[NSCharacterSet punctuationCharacterSet]];
        if (word.length > 1 && [word rangeOfCharacterFromSet:latinOrDigit].location != NSNotFound) return word;
    }
    return nil;
}

+ (void)runWithText:(NSString *)text
               word:(NSString *)word
             window:(NSWindow *)window
         completion:(void (^)(NSString *, NSString *))completion {
    SPAddWordDialog *dialog = [[SPAddWordDialog alloc] init];
    NSAlert *alert = [dialog alertWithText:text word:word ?: [self guessedTermInText:text]];
    void (^finish)(NSModalResponse) = ^(NSModalResponse response) {
        // `dialog` is the text view's delegate: the block keeps it alive.
        NSString *term = nil;
        NSString *message = @"";
        if (response == NSAlertFirstButtonReturn) {
            term = [dialog.termField.stringValue
                stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
            message = [dialog addTerm:term];
            if (![message hasPrefix:@"✓"]) term = nil;
        }
        if (completion) completion(term, message);
    };
    if (window) {
        [alert beginSheetModalForWindow:window completionHandler:finish];
    } else {
        [NSApp activateIgnoringOtherApps:YES];
        finish([alert runModal]);
    }
}

- (NSAlert *)alertWithText:(NSString *)text word:(NSString *)word {
    NSAlert *alert = [[NSAlert alloc] init];
    alert.messageText = @"Добавить слово в словарь";
    alert.informativeText = text.length > 0
        ? @"Выделите слово в тексте или впишите, как его писать. Распознавание будет писать его так."
        : @"Впишите, как писать слово. Распознавание будет писать его так.";

    NSStackView *stack = [[NSStackView alloc] init];
    stack.orientation = NSUserInterfaceLayoutOrientationVertical;
    stack.alignment = NSLayoutAttributeLeading;
    stack.spacing = 6.0;

    if (text.length > 0) {
        NSScrollView *scroll = [NSTextView scrollableTextView];
        scroll.borderType = NSBezelBorder;
        scroll.hasVerticalScroller = YES;
        NSTextView *textView = scroll.documentView;
        textView.editable = NO;
        textView.selectable = YES;
        textView.font = [NSFont systemFontOfSize:13];
        textView.string = text;
        textView.delegate = self;
        [scroll.widthAnchor constraintEqualToConstant:kDialogWidth].active = YES;
        [scroll.heightAnchor constraintEqualToConstant:84.0].active = YES;
        [stack addArrangedSubview:scroll];
    }

    self.termField = [NSTextField textFieldWithString:word ?: @""];
    self.termField.placeholderString = @"Kubernetes";
    self.variantsField = [NSTextField textFieldWithString:@""];
    self.variantsField.placeholderString = @"кубер, кубернетис";
    [stack addArrangedSubview:[NSTextField labelWithString:@"Правильно:"]];
    [stack addArrangedSubview:self.termField];
    [stack addArrangedSubview:[NSTextField labelWithString:@"Как произносят (через запятую, можно пусто):"]];
    [stack addArrangedSubview:self.variantsField];
    for (NSTextField *field in @[ self.termField, self.variantsField ]) {
        [field.widthAnchor constraintEqualToConstant:kDialogWidth].active = YES;
    }
    stack.frame = NSMakeRect(0, 0, kDialogWidth, stack.fittingSize.height);
    alert.accessoryView = stack;

    [alert addButtonWithTitle:@"Добавить"];
    [alert addButtonWithTitle:@"Отмена"];
    alert.window.initialFirstResponder = self.termField;
    return alert;
}

- (void)textViewDidChangeSelection:(NSNotification *)notification {
    NSTextView *textView = notification.object;
    NSRange range = textView.selectedRange;
    if (range.length == 0 || range.length > kMaxSelectionChars) return;
    NSString *selected = [[textView.string substringWithRange:range]
        stringByTrimmingCharactersInSet:[NSCharacterSet characterSetWithCharactersInString:@" \t\n\r.,;:!?«»\"()"]];
    if (selected.length > 0) self.termField.stringValue = selected;
}

/// Writes the entry; returns the message for the user («✓ …» on success).
- (NSString *)addTerm:(NSString *)term {
    if (term.length == 0) return @"Слово не указано";
    int result = sp_dictionary_add(term.UTF8String, self.variantsField.stringValue.UTF8String);
    switch (result) {
        case 1:
        case 2:
            [[NSNotificationCenter defaultCenter] postNotificationName:SPDictionaryDidChangeNotification
                                                                object:nil];
            return [NSString stringWithFormat:@"✓ «%@» в словаре", term];
        case 0:
            return [NSString stringWithFormat:@"«%@» уже в словаре", term];
        default:
            return @"Не удалось записать словарь";
    }
}

@end
