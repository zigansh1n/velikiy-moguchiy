#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// Follows readings of a field after a dictation was pasted into it and says
/// when the text has settled after an edit, so the edit can be compared with
/// the text right after the paste. No Accessibility, no timers.
@interface SPCorrectionTracker : NSObject

/// Readings in a row without the pasted text before the tracker gives up.
@property (class, nonatomic, readonly) NSUInteger maxReadingsWithoutPaste;
/// Equal readings in a row that count as settled.
@property (class, nonatomic, readonly) NSUInteger settledReadings;

- (instancetype)initWithInsertedText:(NSString *)text NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;

@property (nonatomic, copy, readonly) NSString *insertedText;
/// The field text the next settled text is compared with: the first reading
/// that holds the pasted text, moved on by `noCorrectionIn:`.
@property (nonatomic, copy, readonly, nullable) NSString *baseline;
/// The field never showed the pasted text (unreadable or already changed).
@property (nonatomic, assign, readonly) BOOL gaveUp;

/// Feeds one reading (nil when the field could not be read). Returns the text
/// to compare with `baseline` when it differs from it and has not changed for
/// `settledReadings` readings, once per settled text; nil otherwise.
- (nullable NSString *)settledTextAfterReading:(nullable NSString *)value;

/// The settled `text` held no correction. When the pasted text is still in it
/// whole, the edit was elsewhere (typing on after the paste), so `text`
/// becomes the new baseline; otherwise the baseline stays for the rest of a
/// correction in progress.
- (void)noCorrectionIn:(NSString *)text;

@end

NS_ASSUME_NONNULL_END
