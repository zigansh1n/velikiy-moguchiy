#import "SPModelCardPresenter.h"
#import "SPDownloadMeter.h"

static NSString *stringValue(id value) {
    return [value isKindOfClass:[NSString class]] ? value : @"";
}

static NSDictionary *dictionaryValue(id value) {
    return [value isKindOfClass:[NSDictionary class]] ? value : @{};
}

static NSArray *arrayValue(id value) {
    return [value isKindOfClass:[NSArray class]] ? value : @[];
}

static uint64_t unsignedValue(id value) {
    return [value isKindOfClass:[NSNumber class]] ? [value unsignedLongLongValue] : 0;
}

static NSString *joined(NSArray<NSString *> *parts) {
    NSMutableArray *kept = [NSMutableArray array];
    for (NSString *part in parts) {
        if (part.length > 0) [kept addObject:part];
    }
    return [kept componentsJoinedByString:@" · "];
}

/// 1 язык, 2 языка, 5 языков.
static NSString *languagesWord(NSUInteger count) {
    NSUInteger tens = count % 100, ones = count % 10;
    if (tens >= 11 && tens <= 14) return @"языков";
    if (ones == 1) return @"язык";
    if (ones >= 2 && ones <= 4) return @"языка";
    return @"языков";
}

@implementation SPModelCardPresenter

+ (NSString *)titleForAction:(SPModelAction)action {
    switch (action) {
    case SPModelActionDownload:
        return @"Скачать";
    case SPModelActionResume:
        return @"Продолжить";
    case SPModelActionStop:
        return @"Остановить";
    case SPModelActionMakePrimary:
        return @"Сделать основной";
    case SPModelActionUseInPlace:
        return @"Использовать на месте";
    case SPModelActionNone:
        return @"";
    }
    return @"";
}

+ (NSString *)engineNameForProvider:(NSString *)provider {
    NSDictionary *names = @{
        @"sherpa-onnx" : @"sherpa-onnx",
        @"whisper-cpp" : @"whisper.cpp",
        @"mlx" : @"MLX",
        @"apple-speech" : @"Apple Speech",
        @"wetype" : @"WeType",
    };
    return names[stringValue(provider)] ?: stringValue(provider);
}

+ (BOOL)isPrimaryModel:(NSString *)modelPath
              provider:(NSString *)provider
        configProvider:(NSString *)configProvider
           configModel:(NSString *)configModel {
    NSCharacterSet *slash = [NSCharacterSet characterSetWithCharactersInString:@"/"];
    NSString *path = [stringValue(modelPath) stringByTrimmingCharactersInSet:slash];
    NSString *configured = [stringValue(configModel) stringByTrimmingCharactersInSet:slash];
    return path.length > 0 && [stringValue(provider) isEqualToString:stringValue(configProvider)] &&
           [path isEqualToString:configured];
}

+ (NSString *)languagesTextForCodes:(NSArray *)codes {
    NSDictionary *names = @{
        @"ru" : @"русский",
        @"en" : @"английский",
        @"uk" : @"украинский",
        @"be" : @"белорусский",
        @"kk" : @"казахский",
        @"de" : @"немецкий",
        @"fr" : @"французский",
        @"es" : @"испанский",
        @"zh" : @"китайский",
        @"ja" : @"японский",
    };
    NSMutableArray<NSString *> *list = [NSMutableArray array];
    for (id code in codes) {
        if ([code isKindOfClass:[NSString class]] && [code length] > 0) [list addObject:code];
    }
    if (list.count > 3) {
        NSString *count = [NSString stringWithFormat:@"%lu %@", (unsigned long)list.count, languagesWord(list.count)];
        return [list containsObject:@"ru"] ? [count stringByAppendingString:@", включая русский"] : count;
    }
    NSMutableArray<NSString *> *shown = [NSMutableArray array];
    for (NSString *code in list) [shown addObject:names[code] ?: code];
    return [shown componentsJoinedByString:@", "];
}

