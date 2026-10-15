#import "SPCorrectionTracker.h"

@interface SPCorrectionTracker ()
@property (nonatomic, copy, readwrite, nullable) NSString *baseline;
@property (nonatomic, assign, readwrite) BOOL gaveUp;
@property (nonatomic, copy, nullable) NSString *lastReading;
@property (nonatomic, copy, nullable) NSString *lastReported;
@property (nonatomic, assign) NSUInteger sameReadings;
@property (nonatomic, assign) NSUInteger readingsWithoutPaste;
@end

@implementation SPCorrectionTracker

+ (NSUInteger)maxReadingsWithoutPaste {
    return 4;
}

+ (NSUInteger)settledReadings {
    return 2;
}

- (instancetype)initWithInsertedText:(NSString *)text {
    self = [super init];
    if (self) {
        _insertedText = [text copy];
    }
    return self;
}

- (NSString *)settledTextAfterReading:(NSString *)value {
    if (self.gaveUp || !value) return nil;
    if (!self.baseline) {
        NSString *pasted = [self.insertedText stringByTrimmingCharactersInSet:
            [NSCharacterSet whitespaceAndNewlineCharacterSet]];
        if (pasted.length > 0 && [value containsString:pasted]) {
            self.baseline = value;
            self.lastReading = value;
        } else if (++self.readingsWithoutPaste >= SPCorrectionTracker.maxReadingsWithoutPaste) {
            self.gaveUp = YES;
        }
        return nil;
    }
    if (![value isEqualToString:self.lastReading]) {
        self.lastReading = value;
        self.sameReadings = 1;
        return nil;
    }
    self.sameReadings++;
    if (self.sameReadings < SPCorrectionTracker.settledReadings || [value isEqualToString:self.baseline] ||
        [value isEqualToString:self.lastReported]) {
        return nil;
    }
    self.lastReported = value;
    return value;
}

- (void)noCorrectionIn:(NSString *)text {
    NSString *pasted = [self.insertedText stringByTrimmingCharactersInSet:
        [NSCharacterSet whitespaceAndNewlineCharacterSet]];
    if ([text containsString:pasted]) self.baseline = text;
}

@end
