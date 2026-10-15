#import "SPReleaseTail.h"

@implementation SPReleaseTail

+ (NSTimeInterval)recordingTail {
    return 0.25;
}

+ (NSTimeInterval)modifierReleaseDebounce {
    return 0.25;
}

+ (NSTimeInterval)remainingTailAfter:(NSTimeInterval)elapsed {
    return fmax(0.0, self.recordingTail - fmax(0.0, elapsed));
}

@end
