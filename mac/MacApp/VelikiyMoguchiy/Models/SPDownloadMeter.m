#import "SPDownloadMeter.h"

/// Speed is the transfer over this window: short enough to follow a change
/// of network, long enough not to jump with every chunk.
static const NSTimeInterval kSpeedWindow = 5.0;
static const NSTimeInterval kMinSpeedSpan = 1.0;

@interface SPDownloadMeter ()
@property (nonatomic, readwrite) uint64_t totalBytes;
@property (nonatomic, strong) NSMutableDictionary<NSNumber *, NSNumber *> *fileBytes;
/// (time, bytes transferred in this session), oldest first.
@property (nonatomic, strong) NSMutableArray<NSArray<NSNumber *> *> *samples;
@property (nonatomic) uint64_t transferred;
@end

@implementation SPDownloadMeter

- (instancetype)initWithTotalBytes:(uint64_t)totalBytes {
    self = [super init];
    if (self) {
        _totalBytes = totalBytes;
        _fileBytes = [NSMutableDictionary dictionary];
        _samples = [NSMutableArray array];
    }
    return self;
}

- (void)file:(NSUInteger)index reachedBytes:(uint64_t)bytes atTime:(NSTimeInterval)time {
    NSNumber *previous = self.fileBytes[@(index)];
    self.fileBytes[@(index)] = @(bytes);
    if (previous && bytes > previous.unsignedLongLongValue) {
        self.transferred += bytes - previous.unsignedLongLongValue;
    }
    [self.samples addObject:@[ @(time), @(self.transferred) ]];
    while (self.samples.count > 2 && time - self.samples[1][0].doubleValue >= kSpeedWindow) {
        [self.samples removeObjectAtIndex:0];
    }
}

- (uint64_t)downloadedBytes {
    uint64_t sum = 0;
    for (NSNumber *bytes in self.fileBytes.allValues) sum += bytes.unsignedLongLongValue;
    return self.totalBytes > 0 ? MIN(sum, self.totalBytes) : sum;
}

- (double)bytesPerSecond {
    if (self.samples.count < 2) return 0;
    NSArray<NSNumber *> *first = self.samples.firstObject;
    NSArray<NSNumber *> *last = self.samples.lastObject;
    NSTimeInterval span = last[0].doubleValue - first[0].doubleValue;
    if (span < kMinSpeedSpan) return 0;
    return (last[1].doubleValue - first[1].doubleValue) / span;
}

- (NSTimeInterval)secondsRemaining {
    double speed = self.bytesPerSecond;
    if (speed <= 0 || self.totalBytes == 0) return -1;
    uint64_t done = self.downloadedBytes;
    return done >= self.totalBytes ? 0 : (double)(self.totalBytes - done) / speed;
}

- (NSString *)progressText {
    NSMutableArray<NSString *> *parts = [NSMutableArray array];
    NSString *done = [[self class] stringForBytes:self.downloadedBytes];
    NSString *total = [[self class] stringForBytes:self.totalBytes];
    NSString *unit = [total componentsSeparatedByString:@" "].lastObject;
    if ([done hasSuffix:[@" " stringByAppendingString:unit]]) {
        done = [done substringToIndex:done.length - unit.length - 1];
    }
    [parts addObject:[NSString stringWithFormat:@"%@ из %@", done, total]];
    double speed = self.bytesPerSecond;
    if (speed > 0) {
        [parts addObject:[[self class] stringForSpeed:speed]];
        NSTimeInterval left = self.secondsRemaining;
        if (left >= 0) {
            [parts addObject:[@"осталось " stringByAppendingString:[[self class] stringForSecondsRemaining:left]]];
        }
    }
    return [parts componentsJoinedByString:@" · "];
}

+ (NSString *)decimal:(double)value digits:(int)digits {
    NSString *text = [NSString stringWithFormat:@"%.*f", digits, value];
    return [text stringByReplacingOccurrencesOfString:@"." withString:@","];
}

+ (NSString *)stringForBytes:(uint64_t)bytes {
    const double kb = 1024.0, mb = kb * 1024.0, gb = mb * 1024.0;
    if (bytes >= gb) return [NSString stringWithFormat:@"%@ ГБ", [self decimal:bytes / gb digits:2]];
    if (bytes >= 100 * mb) return [NSString stringWithFormat:@"%.0f МБ", bytes / mb];
    if (bytes >= mb) return [NSString stringWithFormat:@"%@ МБ", [self decimal:bytes / mb digits:1]];
    return [NSString stringWithFormat:@"%.0f КБ", ceil(bytes / kb)];
}

+ (NSString *)stringForSpeed:(double)bytesPerSecond {
    const double mb = 1024.0 * 1024.0;
    if (bytesPerSecond >= mb) {
        return [NSString stringWithFormat:@"%@ МБ/с", [self decimal:bytesPerSecond / mb digits:1]];
    }
    return [NSString stringWithFormat:@"%.0f КБ/с", bytesPerSecond / 1024.0];
}

+ (NSString *)stringForSecondsRemaining:(NSTimeInterval)seconds {
    long total = (long)ceil(MAX(seconds, 0));
    if (total < 60) return [NSString stringWithFormat:@"≈ %ld с", MAX(total, 1L)];
    long minutes = (total + 59) / 60;
    if (minutes < 60) return [NSString stringWithFormat:@"≈ %ld мин", minutes];
    long hours = minutes / 60;
    long rest = minutes % 60;
    return rest > 0 ? [NSString stringWithFormat:@"≈ %ld ч %ld мин", hours, rest]
                    : [NSString stringWithFormat:@"≈ %ld ч", hours];
}

@end