+ (NSString *)countText:(uint64_t)count {
    if (count < 1000) return [NSString stringWithFormat:@"%llu", count];
    double value;
    NSString *unit;
    if (count < 1000000) {
        value = count / 1000.0;
        unit = @"тыс.";
    } else {
        value = count / 1000000.0;
        unit = @"млн";
    }
    NSString *number = value < 10 ? [NSString stringWithFormat:@"%.1f", floor(value * 10) / 10]
                                  : [NSString stringWithFormat:@"%.0f", floor(value)];
    number = [number stringByReplacingOccurrencesOfString:@"." withString:@","];
    if ([number hasSuffix:@",0"]) number = [number substringToIndex:number.length - 2];
    return [NSString stringWithFormat:@"%@ %@", number, unit];
}

+ (NSString *)displayPath:(NSString *)path home:(NSString *)home {
    NSString *shown = stringValue(path);
    if (home.length > 1 && [shown hasPrefix:[home stringByAppendingString:@"/"]]) {
        shown = [@"~" stringByAppendingString:[shown substringFromIndex:home.length]];
    }
    NSRange snapshots = [shown rangeOfString:@"/snapshots/"];
    if (snapshots.location != NSNotFound) {
        NSUInteger start = NSMaxRange(snapshots);
        if (shown.length - start > 8) shown = [[shown substringToIndex:start + 8] stringByAppendingString:@"…"];
    }
    return shown;
}

#pragma mark Рекомендуемые

+ (NSString *)detailLineForRecommended:(NSDictionary *)entry {
    NSString *quality = stringValue(entry[@"quality"]);
    return joined(@[
        [SPDownloadMeter stringForBytes:unsignedValue(entry[@"size"])],
        quality.length > 0 ? [@"качество " stringByAppendingString:quality] : @"",
        stringValue(entry[@"speed"]),
        [entry[@"streaming"] boolValue] ? @"текст во время речи" : @"текст после паузы",
        stringValue(entry[@"languages"]),
        stringValue(entry[@"license"]),
    ]);
}

+ (SPModelAction)actionForRecommended:(NSDictionary *)entry downloading:(BOOL)downloading primary:(BOOL)primary {
    if (downloading) return SPModelActionStop;
    if (![entry[@"engine_ready"] boolValue]) return SPModelActionNone;
    switch ([entry[@"status"] integerValue]) {
    case 2:
        return primary ? SPModelActionNone : SPModelActionMakePrimary;
    case 1:
        return SPModelActionResume;
    default:
        return SPModelActionDownload;
    }
}

#pragma mark Поиск

+ (NSString *)detailLineForSearchItem:(NSDictionary *)item {
    return joined(@[
        [@"↓ " stringByAppendingString:[self countText:unsignedValue(item[@"downloads"])]],
        [@"♥ " stringByAppendingString:[self countText:unsignedValue(item[@"likes"])]],
        stringValue(item[@"languages"]),
        stringValue(item[@"license"]),
    ]);
}

+ (NSString *)summaryForSearchResults:(NSDictionary *)results {
    NSUInteger found = arrayValue(results[@"items"]).count;
    NSUInteger hidden = (NSUInteger)unsignedValue(results[@"hidden"]);
    if (found == 0 && hidden == 0) {
        return @"Ничего не нашлось. Попробуйте другое название или написание латиницей.";
    }
    if (found == 0) {
        return [NSString stringWithFormat:@"С русским ничего не нашлось; без фильтра — моделей: %lu.",
                                          (unsigned long)hidden];
    }
    if (hidden == 0) return [NSString stringWithFormat:@"Найдено: %lu.", (unsigned long)found];
    return [NSString stringWithFormat:@"Найдено: %lu, скрыто без русского: %lu.", (unsigned long)found,
                                      (unsigned long)hidden];
}

#pragma mark На этом Mac

+ (NSString *)sourceLineForFind:(NSDictionary *)find home:(NSString *)home {
    NSString *source = [stringValue(find[@"source"]) isEqualToString:@"hf_cache"] ? @"Кэш Hugging Face" : @"Папка";
    return joined(@[ source, [self displayPath:stringValue(find[@"location"]) home:home] ]);
}

