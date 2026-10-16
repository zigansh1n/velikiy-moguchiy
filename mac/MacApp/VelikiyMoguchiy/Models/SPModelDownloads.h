#import <Foundation/Foundation.h>

@class SPDownloadMeter;
@class SPRustBridge;

NS_ASSUME_NONNULL_BEGIN

/// Posted on the main queue whenever a download moves or ends; `userInfo[@"path"]` is the model path.
extern NSNotificationName const SPModelDownloadsDidChangeNotification;

/// The model downloads the model window started. They outlive the window:
/// closing it does not stop them, and reopening it shows where they are.
@interface SPModelDownloads : NSObject

+ (instancetype)shared;

- (void)startModelAtPath:(NSString *)path totalBytes:(uint64_t)totalBytes bridge:(SPRustBridge *)bridge;
- (void)stopModelAtPath:(NSString *)path bridge:(SPRustBridge *)bridge;

- (BOOL)isDownloading:(NSString *)path;
- (nullable SPDownloadMeter *)meterForPath:(NSString *)path;
/// How the last download of `path` ended, in the window's words: nil while
/// it runs or after success.
- (nullable NSString *)problemForPath:(NSString *)path;
/// The download of `path` ended with every file checked against its SHA-256.
- (BOOL)finishedForPath:(NSString *)path;
- (void)forgetPath:(NSString *)path;

@end

NS_ASSUME_NONNULL_END
