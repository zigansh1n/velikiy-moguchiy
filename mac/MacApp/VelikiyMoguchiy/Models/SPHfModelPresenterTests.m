#import <XCTest/XCTest.h>
#import "SPHfModelPresenter.h"

@interface SPHfModelPresenterTests : XCTestCase
@end

@implementation SPHfModelPresenterTests

/// Shaped like the core's inspection of GigaAM v3 with punctuation.
- (NSDictionary *)gigaam {
    return @{
        @"repo" : @"csukuangfj/sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16",
        @"revision" : @"main",
        @"sha" : @"a6039be7cee829a9044a69ac0ebaf1c191217c97",
        @"repo_file_count" : @9,
        @"variants" : @[ @{
            @"id" : @"||",
            @"label" : @"",
            @"title" : @"GigaAM (трансдьюсер)",
            @"mode" : @"текст появляется после паузы",
            @"precision" : @"int8 и fp32",
            @"total_size" : @232245172,
            @"files" : @[ @{}, @{}, @{}, @{}, @{} ],
        } ],
        @"license" : @{ @"name" : @"MIT", @"kind" : @"permissive" },
        @"languages" : @{ @"display" : @"русский" },
        @"warnings" : @[],
        @"search_phrase" : @"nemo-transducer-punct-giga-am-v3-russian-2025-12-16 sherpa-onnx",
    };
}

- (void)testRowsDescribeTheVariant {
    NSDictionary *inspection = [self gigaam];
    NSDictionary *variant = [SPHfModelPresenter variantsOfInspection:inspection].firstObject;
    NSArray *rows = [SPHfModelPresenter rowsForInspection:inspection variant:variant];
    NSArray *expected = @[
        @[ @"Тип", @"GigaAM (трансдьюсер), текст появляется после паузы" ],
        @[ @"Размер", @"221 МБ · файлов: 5 из 9 · int8 и fp32" ],
        @[ @"Языки", @"русский" ],
        @[ @"Лицензия", @"MIT" ],
        @[ @"Коммит", @"a6039be7cee8 (ревизия main)" ],
    ];
    XCTAssertEqualObjects(rows, expected);
    XCTAssertEqualObjects([SPHfModelPresenter menuTitleForVariant:variant], @"GigaAM (трансдьюсер) · 221 МБ");
    XCTAssertNil([SPHfModelPresenter problemTextForInspection:inspection]);
    XCTAssertEqualObjects([SPHfModelPresenter warningsTextForInspection:inspection], @"");
}

- (void)testACommitRevisionIsNotRepeated {
    NSMutableDictionary *inspection = [[self gigaam] mutableCopy];
    inspection[@"revision"] = @"a6039be7";
    NSArray *rows = [SPHfModelPresenter rowsForInspection:inspection variant:nil];
    XCTAssertEqualObjects(rows.lastObject, (@[ @"Коммит", @"a6039be7cee8" ]));
    XCTAssertEqualObjects(rows.firstObject[0], @"Языки");
}

- (void)testAnUnsupportedRepositoryExplainsAndSuggestsASearch {
    NSDictionary *inspection = @{
        @"variants" : @[],
        @"problem" : @{ @"code" : @"pytorch", @"message" : @"В репозитории нет ONNX-файлов — эта модель только для PyTorch." },
        @"search_phrase" : @"parakeet-tdt-0.6b-v3 sherpa-onnx",
        @"warnings" : @[ @"a", @"", @"b" ],
    };
    XCTAssertEqual([SPHfModelPresenter variantsOfInspection:inspection].count, 0);
    XCTAssertEqualObjects([SPHfModelPresenter problemTextForInspection:inspection],
                          @"В репозитории нет ONNX-файлов — эта модель только для PyTorch.\n\n"
                          @"Поищите «parakeet-tdt-0.6b-v3 sherpa-onnx» — часто кто-то уже выложил версию для sherpa-onnx.");
    XCTAssertEqualObjects([SPHfModelPresenter warningsTextForInspection:inspection], @"a\nb");

    NSDictionary *gated = @{ @"problem" : @{ @"code" : @"gated", @"message" : @"Доступ по запросу." },
                             @"search_phrase" : @"x sherpa-onnx" };
    XCTAssertEqualObjects([SPHfModelPresenter problemTextForInspection:gated], @"Доступ по запросу.");
}

- (void)testDownloadErrorsInPlainWords {
    XCTAssertTrue([[SPHfModelPresenter textForDownloadError:@"sha256 mismatch for encoder.onnx: expected a, got b"]
        containsString:@"контрольной суммой"]);
    XCTAssertTrue([[SPHfModelPresenter textForDownloadError:@"download x: error sending request for url"]
        containsString:@"«Продолжить»"]);
    XCTAssertTrue([[SPHfModelPresenter textForDownloadError:@"download x: HTTP status client error (404 Not Found)"]
        containsString:@"больше нет"]);
    XCTAssertEqualObjects([SPHfModelPresenter textForDownloadError:@"rename x: busy"], @"Не удалось скачать: rename x: busy");
}

- (void)testCheckResultIsFoundAmongOtherOutput {
    NSData *output = [@"warning from onnxruntime\nSPCHECK {\"ok\":true,\"text\":\"и давно определил про себя\","
                      @"\"seconds\":1.8,\"error\":null}\n" dataUsingEncoding:NSUTF8StringEncoding];
    NSDictionary *result = [SPHfModelPresenter checkResultFromOutput:output status:0 exitedNormally:YES];
    XCTAssertEqualObjects(result[@"text"], @"и давно определил про себя");
    XCTAssertEqualObjects([SPHfModelPresenter summaryForCheckResult:result],
                          @"Работает: «и давно определил про себя» (за 1,8 с)");
}

- (void)testACrashedOrSilentCheckIsAFailure {
    NSDictionary *crashed = [SPHfModelPresenter checkResultFromOutput:nil status:6 exitedNormally:NO];
    XCTAssertFalse([crashed[@"ok"] boolValue]);
    XCTAssertTrue([[SPHfModelPresenter summaryForCheckResult:crashed] containsString:@"аварийно"]);
    NSDictionary *exited = [SPHfModelPresenter checkResultFromOutput:[NSData data] status:255 exitedNormally:YES];
    XCTAssertTrue([[SPHfModelPresenter summaryForCheckResult:exited] containsString:@"код 255"]);
    NSDictionary *empty = @{ @"ok" : @YES, @"text" : @"  ", @"seconds" : @2 };
    XCTAssertTrue([[SPHfModelPresenter summaryForCheckResult:empty] containsString:@"ничего не распознала"]);
    NSDictionary *failed = @{ @"ok" : @NO, @"error" : @"model not found" };
    XCTAssertEqualObjects([SPHfModelPresenter summaryForCheckResult:failed], @"Не работает: model not found");
}

@end
