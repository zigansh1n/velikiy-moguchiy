#import <XCTest/XCTest.h>
#import "SPCorrectionTracker.h"

@interface SPCorrectionTrackerTests : XCTestCase
@end

@implementation SPCorrectionTrackerTests

- (void)testTheFirstReadingWithThePasteIsTheBaseline {
    SPCorrectionTracker *tracker = [[SPCorrectionTracker alloc] initWithInsertedText:@"в кубер "];
    XCTAssertNil([tracker settledTextAfterReading:@"Привет"]);
    XCTAssertNil(tracker.baseline);
    XCTAssertNil([tracker settledTextAfterReading:@"Привет, в кубер"]);
    XCTAssertEqualObjects(tracker.baseline, @"Привет, в кубер");
}

- (void)testAnEditCountsOnceItHasSettled {
    SPCorrectionTracker *tracker = [[SPCorrectionTracker alloc] initWithInsertedText:@"в кубер"];
    [tracker settledTextAfterReading:@"в кубер"];
    XCTAssertNil([tracker settledTextAfterReading:@"в кубер"]);
    XCTAssertNil([tracker settledTextAfterReading:@"в K"]);
    XCTAssertNil([tracker settledTextAfterReading:@"в Kube"]);
    XCTAssertNil([tracker settledTextAfterReading:@"в Kubernetes"]);
    XCTAssertEqualObjects([tracker settledTextAfterReading:@"в Kubernetes"], @"в Kubernetes");
    XCTAssertNil([tracker settledTextAfterReading:@"в Kubernetes"]);
    XCTAssertEqualObjects(tracker.baseline, @"в кубер");
}

- (void)testTypingOnAfterThePasteMovesTheBaseline {
    SPCorrectionTracker *tracker = [[SPCorrectionTracker alloc] initWithInsertedText:@"в кубер"];
    [tracker settledTextAfterReading:@"в кубер"];
    [tracker settledTextAfterReading:@"в кубер и дальше"];
    NSString *settled = [tracker settledTextAfterReading:@"в кубер и дальше"];
    XCTAssertEqualObjects(settled, @"в кубер и дальше");
    [tracker noCorrectionIn:settled];
    XCTAssertEqualObjects(tracker.baseline, @"в кубер и дальше");
}

- (void)testAHalfDoneCorrectionKeepsTheBaseline {
    SPCorrectionTracker *tracker = [[SPCorrectionTracker alloc] initWithInsertedText:@"в кубер"];
    [tracker settledTextAfterReading:@"в кубер"];
    [tracker settledTextAfterReading:@"в "];
    NSString *settled = [tracker settledTextAfterReading:@"в "];
    XCTAssertEqualObjects(settled, @"в ");
    [tracker noCorrectionIn:settled];
    XCTAssertEqualObjects(tracker.baseline, @"в кубер");
}

- (void)testAFieldWithoutThePasteIsGivenUp {
    SPCorrectionTracker *tracker = [[SPCorrectionTracker alloc] initWithInsertedText:@"текст"];
    for (NSUInteger i = 0; i < SPCorrectionTracker.maxReadingsWithoutPaste; i++) {
        XCTAssertNil([tracker settledTextAfterReading:@"другое"]);
    }
    XCTAssertTrue(tracker.gaveUp);
    XCTAssertNil([tracker settledTextAfterReading:@"текст"]);
}

- (void)testUnreadableFieldsDoNotCount {
    SPCorrectionTracker *tracker = [[SPCorrectionTracker alloc] initWithInsertedText:@"текст"];
    for (int i = 0; i < 10; i++) XCTAssertNil([tracker settledTextAfterReading:nil]);
    XCTAssertFalse(tracker.gaveUp);
}

@end
