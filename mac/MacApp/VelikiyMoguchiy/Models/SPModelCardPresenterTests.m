#import "SPModelCardPresenter.h"
#import <XCTest/XCTest.h>

@interface SPModelCardPresenterTests : XCTestCase
@end

@implementation SPModelCardPresenterTests

- (NSDictionary *)recommended:(NSDictionary *)changes {
    NSMutableDictionary *entry = [@{
        @"id" : @"gigaam-v3-ctc-punct",
        @"name" : @"GigaAM v3 CTC с пунктуацией",
        @"engine" : @"sherpa-onnx",
        @"size" : @224896738,
        @"quality" : @"высокая",
        @"speed" : @"очень быстро",
        @"streaming" : @NO,
        @"languages" : @"русский",
        @"license" : @"MIT",
        @"engine_ready" : @YES,
        @"engine_note" : [NSNull null],
        @"model_path" : [NSNull null],
        @"status" : @0,
    } mutableCopy];
    [entry addEntriesFromDictionary:changes];
    return entry;
}

- (void)testARecommendedCardSaysWhatItIsBeforeTheDownload {
    NSDictionary *entry = [self recommended:@{}];
    XCTAssertEqualObjects([SPModelCardPresenter detailLineForRecommended:entry],
                          @"214 МБ · качество высокая · очень быстро · текст после паузы · русский · MIT");
    NSDictionary *streaming = [self recommended:@{@"streaming" : @YES}];
    XCTAssertTrue([[SPModelCardPresenter detailLineForRecommended:streaming] containsString:@"текст во время речи"]);
}

- (void)testTheRecommendedButtonFollowsTheDownload {
    NSDictionary *fresh = [self recommended:@{}];
    XCTAssertEqual([SPModelCardPresenter actionForRecommended:fresh downloading:NO primary:NO], SPModelActionDownload);
    NSDictionary *partial = [self recommended:@{@"status" : @1}];
    NSDictionary *ready = [self recommended:@{@"status" : @2}];
    XCTAssertEqual([SPModelCardPresenter actionForRecommended:partial downloading:NO primary:NO], SPModelActionResume);
    XCTAssertEqual([SPModelCardPresenter actionForRecommended:partial downloading:YES primary:NO], SPModelActionStop);
    XCTAssertEqual([SPModelCardPresenter actionForRecommended:ready downloading:NO primary:NO],
                   SPModelActionMakePrimary);
    XCTAssertEqual([SPModelCardPresenter actionForRecommended:ready downloading:NO primary:YES], SPModelActionNone);
    NSDictionary *mlx = [self recommended:@{@"engine" : @"mlx", @"engine_ready" : @NO}];
    XCTAssertEqual([SPModelCardPresenter actionForRecommended:mlx downloading:NO primary:NO], SPModelActionNone);
}

- (void)testPrimaryNeedsBothTheEngineAndThePath {
    XCTAssertTrue([SPModelCardPresenter isPrimaryModel:@"sherpa-onnx/ru-vosk-small"
                                              provider:@"sherpa-onnx"
                                        configProvider:@"sherpa-onnx"
                                           configModel:@"sherpa-onnx/ru-vosk-small/"]);
    XCTAssertFalse([SPModelCardPresenter isPrimaryModel:@"sherpa-onnx/ru-vosk-small"
                                               provider:@"sherpa-onnx"
                                         configProvider:@"qwen"
                                            configModel:@"sherpa-onnx/ru-vosk-small"]);
    XCTAssertFalse([SPModelCardPresenter isPrimaryModel:@"" provider:@"sherpa-onnx" configProvider:@"sherpa-onnx" configModel:@""]);
}

- (void)testCountsAndLanguagesReadShort {
    XCTAssertEqualObjects([SPModelCardPresenter countText:950], @"950");
    XCTAssertEqualObjects([SPModelCardPresenter countText:1250], @"1,2 тыс.");
    XCTAssertEqualObjects([SPModelCardPresenter countText:12000], @"12 тыс.");
    XCTAssertEqualObjects([SPModelCardPresenter countText:3400000], @"3,4 млн");
    NSArray *two = @[ @"ru", @"en" ];
    NSArray *five = @[ @"ru", @"en", @"de", @"fr", @"es" ];
    NSArray *four = @[ @"en", @"de", @"fr", @"es" ];
    XCTAssertEqualObjects([SPModelCardPresenter languagesTextForCodes:two], @"русский, английский");
    XCTAssertEqualObjects([SPModelCardPresenter languagesTextForCodes:five], @"5 языков, включая русский");
    XCTAssertEqualObjects([SPModelCardPresenter languagesTextForCodes:four], @"4 языка");
}

- (void)testSearchSummaryNamesWhatTheFilterHid {
    NSDictionary *item = @{
        @"repo" : @"csukuangfj/sherpa-onnx-whisper-turbo",
        @"downloads" : @12500,
        @"likes" : @7,
        @"languages" : @"99 языков",
        @"license" : @"apache-2.0",
    };
    XCTAssertEqualObjects([SPModelCardPresenter detailLineForSearchItem:item],
                          @"↓ 12 тыс. · ♥ 7 · 99 языков · apache-2.0");
    NSDictionary *some = @{@"items" : @[ item ], @"hidden" : @3};
    NSDictionary *allHidden = @{@"items" : @[], @"hidden" : @2};
    NSDictionary *none = @{@"items" : @[], @"hidden" : @0};
    XCTAssertEqualObjects([SPModelCardPresenter summaryForSearchResults:some], @"Найдено: 1, скрыто без русского: 3.");
    XCTAssertTrue([[SPModelCardPresenter summaryForSearchResults:allHidden] containsString:@"без фильтра"]);
    XCTAssertTrue([[SPModelCardPresenter summaryForSearchResults:none] hasPrefix:@"Ничего не нашлось"]);
}

