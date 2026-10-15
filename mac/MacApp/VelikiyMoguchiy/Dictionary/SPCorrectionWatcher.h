#import <Foundation/Foundation.h>

@class SPInsertionRecord;

NS_ASSUME_NONNULL_BEGIN

/// After a dictation is pasted, reads the field once a second for about half
/// a minute and reports a word the user corrected in the pasted text
/// (velikiy-moguchiy-core dictionary::detect_correction).
@interface SPCorrectionWatcher : NSObject

/// Called on the main queue with the corrected spelling and the pasted word
/// it replaced (empty when that is not a Cyrillic spoken form). May be called
/// again for the same insertion when the user keeps editing.
@property (nonatomic, copy, nullable) void (^onCorrection)(NSString *term, NSString *heard);

/// Starts watching `record`'s field, ending any earlier watch.
- (void)watchInsertion:(SPInsertionRecord *)record;

- (void)stop;

@end

NS_ASSUME_NONNULL_END
