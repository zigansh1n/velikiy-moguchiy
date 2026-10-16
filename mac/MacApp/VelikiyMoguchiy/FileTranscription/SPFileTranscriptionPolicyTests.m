#import <XCTest/XCTest.h>
#import "SPFileTranscriptionPolicy.h"

@interface SPFileTranscriptionPolicyTests : XCTestCase
@end

@implementation SPFileTranscriptionPolicyTests

- (void)testAcceptsVoiceMessagesAudioAndVideo {
    for (NSString *name in @[ @"audio_2026.ogg", @"Голос.OGA", @"note.opus", @"call.m4a", @"song.mp3",
                              @"rec.wav", @"clip.mp4", @"screen.mov" ]) {
        NSURL *url = [NSURL fileURLWithPath:[@"/tmp" stringByAppendingPathComponent:name]];
        XCTAssertTrue([SPFileTranscriptionPolicy canTranscribeURL:url], @"%@", name);
    }
    for (NSString *name in @[ @"notes.txt", @"photo.jpg", @"archive.zip", @"noext" ]) {
        NSURL *url = [NSURL fileURLWithPath:[@"/tmp" stringByAppendingPathComponent:name]];
        XCTAssertFalse([SPFileTranscriptionPolicy canTranscribeURL:url], @"%@", name);
    }
    XCTAssertFalse([SPFileTranscriptionPolicy canTranscribeURL:[NSURL URLWithString:@"https://example.com/a.ogg"]]);
}

- (void)testOggIsDecodedByTheCore {
    XCTAssertTrue([SPFileTranscriptionPolicy isOggOpusURL:[NSURL fileURLWithPath:@"/tmp/a.OGG"]]);
    XCTAssertFalse([SPFileTranscriptionPolicy isOggOpusURL:[NSURL fileURLWithPath:@"/tmp/a.m4a"]]);
}

- (void)testProgressUsesTheGenitive {
    XCTAssertEqualObjects([SPFileTranscriptionPolicy progressTextForDone:0 total:0], @"Готовлю звук…");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy progressTextForDone:0 total:1], @"Распознано 0 из 1 части");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy progressTextForDone:3 total:12], @"Распознано 3 из 12 частей");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy progressTextForDone:5 total:21], @"Распознано 5 из 21 части");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy progressTextForDone:5 total:11], @"Распознано 5 из 11 частей");
}

- (void)testDurationAndSaveNames {
    XCTAssertEqualObjects([SPFileTranscriptionPolicy durationText:47.3], @"0:47");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy durationText:3725], @"1:02:05");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy saveNameForSourceName:@"Голосовое.ogg" extension:@"srt"],
                          @"Голосовое.srt");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy saveNameForSourceName:@"" extension:@"txt"],
                          @"Расшифровка.txt");
}

- (void)testProviderTitles {
    XCTAssertEqualObjects([SPFileTranscriptionPolicy providerTitle:@"sherpa-onnx"], @"локальная модель");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy providerTitle:@"whisper-cpp"], @"локальная модель Whisper");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy providerTitle:@"qwen"], @"Qwen");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy providerTitle:@"future"], @"future");
}

- (void)testReadsTheCoreResult {
    SPFileTranscriptionOutcome *ok = [SPFileTranscriptionPolicy
        outcomeFromJSON:@"{\"provider\":\"qwen\",\"duration\":47.3,\"text\":\"Привет.\",\"segments\":[],"
                        @"\"srt\":\"1\\n00:00:00,000 --> 00:00:02,000\\nПривет.\\n\\n\"}"];
    XCTAssertEqualObjects(ok.status, @"ok");
    XCTAssertEqualObjects(ok.text, @"Привет.");
    XCTAssertEqualWithAccuracy(ok.duration, 47.3, 0.001);
    XCTAssertTrue([ok.srt hasPrefix:@"1\n00:00:00,000"]);

    SPFileTranscriptionOutcome *failed =
        [SPFileTranscriptionPolicy outcomeFromJSON:@"{\"error\":\"connect: api_key is required\"}"];
    XCTAssertEqualObjects(failed.status, @"error");
    XCTAssertEqualObjects(failed.error, @"connect: api_key is required");

    XCTAssertEqualObjects([SPFileTranscriptionPolicy outcomeFromJSON:@"{\"cancelled\":true}"].status, @"cancelled");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy outcomeFromJSON:@"nonsense"].status, @"error");
    XCTAssertEqualObjects([SPFileTranscriptionPolicy outcomeFromJSON:nil].status, @"error");
}

@end
