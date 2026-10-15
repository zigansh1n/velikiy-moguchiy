#import <XCTest/XCTest.h>
#import "SPDictationsCSV.h"

@interface SPDictationsCSVTests : XCTestCase
@end

@implementation SPDictationsCSVTests

- (void)testHeaderUsesSemicolons {
    XCTAssertEqualObjects([SPDictationsCSV headerLine],
                          @"date;duration_s;type;status;text;original_text;file\r\n");
}

- (void)testRowUsesSemicolonSeparatorAndDecimalComma {
    NSString *row = [SPDictationsCSV rowWithDate:@"2026-10-12 18:30:00"
                                      durationMs:12345
                                            type:@"диктовка"
                                          status:@"ok"
                                            text:@"Привет, мир."
                                        original:@""
                                            file:@"a.m4a"];
    XCTAssertEqualObjects(row,
        @"\"2026-10-12 18:30:00\";\"12,3\";\"диктовка\";\"ok\";\"Привет, мир.\";\"\";\"a.m4a\"\r\n");
}

- (void)testDurationHasNoDotDecimalMark {
    NSString *row = [SPDictationsCSV rowWithDate:@"d" durationMs:500 type:@"" status:@"ok" text:@""
                                        original:@"" file:@""];
    XCTAssertTrue([row containsString:@"\"0,5\""], @"%@", row);
    XCTAssertFalse([row containsString:@"0.5"], @"%@", row);
}

- (void)testQuotesAreDoubledAndSeparatorsInTextStayInsideQuotes {
    NSString *row = [SPDictationsCSV rowWithDate:@"d" durationMs:1000 type:@"" status:@"ok"
                                            text:@"он сказал \"да\"; потом ушёл"
                                        original:@"" file:@""];
    XCTAssertTrue([row containsString:@"\"он сказал \"\"да\"\"; потом ушёл\""], @"%@", row);
}

- (void)testNilFieldsBecomeEmptyQuotedFields {
    XCTAssertEqualObjects([SPDictationsCSV quotedField:nil], @"\"\"");
}

- (void)testFormulaLikeCellsAreKeptAsText {
    XCTAssertEqualObjects([SPDictationsCSV quotedField:@"+7 999 123-45-67"], @"\"'+7 999 123-45-67\"");
    XCTAssertEqualObjects([SPDictationsCSV quotedField:@"=HYPERLINK(\"x\")"], @"\"'=HYPERLINK(\"\"x\"\")\"");
    XCTAssertEqualObjects([SPDictationsCSV quotedField:@"-1"], @"\"'-1\"");
    XCTAssertEqualObjects([SPDictationsCSV quotedField:@"@user"], @"\"'@user\"");
    XCTAssertEqualObjects([SPDictationsCSV quotedField:@"\tx"], @"\"'\tx\"");
    XCTAssertEqualObjects([SPDictationsCSV quotedField:@"a-b = c"], @"\"a-b = c\"");
    NSString *row = [SPDictationsCSV rowWithDate:@"d" durationMs:0 type:@"" status:@"ok"
                                            text:@"=1+1" original:@"@x" file:@""];
    XCTAssertTrue([row containsString:@"\"'=1+1\";\"'@x\""], @"%@", row);
}

@end
