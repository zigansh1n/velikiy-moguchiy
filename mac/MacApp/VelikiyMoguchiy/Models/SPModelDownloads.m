#import "SPModelDownloads.h"
#import "SPDownloadMeter.h"
#import "SPHfModelPresenter.h"
#import "SPRustBridge.h"

NSNotificationName const SPModelDownloadsDidChangeNotification = @"SPModelDownloadsDidChangeNotification";

@interface SPModelDownloads ()
@property (nonatomic, strong) NSMutableDictionary<NSString *, SPDownloadMeter *> *meters;
@property (nonatomic, strong) NSMutableDictionary<NSString *, NSString *> *problems;
@property (nonatomic, strong) NSMutableSet<NSString *> *finished;
@end

@implementation SPModelDownloads

+ (instancetype)shared {
    static SPModelDownloads *shared;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        shared = [[self alloc] init];
    });
    return shared;
}

- (instancetype)init {
    self = [super init];
    if (self) {
        _meters = [NSMutableDictionary dictionary];
        _problems = [NSMutableDictionary dictionary];
        _finished = [NSMutableSet set];
    }
    return self;
}

- (void)post:(NSString *)path {
    [[NSNotificationCenter defaultCenter] postNotificationName:SPModelDownloadsDidChangeNotification
                                                        object:self
                                                      userInfo:@{ @"path" : path }];
}

- (void)startModelAtPath:(NSString *)path totalBytes:(uint64_t)totalBytes bridge:(SPRustBridge *)bridge {
    if (self.meters[path]) return;
    SPDownloadMeter *meter = [[SPDownloadMeter alloc] initWithTotalBytes:totalBytes];
    self.meters[path] = meter;
    [self.problems removeObjectForKey:path];
    [self.finished removeObject:path];
    [self post:path];
    __weak typeof(self) weakSelf = self;
    [bridge downloadModel:path
        progress:^(NSUInteger fileIndex, NSUInteger fileCount, uint64_t downloaded, uint64_t total, NSString *filename) {
            typeof(self) strongSelf = weakSelf;
            if (!strongSelf || strongSelf.meters[path] != meter) return;
            [meter file:fileIndex reachedBytes:downloaded atTime:NSProcessInfo.processInfo.systemUptime];
            [strongSelf post:path];
        }
        completion:^(BOOL success, NSString *message) {
            typeof(self) strongSelf = weakSelf;
            if (!strongSelf || strongSelf.meters[path] != meter) return;
            [strongSelf.meters removeObjectForKey:path];
            if (success) {
                [strongSelf.finished addObject:path];
            } else if ([message isEqualToString:@"cancelled"]) {
                strongSelf.problems[path] = @"Скачивание остановлено. Скачанное сохранится: «Продолжить» докачает остальное.";
            } else {
                strongSelf.problems[path] = [SPHfModelPresenter textForDownloadError:message ?: @""];
            }
            [strongSelf post:path];
        }];
}

- (void)stopModelAtPath:(NSString *)path bridge:(SPRustBridge *)bridge {
    if (self.meters[path]) [bridge cancelDownload:path];
}

- (BOOL)isDownloading:(NSString *)path {
    return path.length > 0 && self.meters[path] != nil;
}

- (SPDownloadMeter *)meterForPath:(NSString *)path {
    return path.length > 0 ? self.meters[path] : nil;
}

- (NSString *)problemForPath:(NSString *)path {
    return path.length > 0 ? self.problems[path] : nil;
}

- (BOOL)finishedForPath:(NSString *)path {
    return path.length > 0 && [self.finished containsObject:path];
}

- (void)forgetPath:(NSString *)path {
    [self.problems removeObjectForKey:path];
    [self.finished removeObject:path];
}

@end
