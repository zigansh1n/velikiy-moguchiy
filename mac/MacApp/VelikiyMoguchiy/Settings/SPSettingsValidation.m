#import "SPSettingsValidation.h"
#import "SPHotkeyNames.h"
#import "SPTriggerKeyPolicy.h"

static NSString *SPTrimmed(NSString *value) {
    return [value ?: @"" stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
}

static BOOL SPIsKeycode(NSString *value) {
    return value.length > 0 &&
           [value rangeOfCharacterFromSet:NSCharacterSet.decimalDigitCharacterSet.invertedSet].location ==
               NSNotFound;
}

NSString *SPTriggerKeyProblem(NSString *triggerValue) {
    NSString *value = SPTrimmed(triggerValue);
    if (!SPIsKeycode(value)) return nil;
    return [SPRecordedTrigger triggerForKeyCode:value.integerValue modifierFlags:0].refusal;
}

/// One spelling per key: case and spaces do not matter, Escape is keycode 53.
static NSString *SPHotkeyIdentity(NSString *value) {
    NSString *key = SPTrimmed(value).lowercaseString;
    if ([key isEqualToString:@"escape"] || [key isEqualToString:@"esc"]) return @"53";
    if (SPIsKeycode(key)) return [NSString stringWithFormat:@"%ld", (long)key.integerValue];
    return key;
}

NSString *SPSharedHotkeyProblem(NSArray<NSArray<NSString *> *> *roles) {
    NSMutableDictionary<NSString *, NSString *> *firstRoleByKey = [NSMutableDictionary dictionary];
    for (NSArray<NSString *> *role in roles) {
        if (role.count < 2) continue;
        NSString *key = SPHotkeyIdentity(role[1]);
        if (key.length == 0 || [key isEqualToString:@"0"]) continue;
        NSString *other = firstRoleByKey[key];
        if (other) {
            NSString *name = SPHotkeyShortName(role[1]) ?: SPTrimmed(role[1]);
            return [NSString stringWithFormat:@"«%@» и «%@» назначены на одну клавишу (%@): вторая "
                                              @"тогда не работала бы. Выберите для одной из них другую.",
                                              other, role[0], name];
        }
        firstRoleByKey[key] = role[0];
    }
    return nil;
}

NSString *SPMissingProviderKeyProblem(NSString *provider, NSDictionary<NSString *, NSString *> *keys) {
    BOOL (^has)(NSString *) = ^BOOL(NSString *name) {
        return SPTrimmed(keys[name]).length > 0;
    };
    if ([provider isEqualToString:@"qwen"]) {
        return has(@"api_key") || has(@"api_keys") ? nil : @"Для Qwen не указан ключ DashScope.";
    }
    if ([provider isEqualToString:@"doubao"]) {
        return has(@"api_key") || (has(@"app_key") && has(@"access_key"))
                   ? nil
                   : @"Для Doubao не указан API-ключ (или App Key вместе с Access Key).";
    }
    if ([provider isEqualToString:@"glm"]) {
        return has(@"api_key") ? nil : @"Для GLM не указан API-ключ.";
    }
    if ([provider isEqualToString:@"mimo"]) {
        return has(@"api_key") ? nil : @"Для MiMo не указан API-ключ.";
    }
    return nil;
}

static NSInteger SPProblemNumber(NSDictionary *problem, NSString *key) {
    id value = problem[key];
    return [value isKindOfClass:[NSNumber class]] ? [value integerValue] : 0;
}

NSString *SPConfigProblemTitle(NSDictionary *problem) {
    NSInteger line = SPProblemNumber(problem, @"line");
    return line > 0 ? [NSString stringWithFormat:@"config.yaml не читается (строка %ld)", (long)line]
                    : @"config.yaml не читается";
}

NSString *SPConfigProblemDetail(NSDictionary *problem) {
    NSInteger line = SPProblemNumber(problem, @"line");
    NSInteger column = SPProblemNumber(problem, @"column");
    NSMutableString *detail = [NSMutableString
        stringWithString:@"Пока файл не исправлен, приложение работает с настройками по умолчанию, "
                         @"а сохранение настроек не сработает. Сам файл не изменён."];
    if (line > 0) {
        [detail appendFormat:@"\n\nПроверьте строку %ld", (long)line];
        if (column > 0) [detail appendFormat:@", знак %ld", (long)column];
        [detail appendString:@": отступы пробелами, двоеточие после имени, закрытые кавычки и скобки."];
    }
    if ([problem[@"backup"] boolValue]) {
        [detail appendString:@"\n\nРядом лежит config.yaml.bak — копия до последнего сохранения; её можно "
                             @"переименовать в config.yaml."];
    }
    NSString *message = [problem[@"message"] isKindOfClass:[NSString class]] ? problem[@"message"] : nil;
    if (message.length > 0) [detail appendFormat:@"\n\nОшибка разбора: %@", message];
    return detail;
}
