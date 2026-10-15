#import <XCTest/XCTest.h>
#import "SPSettingsValidation.h"

@interface SPSettingsValidationTests : XCTestCase
@end

@implementation SPSettingsValidationTests

- (void)testTypingKeysCannotTrigger {
    // "a", space and return as bare keycodes.
    for (NSString *value in @[ @"0", @"49", @" 36 " ]) {
        XCTAssertNotNil(SPTriggerKeyProblem(value), @"%@", value);
    }
    for (NSString *value in @[ @"fn", @"right_command", @"96", @"command+shift+49", @"" ]) {
        XCTAssertNil(SPTriggerKeyProblem(value), @"%@", value);
    }
}

- (void)testTwoRolesOnOneKey {
    NSArray *fine = @[
        @[ @"Клавиша запуска", @"right_command" ], @[ @"Клавиша перевода", @"right_option" ],
        @[ @"Клавиша вопроса", @"left_control" ], @[ @"Клавиша правки", @"" ],
        @[ @"Отмена вставки", @"" ], @[ @"Клавиша отмены записи", @"escape" ],
    ];
    XCTAssertNil(SPSharedHotkeyProblem(fine));

    NSArray *clash = @[
        @[ @"Клавиша запуска", @"right_command" ], @[ @"Клавиша перевода", @"right_option" ],
        @[ @"Клавиша вопроса", @" Right_Option" ],
    ];
    NSString *problem = SPSharedHotkeyProblem(clash);
    XCTAssertTrue([problem containsString:@"«Клавиша перевода» и «Клавиша вопроса»"], @"%@", problem);
    XCTAssertTrue([problem containsString:@"правый ⌥"], @"%@", problem);

    XCTAssertNotNil(SPSharedHotkeyProblem(@[ @[ @"Отмена записи", @"escape" ], @[ @"Правка", @"53" ] ]));
}

- (void)testMissingKeys {
    XCTAssertNotNil(SPMissingProviderKeyProblem(@"qwen", @{ @"api_key" : @"  " }));
    XCTAssertNil(SPMissingProviderKeyProblem(@"qwen", @{ @"api_keys" : @"sk-1\nsk-2" }));
    XCTAssertNil(SPMissingProviderKeyProblem(@"doubao", @{ @"app_key" : @"a", @"access_key" : @"b" }));
    XCTAssertNotNil(SPMissingProviderKeyProblem(@"doubao", @{ @"app_key" : @"a" }));
    XCTAssertNotNil(SPMissingProviderKeyProblem(@"glm", @{}));
    XCTAssertNil(SPMissingProviderKeyProblem(@"mlx", @{}));
    XCTAssertNil(SPMissingProviderKeyProblem(@"chatgpt", @{}));
}

- (void)testConfigProblemNamesTheLine {
    NSDictionary *problem = @{ @"line" : @12, @"column" : @5, @"message" : @"did not find expected key",
                               @"backup" : @YES };
    XCTAssertEqualObjects(SPConfigProblemTitle(problem), @"config.yaml не читается (строка 12)");
    NSString *detail = SPConfigProblemDetail(problem);
    XCTAssertTrue([detail containsString:@"строку 12, знак 5"], @"%@", detail);
    XCTAssertTrue([detail containsString:@"config.yaml.bak"], @"%@", detail);
    XCTAssertTrue([detail containsString:@"did not find expected key"], @"%@", detail);

    NSDictionary *unknown = @{ @"line" : [NSNull null], @"message" : @"permission denied", @"backup" : @NO };
    XCTAssertEqualObjects(SPConfigProblemTitle(unknown), @"config.yaml не читается");
    XCTAssertFalse([SPConfigProblemDetail(unknown) containsString:@".bak"]);
}

@end
