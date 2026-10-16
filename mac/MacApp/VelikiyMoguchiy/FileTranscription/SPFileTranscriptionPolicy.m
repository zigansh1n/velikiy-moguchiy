#import "SPFileTranscriptionPolicy.h"
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>

@implementation SPFileTranscriptionOutcome
@end

@implementation SPFileTranscriptionPolicy

+ (BOOL)isOggOpusURL:(NSURL *)url {
    return [@[ @"ogg", @"oga", @"opus" ] containsObject:url.pathExtension.lowercaseString];
}

+ (BOOL)canTranscribeURL:(NSURL *)url {
    if (!url.isFileURL) return NO;
    if ([self isOggOpusURL:url]) return YES;
    UTType *type = [UTType typeWithFilenameExtension:url.pathExtension];
    return [type conformsToType:UTTypeAudio] || [type conformsToType:UTTypeMovie] ||
           [type conformsToType:UTTypeAudiovisualContent];
}

+ (NSArray<NSURL *> *)transcribableURLs:(NSArray<NSURL *> *)urls {
    NSMutableArray<NSURL *> *result = [NSMutableArray array];
    for (NSURL *url in urls) {
        if ([self canTranscribeURL:url]) [result addObject:url];
    }
    return result;
}

+ (NSString *)progressTextForDone:(NSUInteger)done total:(NSUInteger)total {
    if (total == 0) return @"Готовлю звук…";
    // After «из» the genitive: «из 1 части», «из 21 части», «из 5 частей».
    BOOL singular = total % 10 == 1 && total % 100 != 11;
    return [NSString stringWithFormat:@"Распознано %lu из %lu %@", (unsigned long)done,
                                      (unsigned long)total, singular ? @"части" : @"частей"];
}

+ (NSString *)durationText:(NSTimeInterval)seconds {
    NSInteger total = (NSInteger)llround(MAX(seconds, 0));
    NSInteger hours = total / 3600;
    NSInteger minutes = total / 60 % 60;
    NSInteger secs = total % 60;
    if (hours > 0) {
        return [NSString stringWithFormat:@"%ld:%02ld:%02ld", (long)hours, (long)minutes, (long)secs];
    }
    return [NSString stringWithFormat:@"%ld:%02ld", (long)minutes, (long)secs];
}

+ (NSString *)saveNameForSourceName:(NSString *)name extension:(NSString *)extension {
    NSString *base = name.stringByDeletingPathExtension;
    if (base.length == 0) base = @"Расшифровка";
    return [base stringByAppendingPathExtension:extension];
}

+ (NSString *)providerTitle:(NSString *)provider {
    NSDictionary<NSString *, NSString *> *titles = @{
        @"qwen" : @"Qwen",
        @"doubao" : @"Doubao",
        @"glm" : @"GLM",
        @"mimo" : @"MiMo",
        @"chatgpt" : @"ChatGPT",
        @"apple-speech" : @"Apple Speech",
        @"mlx" : @"локальная модель MLX",
        @"sherpa-onnx" : @"локальная модель",
        @"whisper-cpp" : @"локальная модель Whisper",
        @"wetype" : @"WeType",
    };
    return titles[provider] ?: provider;
}

+ (SPFileTranscriptionOutcome *)outcomeFromJSON:(NSString *)json {
    SPFileTranscriptionOutcome *outcome = [[SPFileTranscriptionOutcome alloc] init];
    outcome.text = @"";
    outcome.srt = @"";
    outcome.provider = @"";
    outcome.error = @"";
    NSData *data = [json dataUsingEncoding:NSUTF8StringEncoding];
    id parsed = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
    if (![parsed isKindOfClass:[NSDictionary class]]) {
        outcome.status = @"error";
        outcome.error = @"invalid result from the core";
        return outcome;
    }
    NSDictionary *dict = parsed;
    NSString * (^string)(NSString *) = ^NSString *(NSString *key) {
        id value = dict[key];
        return [value isKindOfClass:[NSString class]] ? value : @"";
    };
    if ([dict[@"cancelled"] isEqual:@YES]) {
        outcome.status = @"cancelled";
    } else if (string(@"error").length > 0) {
        outcome.status = @"error";
        outcome.error = string(@"error");
    } else {
        outcome.status = @"ok";
        outcome.text = string(@"text");
        outcome.srt = string(@"srt");
        outcome.provider = string(@"provider");
        id duration = dict[@"duration"];
        outcome.duration = [duration isKindOfClass:[NSNumber class]] ? [duration doubleValue] : 0;
    }
    return outcome;
}

@end
