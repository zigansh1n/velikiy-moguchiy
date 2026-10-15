#import <XCTest/XCTest.h>
#import "SPVoiceKeyPolicy.h"

@interface SPVoiceKeyPolicyTests : XCTestCase
@end

@implementation SPVoiceKeyPolicyTests

- (void)testALonePressOnlyArms {
    SPDoublePressGate *gate = [[SPDoublePressGate alloc] initWithWindow:0.45];
    XCTAssertFalse([gate pressAt:10.0]);
    XCTAssertTrue([gate pressAt:10.3]);
    // The double press used up the arming: the next press starts over.
    XCTAssertFalse([gate pressAt:10.5]);
    // Too slow: the second press arms again instead of acting.
    XCTAssertFalse([gate pressAt:11.5]);
    XCTAssertTrue([gate pressAt:11.8]);
}

- (void)testAssistantMessages {
    XCTAssertEqualObjects(SPAssistantNeedsKeyMessage(SPSessionPurposeTranslate), @"Для перевода нужен ключ DashScope");
    XCTAssertNotNil(SPAssistantNeedsKeyMessage(SPSessionPurposeAsk));
    XCTAssertNotNil(SPAssistantNeedsKeyMessage(SPSessionPurposeEdit));
    XCTAssertNil(SPAssistantNeedsKeyMessage(SPSessionPurposeDictate));
}

- (void)testVoiceKeysSummary {
    XCTAssertEqualObjects(SPVoiceKeysSummary(@"right_option", @"left_control", @"английский"),
                          @"Ещё: правый ⌥ — перевод на английский; левый ⌃ — вопрос по выделенному "
                          @"тексту. Эти клавиши работают так же, как клавиша диктовки; нужен ключ DashScope.");
    XCTAssertEqualObjects(SPVoiceKeysSummary(@"", @"left_control", @""),
                          @"Ещё: левый ⌃ — вопрос по выделенному тексту. Эти клавиши работают так же, как "
                          @"клавиша диктовки; нужен ключ DashScope.");
    XCTAssertNil(SPVoiceKeysSummary(@"", @"", @"немецкий"));
}

@end