- (NSDictionary *)gigaamFind:(NSDictionary *)added {
    return @{
        @"location" : @"/Users/user/.cache/huggingface/hub/models--aystream--GigaAM-v3-e2e-rnnt-mlx/snapshots/"
                      @"decfca492069ea30fb5ead79c4516d50c16d93ea",
        @"name" : @"aystream/GigaAM-v3-e2e-rnnt-mlx",
        @"source" : @"hf_cache",
        @"variants" : @[ @{
            @"id" : @"mlx",
            @"title" : @"GigaAM (MLX)",
            @"total_size" : @891000000,
            @"engine_ready" : @NO,
            @"engine_note" : @"Это MLX-модель (GigaAM).",
        } ],
        @"languages" : @{@"display" : @"русский, английский"},
        @"license" : @{@"name" : @"MIT"},
        @"added" : added,
    };
}

- (void)testAFoundModelShowsWhereItIsAndWhetherItCanRun {
    NSDictionary *find = [self gigaamFind:@{}];
    NSDictionary *variant = find[@"variants"][0];
    XCTAssertEqualObjects([SPModelCardPresenter sourceLineForFind:find home:@"/Users/user"],
                          @"Кэш Hugging Face · ~/.cache/huggingface/hub/models--aystream--GigaAM-v3-e2e-rnnt-mlx/"
                          @"snapshots/decfca49…");
    XCTAssertEqualObjects([SPModelCardPresenter detailLineForFind:find variant:variant],
                          @"GigaAM (MLX) · 850 МБ · русский, английский · MIT");
    XCTAssertEqual([SPModelCardPresenter actionForFind:find variant:variant primary:NO], SPModelActionNone);

    NSMutableDictionary *ready = [variant mutableCopy];
    ready[@"engine_ready"] = @YES;
    XCTAssertEqual([SPModelCardPresenter actionForFind:find variant:ready primary:NO], SPModelActionUseInPlace);
    NSDictionary *added = [self gigaamFind:@{@"mlx" : @"external/aystream-gigaam-1234abcd"}];
    XCTAssertEqualObjects([SPModelCardPresenter addedModelPathForFind:added variant:ready],
                          @"external/aystream-gigaam-1234abcd");
    XCTAssertEqual([SPModelCardPresenter actionForFind:added variant:ready primary:NO], SPModelActionMakePrimary);
}

- (void)testMyModelsSayHowEachOneLeaves {
    NSDictionary *external = @{
        @"title" : @"ggml-base · base",
        @"origin" : @"external",
        @"removal" : @"unlist",
        @"location" : @"/Users/user/whisper.cpp/models",
        @"missing" : @YES,
        @"status" : @0,
        @"size" : @147951465,
        @"engine_ready" : @YES,
        @"languages" : @[],
    };
    XCTAssertEqualObjects([SPModelCardPresenter originLineForModel:external home:@"/Users/user"],
                          @"Папка пропала: ~/whisper.cpp/models");
    XCTAssertEqualObjects([SPModelCardPresenter removalTitleForModel:external], @"Убрать из списка");
    XCTAssertTrue([[SPModelCardPresenter removalExplanationForModel:external primary:NO]
        containsString:@"останется на месте"]);
    XCTAssertEqual([SPModelCardPresenter actionForModel:external downloading:NO primary:NO], SPModelActionNone);

    NSDictionary *downloaded = @{
        @"title" : @"GigaAM v3",
        @"origin" : @"hugging_face",
        @"removal" : @"delete",
        @"status" : @2,
        @"size" : @224896738,
        @"engine_ready" : @YES,
        @"languages" : @[ @"ru" ],
        @"license" : @{@"name" : @"MIT"},
    };
    XCTAssertEqualObjects([SPModelCardPresenter detailLineForModel:downloaded], @"214 МБ · русский · MIT");
    XCTAssertEqualObjects([SPModelCardPresenter removalTitleForModel:downloaded], @"Удалить");
    XCTAssertTrue([[SPModelCardPresenter removalExplanationForModel:downloaded primary:YES]
        containsString:@"Сейчас диктовка идёт через эту модель"]);
    XCTAssertEqual([SPModelCardPresenter actionForModel:downloaded downloading:NO primary:NO],
                   SPModelActionMakePrimary);
    XCTAssertEqual([SPModelCardPresenter actionForModel:downloaded downloading:NO primary:YES], SPModelActionNone);
    NSMutableDictionary *partial = [downloaded mutableCopy];
    partial[@"status"] = @1;
    XCTAssertEqual([SPModelCardPresenter actionForModel:partial downloading:NO primary:NO], SPModelActionResume);
    XCTAssertEqual([SPModelCardPresenter actionForModel:partial downloading:YES primary:NO], SPModelActionStop);
}

@end
