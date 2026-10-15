#import <Foundation/Foundation.h>

/// Posted on the main queue whenever a session row is added, removed or
/// changed.
extern NSNotificationName const SPHistoryDidChangeNotification;

/// Values of a session's `purpose`.
extern NSString *const SPHistoryPurposeDictate;
extern NSString *const SPHistoryPurposeTranslate;
extern NSString *const SPHistoryPurposeAsk;
extern NSString *const SPHistoryPurposeEdit;

@interface SPHistoryStats : NSObject

@property (nonatomic, assign) NSInteger sessionCount;
@property (nonatomic, assign) NSInteger totalDurationMs;
@property (nonatomic, assign) NSInteger totalCharCount;
@property (nonatomic, assign) NSInteger totalWordCount;

@end

@interface SPHistoryManager : NSObject

+ (instancetype)sharedManager;

/// Open a history database at a custom path (used by tests; the shared
/// manager uses ~/.velikiy-moguchiy/history.db). The parent directory is created if needed.
- (instancetype)initWithDatabasePath:(NSString *)dbPath;

/// Record a completed session.
/// `text` is the final (possibly LLM-corrected) text that was pasted.
/// `asrText` is the raw ASR transcript before LLM correction (nil if unknown).
/// `asrProvider` is the ASR provider name, e.g. "qwen" (nil if unknown).
/// `llmApplied` is YES only when LLM correction ran and its output was used.
- (void)recordSessionWithDurationMs:(NSInteger)durationMs
                               text:(NSString *)text
                            asrText:(NSString *)asrText
                        asrProvider:(NSString *)asrProvider
                         llmApplied:(BOOL)llmApplied;

/// Record a session with an explicit outcome. `status` is "ok", "cancelled",
/// "error" or "empty"; `audioPath` (nil if none) links the saved recording.
/// A session with empty `text` is stored only when it has an `audioPath`.
- (void)recordSessionWithDurationMs:(NSInteger)durationMs
                               text:(NSString *)text
                            asrText:(NSString *)asrText
                        asrProvider:(NSString *)asrProvider
                         llmApplied:(BOOL)llmApplied
                             status:(NSString *)status
                          audioPath:(NSString *)audioPath;

/// Record a session with an explicit outcome and returns its row id (0 when
/// nothing was stored). `error` is the human-readable reason of an "error" or
/// "empty" outcome (first line = short Russian summary, the rest = the
/// provider's message); `appBundleId` is the app that was frontmost when the
/// session began. Both may be nil.
- (int64_t)recordSessionWithDurationMs:(NSInteger)durationMs
                                  text:(NSString *)text
                               asrText:(NSString *)asrText
                           asrProvider:(NSString *)asrProvider
                            llmApplied:(BOOL)llmApplied
                                status:(NSString *)status
                             audioPath:(NSString *)audioPath
                                 error:(NSString *)error
                           appBundleId:(NSString *)appBundleId;

/// Same, for a voice edit: `originalText` is the selected text the result
/// replaced; it is stored in `original_text` (nil / empty for other sessions).
- (int64_t)recordSessionWithDurationMs:(NSInteger)durationMs
                                  text:(NSString *)text
                               asrText:(NSString *)asrText
                           asrProvider:(NSString *)asrProvider
                            llmApplied:(BOOL)llmApplied
                                status:(NSString *)status
                             audioPath:(NSString *)audioPath
                                 error:(NSString *)error
                           appBundleId:(NSString *)appBundleId
                          originalText:(NSString *)originalText;

/// Same, with the session's purpose (`SPHistoryPurpose…`; nil is a
/// dictation). For an Ask, `originalText` is the selection the question was
/// about; for an Edit, the selection the result replaced.
- (int64_t)recordSessionWithDurationMs:(NSInteger)durationMs
                                  text:(NSString *)text
                               asrText:(NSString *)asrText
                           asrProvider:(NSString *)asrProvider
                            llmApplied:(BOOL)llmApplied
                                status:(NSString *)status
                             audioPath:(NSString *)audioPath
                                 error:(NSString *)error
                           appBundleId:(NSString *)appBundleId
                          originalText:(NSString *)originalText
                               purpose:(NSString *)purpose;

/// The `SPHistoryPurpose…` value of an sp_core_set_next_session_purpose code
/// (0 dictate, 1 translate, 2 ask, 3 edit); unknown codes are dictations.
+ (NSString *)purposeForCode:(NSInteger)code;

/// Russian name of a purpose for the history list: «диктовка», «перевод»,
/// «вопрос», «правка».
+ (NSString *)titleForPurpose:(NSString *)purpose;

/// Replace the outcome of a stored session (re-transcription). A nil `text`
/// or `status` leaves that column unchanged; `error` is always written (nil
/// clears it). Replacing a non-empty text with a different one keeps the first
/// text in `original_text`; the raw ASR transcript of a replaced text is
/// dropped so dictionary suggestions never compare it with the new text.
/// Returns YES when the row exists.
- (BOOL)updateSessionWithId:(int64_t)sessionId
                       text:(NSString *)text
                     status:(NSString *)status
                      error:(NSString *)error;

/// Removes API keys (`sk-…`) and bearer tokens from an error message.
+ (NSString *)scrubbedSecrets:(NSString *)message;

/// "<summary>\n<provider message>" with secrets scrubbed and the provider
/// message cut to about 300 characters. Either part may be empty.
+ (NSString *)errorReasonWithSummary:(NSString *)summary providerMessage:(NSString *)providerMessage;

/// First line of a stored reason: the short summary shown in the list.
+ (NSString *)shortErrorReason:(NSString *)error;

/// Aggregates cover successful dictations and translations only: an Ask
/// answer or an Edit result is not text the user dictated.
- (SPHistoryStats *)aggregateStats;

/// The most recent sessions, newest first. Each entry is a dictionary with
/// `id` (NSNumber), `timestamp` (NSDate), `duration_ms` (NSNumber), `text`,
/// `status`, `audio_path`, `error`, `app_bundle_id`, `original_text`
/// (NSString, empty when there is no recording / reason / app / original) and
/// `purpose` (an `SPHistoryPurpose…` value).
- (NSArray<NSDictionary *> *)recentSessionsWithLimit:(NSUInteger)limit;

/// Sessions of the whole history whose text or original text contains
/// `query`, ignoring case and diacritics (so «е» finds «ё»), newest first.
/// Same entries as `recentSessionsWithLimit:`.
- (NSArray<NSDictionary *> *)sessionsMatchingQuery:(NSString *)query limit:(NSUInteger)limit;

/// Number of stored sessions.
- (NSInteger)sessionCount;

- (void)deleteSessionWithId:(int64_t)sessionId;

/// Removes every session row (recording files are the caller's business).
- (void)deleteAllSessions;

/// Called after a recording file was deleted: clears `audio_path` of the rows
/// that referenced it and drops those that had no text of their own.
- (void)detachAudioPath:(NSString *)audioPath;

@end
