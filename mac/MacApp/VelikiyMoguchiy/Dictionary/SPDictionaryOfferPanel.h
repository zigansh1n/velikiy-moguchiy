#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

/// A small card above the dictation pill: «Запомнить „Kubernetes“?» with
/// «Да» / «Нет». It never takes the keyboard focus and hides by itself.
@interface SPDictionaryOfferPanel : NSObject

/// Shows the offer for `term` (or replaces the one on screen). A term the user
/// declined in this run is not offered again.
- (void)offerTerm:(NSString *)term heard:(NSString *)heard;

- (void)dismiss;

/// Called after «Да» added the term to the dictionary.
@property (nonatomic, copy, nullable) void (^onAdded)(NSString *term);

@end

NS_ASSUME_NONNULL_END
