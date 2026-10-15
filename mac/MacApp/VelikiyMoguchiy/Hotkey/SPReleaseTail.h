#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// How long a recording goes on after the trigger is let go. The tail is
/// counted from the physical release, so a modifier trigger's release
/// debounce runs inside it instead of before it.
@interface SPReleaseTail : NSObject

/// Audio kept after the release: the capture buffer being filled, which a stop
/// discards (800 frames at 16 kHz = 50 ms, SPAudioCaptureManager), a Bluetooth
/// headset's input latency (up to ~120 ms) and the decay of the last sound.
@property (class, nonatomic, readonly) NSTimeInterval recordingTail;
/// A modifier-only trigger's release counts once the flag stayed off this
/// long; a shorter dip is a false release. Never longer than recordingTail.
@property (class, nonatomic, readonly) NSTimeInterval modifierReleaseDebounce;

/// What is left of the tail when the release is reported `elapsed` seconds after it happened.
+ (NSTimeInterval)remainingTailAfter:(NSTimeInterval)elapsed;

@end

NS_ASSUME_NONNULL_END
