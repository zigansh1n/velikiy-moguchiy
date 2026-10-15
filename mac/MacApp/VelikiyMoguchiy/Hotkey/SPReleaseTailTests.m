#import <XCTest/XCTest.h>
#import "SPReleaseTail.h"

@interface SPReleaseTailTests : XCTestCase
@end

@implementation SPReleaseTailTests

- (void)testTailIsShortEnoughNotToCatchTheNextPhrase {
    XCTAssertEqualWithAccuracy(SPReleaseTail.recordingTail, 0.25, 1e-9);
}

- (void)testTailCoversTheDiscardedCaptureBuffer {
    // 800 frames at 16 kHz: the buffer being filled when the queue stops.
    XCTAssertGreaterThanOrEqual(SPReleaseTail.recordingTail, 800.0 / 16000.0 + 0.12);
}

- (void)testDebounceAddsNothingAfterTheTail {
    XCTAssertLessThanOrEqual(SPReleaseTail.modifierReleaseDebounce, SPReleaseTail.recordingTail);
    NSTimeInterval stop = SPReleaseTail.modifierReleaseDebounce +
                          [SPReleaseTail remainingTailAfter:SPReleaseTail.modifierReleaseDebounce];
    XCTAssertEqualWithAccuracy(stop, SPReleaseTail.recordingTail, 1e-9);
}

- (void)testRemainingTail {
    XCTAssertEqualWithAccuracy([SPReleaseTail remainingTailAfter:0], SPReleaseTail.recordingTail, 1e-9);
    XCTAssertEqualWithAccuracy([SPReleaseTail remainingTailAfter:0.1], SPReleaseTail.recordingTail - 0.1, 1e-9);
    XCTAssertEqual([SPReleaseTail remainingTailAfter:5], 0);
    XCTAssertEqualWithAccuracy([SPReleaseTail remainingTailAfter:-1], SPReleaseTail.recordingTail, 1e-9);
}

@end
