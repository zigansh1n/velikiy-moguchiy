#import <XCTest/XCTest.h>
#import "SPDictationProgress.h"

@interface SPDictationProgressTests : XCTestCase
@end

@implementation SPDictationProgressTests

- (SPDictationProgress *)processingSince:(NSTimeInterval)start {
    SPDictationProgress *progress = [[SPDictationProgress alloc] init];
    [progress recordingStarted];
    XCTAssertTrue([progress recordingStoppedAt:start]);
    XCTAssertEqual(progress.phase, SPDictationPhaseProcessing);
    return progress;
}

- (void)testRecordingThenProcessingThenInserted {
    SPDictationProgress *progress = [[SPDictationProgress alloc] init];
    XCTAssertEqual(progress.phase, SPDictationPhaseIdle);
    [progress recordingStarted];
    XCTAssertEqual(progress.phase, SPDictationPhaseRecording);
    XCTAssertFalse(progress.shouldIgnoreTrigger);
    XCTAssertTrue([progress recordingStoppedAt:100]);
    XCTAssertEqual(progress.phase, SPDictationPhaseProcessing);
    XCTAssertTrue(progress.shouldIgnoreTrigger);

    XCTAssertTrue([progress applyCoreState:@"finalizing_asr"]);
    XCTAssertTrue([progress applyCoreState:@"correcting"]);
    XCTAssertEqual(progress.phase, SPDictationPhaseProcessing);

    XCTAssertTrue([progress applyCoreState:@"preparing_paste"]);
    XCTAssertEqual(progress.phase, SPDictationPhaseIdle);
    XCTAssertEqual(progress.lastOutcome, SPDictationOutcomeInserted);
    XCTAssertFalse(progress.shouldIgnoreTrigger);
}

- (void)testProcessingEndsInAnError {
    SPDictationProgress *progress = [self processingSince:0];
    [progress applyCoreState:@"failed"];
    XCTAssertEqual(progress.phase, SPDictationPhaseIdle);
    XCTAssertEqual(progress.lastOutcome, SPDictationOutcomeError);

    progress = [self processingSince:0];
    [progress finishWithOutcome:SPDictationOutcomeError];
    XCTAssertEqual(progress.lastOutcome, SPDictationOutcomeError);
}

- (void)testProcessingEndsCancelled {
    SPDictationProgress *progress = [self processingSince:0];
    [progress applyCoreState:@"cancelled"];
    XCTAssertEqual(progress.lastOutcome, SPDictationOutcomeCancelled);
    // The core's idle after the cancel keeps the outcome.
    [progress applyCoreState:@"idle"];
    XCTAssertEqual(progress.lastOutcome, SPDictationOutcomeCancelled);
    XCTAssertEqual(progress.phase, SPDictationPhaseIdle);
}

- (void)testIdleWithoutAResultEndsProcessing {
    SPDictationProgress *progress = [self processingSince:0];
    [progress applyCoreState:@"idle"];
    XCTAssertEqual(progress.phase, SPDictationPhaseIdle);
    XCTAssertEqual(progress.lastOutcome, SPDictationOutcomeNothing);
}

- (void)testALateRecordingStateDoesNotTakeBackProcessing {
    SPDictationProgress *progress = [self processingSince:0];
    XCTAssertFalse([progress applyCoreState:@"recording_toggle"]);
    XCTAssertEqual(progress.phase, SPDictationPhaseProcessing);

    SPDictationProgress *recording = [[SPDictationProgress alloc] init];
    [recording recordingStarted];
    XCTAssertTrue([recording applyCoreState:@"recording_hold"]);
}

- (void)testStoppingWithoutARecordingIsNotProcessing {
    SPDictationProgress *progress = [[SPDictationProgress alloc] init];
    XCTAssertFalse([progress recordingStoppedAt:0]);
    XCTAssertEqual(progress.phase, SPDictationPhaseIdle);

    // A session that failed to start was finished before its key came up.
    [progress recordingStarted];
    [progress finishWithOutcome:SPDictationOutcomeError];
    XCTAssertFalse([progress recordingStoppedAt:1]);
    XCTAssertFalse(progress.shouldIgnoreTrigger);

    // A second stop of the same recording starts nothing new.
    progress = [self processingSince:0];
    XCTAssertFalse([progress recordingStoppedAt:5]);
}

- (void)testAHintOnlyAfterALongWait {
    SPDictationProgress *progress = [self processingSince:10];
    XCTAssertNil([progress hintAt:10]);
    XCTAssertNil([progress hintAt:10 + SPDictationProgress.hintDelay - 0.1]);
    XCTAssertEqualObjects([progress hintAt:10 + SPDictationProgress.hintDelay], @"Распознаю…");
    [progress finishWithOutcome:SPDictationOutcomeInserted];
    XCTAssertNil([progress hintAt:100]);
}

- (void)testGivesUpAfterTheProvidersTimeout {
    XCTAssertGreaterThan(SPDictationProgress.giveUpDelay, 120.0);
    SPDictationProgress *progress = [self processingSince:0];
    XCTAssertFalse([progress hasTimedOutAt:SPDictationProgress.giveUpDelay - 1]);
    XCTAssertTrue([progress hasTimedOutAt:SPDictationProgress.giveUpDelay]);
    [progress finishWithOutcome:SPDictationOutcomeError];
    XCTAssertFalse([progress hasTimedOutAt:SPDictationProgress.giveUpDelay * 2]);
}

- (void)testANewRecordingClearsTheLastOutcome {
    SPDictationProgress *progress = [self processingSince:0];
    [progress finishWithOutcome:SPDictationOutcomeInserted];
    [progress recordingStarted];
    XCTAssertEqual(progress.lastOutcome, SPDictationOutcomeNone);
    XCTAssertEqual(progress.phase, SPDictationPhaseRecording);
}

@end
