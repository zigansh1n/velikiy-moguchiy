#import "SPVoiceKeyPolicy.h"
#import "SPHotkeyNames.h"

@implementation SPDoublePressGate {
    NSTimeInterval _window;
    NSTimeInterval _armedAt;
    BOOL _armed;
}

- (instancetype)initWithWindow:(NSTimeInterval)window {
    if ((self = [super init])) _window = window;
    return self;
}

- (BOOL)pressAt:(NSTimeInterval)time {
    if (_armed && time - _armedAt <= _window && time >= _armedAt) {
        _armed = NO;
        return YES;
    }
    _armed = YES;
    _armedAt = time;
    return NO;
}

@end

NSString *SPAssistantNeedsKeyMessage(SPSessionPurpose purpose) {
    switch (purpose) {
        case SPSessionPurposeTranslate: return @"Для перевода нужен ключ DashScope";
        case SPSessionPurposeAsk: return @"Для вопроса нужен ключ DashScope";
        case SPSessionPurposeEdit: return @"Для правки нужен ключ DashScope";
        default: return nil;
    }
}

NSString *SPVoiceKeysSummary(NSString *translateKey, NSString *askKey, NSString *translateTo) {
    NSString *translateName = SPHotkeyShortName(translateKey ?: @"");
    NSString *askName = SPHotkeyShortName(askKey ?: @"");
    NSString *language = [translateTo ?: @"" stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceCharacterSet];
    if (language.length == 0) language = @"английский";
    NSMutableArray<NSString *> *parts = [NSMutableArray array];
    if (translateName)
        [parts addObject:[NSString stringWithFormat:@"%@ — перевод на %@", translateName, language]];
    if (askName)
        [parts addObject:[NSString stringWithFormat:@"%@ — вопрос по выделенному тексту", askName]];
    if (parts.count == 0) return nil;
    return [NSString stringWithFormat:@"Ещё: %@. Эти клавиши работают так же, как клавиша диктовки; нужен ключ "
                                      @"DashScope.",
                                      [parts componentsJoinedByString:@"; "]];
}
