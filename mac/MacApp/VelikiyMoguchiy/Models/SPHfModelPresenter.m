#import "SPHfModelPresenter.h"
#import "SPDownloadMeter.h"

NSString *const SPLocalModelCheckResultPrefix = @"SPCHECK ";

static NSString *stringValue(id value) {
    return [value isKindOfClass:[NSString class]] ? value : @"";
}

static NSDictionary *dictionaryValue(id value) {
    return [value isKindOfClass:[NSDictionary class]] ? value : @{};
}

@implementation SPHfModelPresenter

+ (NSArray<NSDictionary *> *)variantsOfInspection:(NSDictionary *)inspection {
    NSArray *variants = inspection[@"variants"];
    if (![variants isKindOfClass:[NSArray class]]) return @[];
    NSMutableArray *out = [NSMutableArray array];
    for (id variant in variants) {
        if ([variant isKindOfClass:[NSDictionary class]]) [out addObject:variant];
    }
    return out;
}

+ (NSString *)menuTitleForVariant:(NSDictionary *)variant {
    NSMutableArray<NSString *> *parts = [NSMutableArray arrayWithObject:stringValue(variant[@"title"])];
    NSString *label = stringValue(variant[@"label"]);
    if (label.length > 0) [parts addObject:label];
    [parts addObject:[SPDownloadMeter stringForBytes:[variant[@"total_size"] unsignedLongLongValue]]];
    return [parts componentsJoinedByString:@" · "];
}

+ (NSArray<NSArray<NSString *> *> *)rowsForInspection:(NSDictionary *)inspection
                                              variant:(NSDictionary *)variant {
    NSMutableArray *rows = [NSMutableArray array];
    if (variant) {
        [rows addObject:@[ @"Тип", [NSString stringWithFormat:@"%@, %@", stringValue(variant[@"title"]),
                                                                 stringValue(variant[@"mode"])] ]];
        NSArray *files = [variant[@"files"] isKindOfClass:[NSArray class]] ? variant[@"files"] : @[];
        NSUInteger repoFiles = [inspection[@"repo_file_count"] unsignedIntegerValue];
        NSMutableArray<NSString *> *size = [NSMutableArray array];
        [size addObject:[SPDownloadMeter stringForBytes:[variant[@"total_size"] unsignedLongLongValue]]];
        [size addObject:repoFiles > files.count
                            ? [NSString stringWithFormat:@"файлов: %lu из %lu", (unsigned long)files.count,
                                                         (unsigned long)repoFiles]
                            : [NSString stringWithFormat:@"файлов: %lu", (unsigned long)files.count]];
        NSString *precision = stringValue(variant[@"precision"]);
        if (precision.length > 0) [size addObject:precision];
        [rows addObject:@[ @"Размер", [size componentsJoinedByString:@" · "] ]];
    }
    NSString *languages = stringValue(dictionaryValue(inspection[@"languages"])[@"display"]);
    [rows addObject:@[ @"Языки", languages.length > 0 ? languages : @"не указаны" ]];
    NSDictionary *license = dictionaryValue(inspection[@"license"]);
    NSString *licenseName = stringValue(license[@"name"]);
    [rows addObject:@[ @"Лицензия", licenseName.length > 0 ? licenseName : @"не указана" ]];
    NSString *sha = stringValue(inspection[@"sha"]);
    if (sha.length >= 12) {
        NSString *revision = stringValue(inspection[@"revision"]);
        NSString *commit = [sha substringToIndex:12];
        if (revision.length > 0 && ![sha hasPrefix:revision]) {
            commit = [NSString stringWithFormat:@"%@ (ревизия %@)", commit, revision];
        }
        [rows addObject:@[ @"Коммит", commit ]];
    }
    return rows;
}

+ (NSString *)warningsTextForInspection:(NSDictionary *)inspection {
    NSArray *warnings = [inspection[@"warnings"] isKindOfClass:[NSArray class]] ? inspection[@"warnings"] : @[];
    NSMutableArray<NSString *> *lines = [NSMutableArray array];
    for (id warning in warnings) {
        if ([warning isKindOfClass:[NSString class]] && [warning length] > 0) [lines addObject:warning];
    }
    return [lines componentsJoinedByString:@"\n"];
}

