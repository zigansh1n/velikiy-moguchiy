#import "SPDictationProgress.h"

@implementation SPDictationProgress {
    NSTimeInterval _processingStartedAt;
}

+ (NSTimeInterval)hintDelay {
    return 8.0;
}

// The ChatGPT upload gives up after 120 s and a correction after it has its own timeout.
+ (NSTimeInterval)giveUpDelay {
    return 180.0;
}

+ (NSString *)hintText {
    return @"Распознаю…";
}

- (BOOL)shouldIgnoreTrigger {
    return self.phase == SPDictationPhaseProcessing;
}

- (void)recordingStarted {
    _phase = SPDictationPhaseRecording;
    _lastOutcome = SPDictationOutcomeNone;
}

- (BOOL)recordingStoppedAt:(NSTimeInterval)now {
    if (self.phase != SPDictationPhaseRecording) return NO;
    _phase = SPDictationPhaseProcessing;
    _processingStartedAt = now;
    return YES;
}

- (void)finishWithOutcome:(SPDictationOutcome)outcome {
    if (self.phase == SPDictationPhaseIdle) return;
    _phase = SPDictationPhaseIdle;
    _lastOutcome = outcome;
}

- (BOOL)applyCoreState:(NSString *)state {
    if ([state hasPrefix:@"recording"]) {
        return self.phase != SPDictationPhaseProcessing;
    }
    if ([state isEqualToString:@"preparing_paste"] || [state isEqualToString:@"pasting"] ||
        [state isEqualToString:@"completed"]) {
        [self finishWithOutcome:SPDictationOutcomeInserted];
    } else if ([state isEqualToString:@"failed"]) {
        [self finishWithOutcome:SPDictationOutcomeError];
    } else if ([state isEqualToString:@"cancelled"]) {
        [self finishWithOutcome:SPDictationOutcomeCancelled];
    } else if ([state isEqualToString:@"idle"] || [state isEqualToString:@"no_speech"]) {
        [self finishWithOutcome:SPDictationOutcomeNothing];
    }
    return YES;
}

- (NSString *)hintAt:(NSTimeInterval)now {
    if (self.phase != SPDictationPhaseProcessing) return nil;
    return now - _processingStartedAt >= SPDictationProgress.hintDelay ? SPDictationProgress.hintText : nil;
}

- (BOOL)hasTimedOutAt:(NSTimeInterval)now {
    return self.phase == SPDictationPhaseProcessing && now - _processingStartedAt >= SPDictationProgress.giveUpDelay;
}

@end