+ (NSString *)detailLineForFind:(NSDictionary *)find variant:(NSDictionary *)variant {
    NSString *size = variant ? [SPDownloadMeter stringForBytes:unsignedValue(variant[@"total_size"])] : @"";
    return joined(@[
        stringValue(variant[@"title"]),
        size,
        stringValue(dictionaryValue(find[@"languages"])[@"display"]),
        stringValue(dictionaryValue(find[@"license"])[@"name"]),
    ]);
}

+ (NSString *)addedModelPathForFind:(NSDictionary *)find variant:(NSDictionary *)variant {
    NSString *path = stringValue(dictionaryValue(find[@"added"])[stringValue(variant[@"id"])]);
    return path.length > 0 ? path : nil;
}

+ (SPModelAction)actionForFind:(NSDictionary *)find variant:(NSDictionary *)variant primary:(BOOL)primary {
    if (!variant) return SPModelActionNone;
    if ([self addedModelPathForFind:find variant:variant]) {
        return primary ? SPModelActionNone : SPModelActionMakePrimary;
    }
    return [variant[@"engine_ready"] boolValue] ? SPModelActionUseInPlace : SPModelActionNone;
}

#pragma mark Мои модели

+ (NSString *)originLineForModel:(NSDictionary *)model home:(NSString *)home {
    NSString *origin = stringValue(model[@"origin"]);
    if ([origin isEqualToString:@"external"]) {
        NSString *where = [self displayPath:stringValue(model[@"location"]) home:home];
        return [model[@"missing"] boolValue] ? [@"Папка пропала: " stringByAppendingString:where]
                                             : [@"На этом Mac: " stringByAppendingString:where];
    }
    if ([origin isEqualToString:@"hugging_face"]) return @"Скачана с Hugging Face";
    return @"Встроенная модель приложения";
}

+ (NSString *)detailLineForModel:(NSDictionary *)model {
    uint64_t size = unsignedValue(model[@"size"]);
    return joined(@[
        size > 0 ? [SPDownloadMeter stringForBytes:size] : @"",
        [self languagesTextForCodes:arrayValue(model[@"languages"])],
        stringValue(dictionaryValue(model[@"license"])[@"name"]),
    ]);
}

+ (BOOL)isUnlisted:(NSDictionary *)model {
    return [stringValue(model[@"removal"]) isEqualToString:@"unlist"];
}

+ (NSString *)removalTitleForModel:(NSDictionary *)model {
    return [self isUnlisted:model] ? @"Убрать из списка" : @"Удалить";
}

+ (NSString *)removalQuestionForModel:(NSDictionary *)model {
    NSString *title = stringValue(model[@"title"]);
    return [self isUnlisted:model] ? [NSString stringWithFormat:@"Убрать «%@» из списка?", title]
                                   : [NSString stringWithFormat:@"Удалить модель «%@»?", title];
}

+ (NSString *)removalExplanationForModel:(NSDictionary *)model primary:(BOOL)primary {
    NSString *text;
    NSString *size = [SPDownloadMeter stringForBytes:unsignedValue(model[@"size"])];
    if ([self isUnlisted:model]) {
        text = @"Сама папка с моделью останется на месте: приложение её только читало.";
    } else if ([stringValue(model[@"origin"]) isEqualToString:@"hugging_face"]) {
        text = [NSString stringWithFormat:@"Файлы модели (%@) будут удалены с диска. Вернуть её можно, скачав снова.",
                                          size];
    } else {
        text = [NSString stringWithFormat:@"Скачанные файлы (%@) будут удалены. Модель останется среди встроенных, "
                                          @"её можно скачать снова.",
                                          size];
    }
    if (!primary) return text;
    return [text stringByAppendingString:@"\n\nСейчас диктовка идёт через эту модель. Выберите другую, иначе "
                                         @"распознавание на устройстве не запустится."];
}

+ (SPModelAction)actionForModel:(NSDictionary *)model downloading:(BOOL)downloading primary:(BOOL)primary {
    if (downloading) return SPModelActionStop;
    if ([model[@"missing"] boolValue]) return SPModelActionNone;
    NSInteger status = [model[@"status"] integerValue];
    if (status == 1) return SPModelActionResume;
    if (status == 2 && [model[@"engine_ready"] boolValue] && !primary) return SPModelActionMakePrimary;
    return SPModelActionNone;
}

@end