+ (NSString *)problemTextForInspection:(NSDictionary *)inspection {
    NSDictionary *problem = dictionaryValue(inspection[@"problem"]);
    NSString *message = stringValue(problem[@"message"]);
    if (message.length == 0) return nil;
    NSString *phrase = stringValue(inspection[@"search_phrase"]);
    if (phrase.length == 0 || [problem[@"code"] isEqual:@"gated"]) return message;
    return [NSString stringWithFormat:@"%@\n\nПоищите «%@» — часто кто-то уже выложил версию для sherpa-onnx.",
                                      message, phrase];
}

+ (NSString *)textForDownloadError:(NSString *)message {
    NSString *lower = message.lowercaseString;
    if ([lower containsString:@"already downloading"]) {
        return @"Эта модель уже скачивается: её загрузку запустили в настройках.";
    }
    if ([lower containsString:@"sha256 mismatch"]) {
        return @"Файл не совпал с контрольной суммой Hugging Face: он испорчен или подменён. "
               @"«Повторить» скачает его заново.";
    }
    if ([lower containsString:@"exceeds limit"] || [lower containsString:@"size mismatch"]) {
        return @"Сервер прислал не тот размер файла, скачивание остановлено. «Повторить» начнёт файл заново.";
    }
    if ([lower containsString:@"404"]) {
        return @"Файла больше нет на Hugging Face: репозиторий изменили или удалили.";
    }
    if ([lower containsString:@"os error 28"] || [lower containsString:@"no space left"]) {
        return @"На диске закончилось место. Освободите его и нажмите «Продолжить».";
    }
    if ([lower containsString:@"timed out"] || [lower containsString:@"error sending request"] ||
        [lower containsString:@"connect"] || [lower containsString:@"dns"] ||
        [lower containsString:@"decoding response body"]) {
        return @"Связь с Hugging Face прервалась. Нажмите «Продолжить»: скачанное не пропадёт.";
    }
    return [@"Не удалось скачать: " stringByAppendingString:message];
}

+ (NSDictionary *)checkResultFromOutput:(NSData *)output status:(int)status exitedNormally:(BOOL)exitedNormally {
    NSString *text = output.length > 0 ? [[NSString alloc] initWithData:output encoding:NSUTF8StringEncoding] : nil;
    for (NSString *line in [text componentsSeparatedByString:@"\n"]) {
        if (![line hasPrefix:SPLocalModelCheckResultPrefix]) continue;
        NSData *json = [[line substringFromIndex:SPLocalModelCheckResultPrefix.length]
            dataUsingEncoding:NSUTF8StringEncoding];
        id result = json ? [NSJSONSerialization JSONObjectWithData:json options:0 error:NULL] : nil;
        if ([result isKindOfClass:[NSDictionary class]]) return result;
    }
    NSString *error;
    if (!exitedNormally) {
        error = @"движок не смог загрузить модель: проверка завершилась аварийно";
    } else if (status == 0) {
        error = @"проверка ничего не ответила";
    } else {
        error = [NSString stringWithFormat:@"движок не смог загрузить модель (код %d)", status];
    }
    return @{ @"ok" : @NO, @"text" : @"", @"error" : error };
}

+ (NSString *)summaryForCheckResult:(NSDictionary *)result {
    if (![result[@"ok"] boolValue]) {
        NSString *error = stringValue(result[@"error"]);
        return [@"Не работает: " stringByAppendingString:error.length > 0 ? error : @"неизвестная ошибка"];
    }
    NSString *text = [stringValue(result[@"text"])
        stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
    if (text.length == 0) {
        return @"Модель запустилась, но ничего не распознала в тестовой записи: возможно, она не понимает русский.";
    }
    double seconds = [result[@"seconds"] doubleValue];
    NSString *time = [[NSString stringWithFormat:@"%.1f", seconds] stringByReplacingOccurrencesOfString:@"."
                                                                                            withString:@","];
    return [NSString stringWithFormat:@"Работает: «%@» (за %@ с)", text, time];
}

@end
