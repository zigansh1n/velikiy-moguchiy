#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// Speed and time left of a model download whose files progress in parallel,
/// fed with the per-file progress the core reports.
@interface SPDownloadMeter : NSObject

- (instancetype)initWithTotalBytes:(uint64_t)totalBytes;

@property (nonatomic, readonly) uint64_t totalBytes;
@property (nonatomic, readonly) uint64_t downloadedBytes;
/// Over the last few seconds; 0 until a second of transfer has been seen.
@property (nonatomic, readonly) double bytesPerSecond;
/// At the current speed; negative while unknown.
@property (nonatomic, readonly) NSTimeInterval secondsRemaining;

/// `bytes` is how much of file `index` is on disk. The first report of a file
/// is what a previous attempt left, so it counts as downloaded but not as speed.
- (void)file:(NSUInteger)index reachedBytes:(uint64_t)bytes atTime:(NSTimeInterval)time;

/// «120 из 232 МБ · 4,5 МБ/с · осталось ≈ 25 с».
- (NSString *)progressText;

/// «870 КБ», «12,4 МБ», «232 МБ», «1,04 ГБ».
+ (NSString *)stringForBytes:(uint64_t)bytes;
+ (NSString *)stringForSpeed:(double)bytesPerSecond;
/// «≈ 40 с», «≈ 3 мин», «≈ 1 ч 5 мин».
+ (NSString *)stringForSecondsRemaining:(NSTimeInterval)seconds;

@end

NS_ASSUME_NONNULL_END
