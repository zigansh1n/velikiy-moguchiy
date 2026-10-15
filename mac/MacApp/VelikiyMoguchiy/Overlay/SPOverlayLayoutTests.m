#import <XCTest/XCTest.h>
#import "SPOverlayLayout.h"

@interface SPOverlayLayoutTests : XCTestCase
@end

@implementation SPOverlayLayoutTests

static const NSRect kVisible = {{0, 80}, {1440, 800}};

static NSRect PillAtBottomMargin(CGFloat margin) {
    return NSMakeRect(682, NSMinY(kVisible) + margin, 76, 30);
}

- (void)testCaptionGoesUnderThePillWhenThereIsRoom {
    NSRect pill = PillAtBottomMargin(40);
    XCTAssertTrue([SPOverlayLayout captionGoesBelowPill:pill visibleFrame:kVisible]);
    NSRect caption = [SPOverlayLayout captionFrameWithWidth:120 pill:pill visibleFrame:kVisible];
    XCTAssertEqual(NSMaxY(caption), NSMinY(pill) - SPOverlayLayout.gap);
    XCTAssertGreaterThanOrEqual(NSMinY(caption), NSMinY(kVisible));
    XCTAssertEqual(NSMidX(caption), NSMidX(pill));
}

- (void)testCaptionGoesOverThePillAtTheDefaultMargin {
    NSRect pill = PillAtBottomMargin(10);
    XCTAssertFalse([SPOverlayLayout captionGoesBelowPill:pill visibleFrame:kVisible]);
    NSRect caption = [SPOverlayLayout captionFrameWithWidth:120 pill:pill visibleFrame:kVisible];
    XCTAssertEqual(NSMinY(caption), NSMaxY(pill) + SPOverlayLayout.gap);
}

- (void)testCaptionWidthIsCapped {
    NSRect caption = [SPOverlayLayout captionFrameWithWidth:2000 pill:PillAtBottomMargin(10) visibleFrame:kVisible];
    XCTAssertEqual(NSWidth(caption), SPOverlayLayout.captionMaxWidth);
}

- (void)testCardSitsOverThePillAndItsCaption {
    NSRect pill = PillAtBottomMargin(10);
    NSRect bare = [SPOverlayLayout cardFrameWithContentHeight:100 pill:pill captionAbove:NO visibleFrame:kVisible];
    XCTAssertEqual(NSMinY(bare), NSMaxY(pill) + SPOverlayLayout.gap);
    XCTAssertEqual(NSHeight(bare), 100);
    XCTAssertEqual(NSMidX(bare), NSMidX(pill));

    NSRect overCaption = [SPOverlayLayout cardFrameWithContentHeight:100
                                                                pill:pill
                                                        captionAbove:YES
                                                        visibleFrame:kVisible];
    XCTAssertEqual(NSMinY(overCaption), NSMaxY(pill) + 2 * SPOverlayLayout.gap + SPOverlayLayout.captionHeight);
}

- (void)testLongTextScrollsInsteadOfGrowingTheCard {
    NSRect card = [SPOverlayLayout cardFrameWithContentHeight:5000
                                                         pill:PillAtBottomMargin(10)
                                                 captionAbove:NO
                                                 visibleFrame:kVisible];
    XCTAssertEqual(NSHeight(card), SPOverlayLayout.cardMaxHeight);
}

- (void)testCardStaysOnScreenNearAnEdgeAndOnASmallScreen {
    NSRect pill = NSMakeRect(NSMaxX(kVisible) - 80, NSMinY(kVisible) + 10, 76, 30);
    NSRect card = [SPOverlayLayout cardFrameWithContentHeight:100 pill:pill captionAbove:NO visibleFrame:kVisible];
    XCTAssertLessThanOrEqual(NSMaxX(card), NSMaxX(kVisible));

    NSRect low = NSMakeRect(0, 0, 300, 200);
    NSRect lowPill = NSMakeRect(112, 10, 76, 30);
    NSRect squeezed = [SPOverlayLayout cardFrameWithContentHeight:400 pill:lowPill captionAbove:NO visibleFrame:low];
    XCTAssertLessThanOrEqual(NSMaxY(squeezed), NSMaxY(low));
    XCTAssertGreaterThanOrEqual(NSMinX(squeezed), NSMinX(low));
    XCTAssertLessThanOrEqual(NSMaxX(squeezed), NSMaxX(low));
}

- (void)testBadgeCaptionDropsTheMark {
    XCTAssertEqualObjects([SPOverlayLayout captionForBadgeText:@"✓ Скопировано"], @"Скопировано");
    XCTAssertEqualObjects([SPOverlayLayout captionForBadgeText:@"↩︎ Отменено"], @"Отменено");
    XCTAssertEqualObjects([SPOverlayLayout captionForBadgeText:@"Не вставилось — в буфере"], @"Не вставилось — в буфере");
}

- (void)testBadgeIconTellsCopiedFromDelivered {
    XCTAssertEqual([SPOverlayLayout iconForBadgeText:@"✓ Скопировано"], SPOverlayBadgeIconClipboard);
    XCTAssertEqual([SPOverlayLayout iconForBadgeText:@"Не вставилось — в буфере"], SPOverlayBadgeIconClipboard);
    XCTAssertEqual([SPOverlayLayout iconForBadgeText:@"✓ Заменено"], SPOverlayBadgeIconSuccess);
    XCTAssertEqual([SPOverlayLayout iconForBadgeText:@"↩︎ Отменено"], SPOverlayBadgeIconSuccess);
}

- (void)testCaptionStaysLongEnoughToRead {
    XCTAssertEqual([SPOverlayLayout lingerForCaption:nil iconLinger:0.6], 0.6);
    XCTAssertGreaterThanOrEqual([SPOverlayLayout lingerForCaption:@"Скопировано" iconLinger:0.6], 1.5);
    XCTAssertEqual([SPOverlayLayout lingerForCaption:@"Скопировано" iconLinger:9.0], 9.0);
    NSString *long_ = [@"" stringByPaddingToLength:500 withString:@"а" startingAtIndex:0];
    XCTAssertLessThanOrEqual([SPOverlayLayout lingerForCaption:long_ iconLinger:0.6], 7.0);
}

- (void)testCardClosesItselfAfterTwentySeconds {
    XCTAssertEqual(SPOverlayLayout.cardAutoCloseDelay, 20.0);
}

@end
