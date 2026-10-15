#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

typedef NS_ENUM(NSInteger, SPDictationPhase) {
    SPDictationPhaseIdle,
    SPDictationPhaseRecording,
    /// The recording has stopped; the text is not in the field yet.
    SPDictationPhaseProcessing,
};

typedef NS_ENUM(NSInteger, SPDictationOutcome) {
    SPDictationOutcomeNone,
    SPDictationOutcomeInserted,
    SPDictationOutcomeError,
    SPDictationOutcomeCancelled,
    /// The core ended the session with nothing to insert.
    SPDictationOutcomeNothing,
};

/// One dictation from the start of its recording to its result, as the pill
/// shows it: recording, then processing until the text is inserted, an error
/// is shown or the session is cancelled. Times are seconds on any one clock,
/// passed in so tests control them.
@interface SPDictationProgress : NSObject

@property (nonatomic, readonly) SPDictationPhase phase;
/// How the last dictation ended; None while one runs.
@property (nonatomic, readonly) SPDictationOutcome lastOutcome;
/// A press of a trigger now must not start a recording.
@property (nonatomic, readonly) BOOL shouldIgnoreTrigger;

/// Processing longer than this gets a line on the pill.
@property (class, nonatomic, readonly) NSTimeInterval hintDelay;
/// Longer than any provider's own timeout: past it the session is given up.
@property (class, nonatomic, readonly) NSTimeInterval giveUpDelay;
/// The line on the pill during a long processing.
@property (class, nonatomic, readonly) NSString *hintText;

- (void)recordingStarted;
/// YES when a recording was running and processing begins now.
- (BOOL)recordingStoppedAt:(NSTimeInterval)now;
/// Ends the dictation in flight; no-op when none is.
- (void)finishWithOutcome:(SPDictationOutcome)outcome;
/// Takes a state reported by the core, ending the dictation on a final one.
/// NO when the pill must not show it: a recording state that arrives after
/// the recording already stopped.
- (BOOL)applyCoreState:(NSString *)state;
/// The line for the pill, nil when none is due.
- (nullable NSString *)hintAt:(NSTimeInterval)now;
- (BOOL)hasTimedOutAt:(NSTimeInterval)now;

@end

NS_ASSUME_NONNULL_END
