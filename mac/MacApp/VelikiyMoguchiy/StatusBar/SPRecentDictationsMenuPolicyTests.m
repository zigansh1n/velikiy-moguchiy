#import <XCTest/XCTest.h>
#import "SPRecentDictationsMenuPolicy.h"

@interface SPRecentDictationsMenuPolicyTests : XCTestCase
@end

@implementation SPRecentDictationsMenuPolicyTests

- (NSUInteger)composedLength:(NSString *)text {
    __block NSUInteger count = 0;
    [text enumerateSubstringsInRange:NSMakeRange(0, text.length)
                             options:NSStringEnumerationByComposedCharacterSequences
                          usingBlock:^(NSString *s, NSRange r, NSRange e, BOOL *stop) { count++; }];
    return count;
}

#pragma mark Preview

- (void)testShortTextIsKeptWhole {
    XCTAssertEqualObjects([SPRecentDictationsMenuPolicy previewForText:@"Привет, мир." length:50], @"Привет, мир.");
}

- (void)testTextOfExactlyTheLengthHasNoEllipsis {
    NSString *text = [@"" stringByPaddingToLength:50 withString:@"а" startingAtIndex:0];
    XCTAssertEqualObjects([SPRecentDictationsMenuPolicy previewForText:text length:50], text);
}

- (void)testLineBreaksAndSpaceRunsBecomeOneSpace {
    XCTAssertEqualObjects([SPRecentDictationsMenuPolicy previewForText:@"  Первая строка\n\nвторая\tстрока  " length:50],
                          @"Первая строка вторая строка");
}

- (void)testLongTextIsCutAtAWordWithAnEllipsis {
    NSString *text = @"Сегодня мы обсудили план релиза, сроки тестирования и распределение задач в команде";
    NSString *preview = [SPRecentDictationsMenuPolicy previewForText:text length:50];
    XCTAssertEqualObjects(preview, @"Сегодня мы обсудили план релиза, сроки…");
    XCTAssertLessThanOrEqual([self composedLength:preview], 50u);
}

- (void)testPunctuationBeforeTheCutIsDropped {
    NSString *text = @"Раз, два, три, четыре, пять, шесть, семь, восемь, девять, десять";
    XCTAssertEqualObjects([SPRecentDictationsMenuPolicy previewForText:text length:50],
                          @"Раз, два, три, четыре, пять, шесть, семь, восемь…");
}

- (void)testOneLongWordIsCutInsideIt {
    NSString *text = [@"" stringByPaddingToLength:80 withString:@"ж" startingAtIndex:0];
    NSString *preview = [SPRecentDictationsMenuPolicy previewForText:text length:50];
    XCTAssertEqual([self composedLength:preview], 50u);
    XCTAssertTrue([preview hasSuffix:@"ж…"]);
}

- (void)testEmojiIsNeverSplit {
    NSString *text = [[@"" stringByPaddingToLength:48 withString:@"я" startingAtIndex:0]
        stringByAppendingString:@"👍🏽👍🏽👍🏽"];
    NSString *preview = [SPRecentDictationsMenuPolicy previewForText:text length:50];
    XCTAssertEqual([self composedLength:preview], 50u);
    XCTAssertTrue([preview hasSuffix:@"я👍🏽…"]);
}

#pragma mark Relative time

- (NSString *)timeAgo:(NSTimeInterval)seconds {
    NSDate *now = [NSDate dateWithTimeIntervalSince1970:1700000000];
    return [SPRecentDictationsMenuPolicy relativeTimeForDate:[now dateByAddingTimeInterval:-seconds] now:now];
}

- (void)testUnderAMinuteIsJustNow {
    XCTAssertEqualObjects([self timeAgo:0], @"только что");
    XCTAssertEqualObjects([self timeAgo:59], @"только что");
    XCTAssertEqualObjects([self timeAgo:-30], @"только что");
}

- (void)testMinutesAndHours {
    XCTAssertEqualObjects([self timeAgo:60], @"1 мин назад");
    XCTAssertEqualObjects([self timeAgo:2 * 60 + 59], @"2 мин назад");
    XCTAssertEqualObjects([self timeAgo:59 * 60], @"59 мин назад");
    XCTAssertEqualObjects([self timeAgo:60 * 60], @"1 ч назад");
    XCTAssertEqualObjects([self timeAgo:23 * 3600 + 3599], @"23 ч назад");
}

- (void)testDaysTakeTheRussianPlural {
    NSDictionary<NSNumber *, NSString *> *expected = @{
        @1 : @"1 день назад",   @2 : @"2 дня назад",    @4 : @"4 дня назад",
        @5 : @"5 дней назад",   @11 : @"11 дней назад", @12 : @"12 дней назад",
        @21 : @"21 день назад", @22 : @"22 дня назад",  @111 : @"111 дней назад",
    };
    [expected enumerateKeysAndObjectsUsingBlock:^(NSNumber *days, NSString *text, BOOL *stop) {
        XCTAssertEqualObjects([self timeAgo:days.doubleValue * 86400], text);
    }];
}

#pragma mark Rows

- (void)testOnlySuccessfulRowsWithTextUpToTheLimit {
    NSArray *history = @[
        @{ @"id" : @9, @"status" : @"ok", @"text" : @"девять" },
        @{ @"id" : @8, @"status" : @"error", @"text" : @"" },
        @{ @"id" : @7, @"status" : @"cancelled", @"text" : @"отменено" },
        @{ @"id" : @6, @"status" : @"ok", @"text" : @" \n " },
        @{ @"id" : @5, @"status" : @"ok", @"text" : @"пять" },
        @{ @"id" : @4, @"status" : @"ok", @"text" : @"четыре" },
    ];
    NSArray *rows = [SPRecentDictationsMenuPolicy menuRowsFromHistoryRows:history limit:2];
    XCTAssertEqualObjects([rows valueForKey:@"id"], (@[ @9, @5 ]));
    XCTAssertEqual([SPRecentDictationsMenuPolicy menuRowsFromHistoryRows:@[] limit:5].count, 0u);
}

@end
