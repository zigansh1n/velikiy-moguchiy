#import <XCTest/XCTest.h>
#import "SPWorkScreen.h"

@interface SPWorkScreenTests : XCTestCase
@end

@implementation SPWorkScreenTests

// The primary display, a second one to its right and a third above the primary.
static NSArray<NSValue *> *Screens(void) {
    return @[
        [NSValue valueWithRect:NSMakeRect(0, 0, 1440, 900)],
        [NSValue valueWithRect:NSMakeRect(1440, -180, 1920, 1080)],
        [NSValue valueWithRect:NSMakeRect(0, 900, 1440, 900)],
    ];
}

- (void)testFocusedWindowDecides {
    // Accessibility y grows down from the primary display's top.
    NSRect onSecond = NSMakeRect(1600, 100, 800, 600);
    XCTAssertEqual(SPWorkScreenIndex(Screens(), 900, onSecond, NSMakePoint(10, 10)), 1u);
    NSRect onPrimary = NSMakeRect(100, 100, 800, 600);
    XCTAssertEqual(SPWorkScreenIndex(Screens(), 900, onPrimary, NSMakePoint(2000, 0)), 0u);
}

- (void)testDisplayAboveThePrimaryHasNegativeAccessibilityY {
    NSRect onTop = NSMakeRect(100, -800, 800, 600);
    XCTAssertEqual(SPWorkScreenIndex(Screens(), 900, onTop, NSMakePoint(10, 10)), 2u);
}

- (void)testWindowAcrossTwoDisplaysGoesWhereMostOfItIs {
    NSRect mostlySecond = NSMakeRect(1340, 100, 800, 600);
    XCTAssertEqual(SPWorkScreenIndex(Screens(), 900, mostlySecond, NSMakePoint(10, 10)), 1u);
}

- (void)testNoFocusedWindowFollowsThePointer {
    XCTAssertEqual(SPWorkScreenIndex(Screens(), 900, NSZeroRect, NSMakePoint(2000, 500)), 1u);
    XCTAssertEqual(SPWorkScreenIndex(Screens(), 900, NSZeroRect, NSMakePoint(700, 1200)), 2u);
}

- (void)testWindowOffEveryDisplayFollowsThePointer {
    NSRect offscreen = NSMakeRect(-5000, -5000, 100, 100);
    XCTAssertEqual(SPWorkScreenIndex(Screens(), 900, offscreen, NSMakePoint(2000, 500)), 1u);
}

- (void)testNothingKnownFallsBackToThePrimary {
    XCTAssertEqual(SPWorkScreenIndex(Screens(), 900, NSZeroRect, NSMakePoint(-9000, -9000)), 0u);
    XCTAssertEqual(SPWorkScreenIndex(@[], 900, NSZeroRect, NSZeroPoint), 0u);
}

@end
