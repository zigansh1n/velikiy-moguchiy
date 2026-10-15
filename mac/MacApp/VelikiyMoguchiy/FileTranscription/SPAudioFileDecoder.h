#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// Reads the sound of an audio or video file as 16 kHz mono 16-bit PCM with
/// AVFoundation. Ogg voice messages are not readable here; the core decodes them.
@interface SPAudioFileDecoder : NSObject

/// Blocks until the whole file is read; call it off the main thread.
/// `isCancelled` is polled between buffers.
+ (nullable NSData *)pcmFromURL:(NSURL *)url
                    isCancelled:(nullable BOOL (^)(void))isCancelled
                          error:(NSError **)error;

@end

NS_ASSUME_NONNULL_END
