#import <XCTest/XCTest.h>
#import "SPDownloadMeter.h"

@interface SPDownloadMeterTests : XCTestCase
@end

@implementation SPDownloadMeterTests

- (void)testSpeedSumsFilesThatProgressTogether {
    SPDownloadMeter *meter = [[SPDownloadMeter alloc] initWithTotalBytes:100 * 1024 * 1024];
    [meter file:0 reachedBytes:0 atTime:10];
    [meter file:1 reachedBytes:0 atTime:10];
    [meter file:0 reachedBytes:2 * 1024 * 1024 atTime:11];
    [meter file:1 reachedBytes:2 * 1024 * 1024 atTime:12];
    XCTAssertEqual(meter.downloadedBytes, 4 * 1024 * 1024);
    XCTAssertEqualWithAccuracy(meter.bytesPerSecond, 2 * 1024 * 1024, 1);
    XCTAssertEqualWithAccuracy(meter.secondsRemaining, 48, 0.01);
}

- (void)testBytesLeftByAnEarlierAttemptAreNotSpeed {
    SPDownloadMeter *meter = [[SPDownloadMeter alloc] initWithTotalBytes:1000];
    [meter file:0 reachedBytes:600 atTime:1];
    [meter file:0 reachedBytes:700 atTime:3];
    XCTAssertEqual(meter.downloadedBytes, 700);
    XCTAssertEqualWithAccuracy(meter.bytesPerSecond, 50, 0.001);
    XCTAssertEqualWithAccuracy(meter.secondsRemaining, 6, 0.001);
}

- (void)testSpeedIsUnknownUntilASecondHasPassed {
    SPDownloadMeter *meter = [[SPDownloadMeter alloc] initWithTotalBytes:1000];
    [meter file:0 reachedBytes:0 atTime:1];
    [meter file:0 reachedBytes:500 atTime:1.5];
    XCTAssertEqual(meter.bytesPerSecond, 0);
    XCTAssertLessThan(meter.secondsRemaining, 0);
    XCTAssertEqualObjects([meter progressText], @"1 из 1 КБ");
}

- (void)testSpeedFollowsTheLastSeconds {
    SPDownloadMeter *meter = [[SPDownloadMeter alloc] initWithTotalBytes:1000000];
    [meter file:0 reachedBytes:0 atTime:0];
    for (int t = 1; t <= 10; t++) {
        [meter file:0 reachedBytes:(uint64_t)(t <= 5 ? t * 1000 : 5000 + (t - 5) * 100) atTime:t];
    }
    XCTAssertLessThan(meter.bytesPerSecond, 200);
}

- (void)testProgressText {
    SPDownloadMeter *meter = [[SPDownloadMeter alloc] initWithTotalBytes:232 * 1024 * 1024];
    [meter file:0 reachedBytes:0 atTime:0];
    [meter file:0 reachedBytes:120 * 1024 * 1024 atTime:20];
    XCTAssertEqualObjects([meter progressText], @"120 из 232 МБ · 6,0 МБ/с · осталось ≈ 19 с");
}

- (void)testFormatting {
    XCTAssertEqualObjects([SPDownloadMeter stringForBytes:870 * 1024], @"870 КБ");
    XCTAssertEqualObjects([SPDownloadMeter stringForBytes:(uint64_t)(12.4 * 1024 * 1024)], @"12,4 МБ");
    XCTAssertEqualObjects([SPDownloadMeter stringForBytes:232 * 1024 * 1024], @"232 МБ");
    XCTAssertEqualObjects([SPDownloadMeter stringForBytes:1036613791], @"989 МБ");
    XCTAssertEqualObjects([SPDownloadMeter stringForBytes:(uint64_t)(1.5 * 1024 * 1024 * 1024)], @"1,50 ГБ");
    XCTAssertEqualObjects([SPDownloadMeter stringForSpeed:512 * 1024], @"512 КБ/с");
    XCTAssertEqualObjects([SPDownloadMeter stringForSpeed:4.5 * 1024 * 1024], @"4,5 МБ/с");
    XCTAssertEqualObjects([SPDownloadMeter stringForSecondsRemaining:0.2], @"≈ 1 с");
    XCTAssertEqualObjects([SPDownloadMeter stringForSecondsRemaining:40], @"≈ 40 с");
    XCTAssertEqualObjects([SPDownloadMeter stringForSecondsRemaining:150], @"≈ 3 мин");
    XCTAssertEqualObjects([SPDownloadMeter stringForSecondsRemaining:3900], @"≈ 1 ч 5 мин");
    XCTAssertEqualObjects([SPDownloadMeter stringForSecondsRemaining:7200], @"≈ 2 ч");
}

@end
