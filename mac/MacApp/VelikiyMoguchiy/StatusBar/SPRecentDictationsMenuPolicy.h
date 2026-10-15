#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// Rules of the «Последние диктовки» section of the status-bar menu, kept free
/// of AppKit so they can be unit-tested.
@interface SPRecentDictationsMenuPolicy : NSObject

/// How many dictations the menu lists.
@property (class, nonatomic, readonly) NSUInteger itemLimit;
/// Preview length in characters, the ellipsis included.
@property (class, nonatomic, readonly) NSUInteger previewLength;

/// The rows the menu lists, newest first: successful sessions with text that
/// is not blank, at most `limit`. `historyRows` are rows of
/// -[SPHistoryManager recentSessionsWithLimit:], newest first.
+ (NSArray<NSDictionary *> *)menuRowsFromHistoryRows:(NSArray<NSDictionary *> *)historyRows
                                               limit:(NSUInteger)limit;

/// One line of `text`: whitespace runs (line breaks included) become one
/// space; a longer text is cut to at most `length` characters with «…» at the
/// end, at a word boundary when one is near.
+ (NSString *)previewForText:(NSString *)text length:(NSUInteger)length;

/// «только что», «5 мин назад», «3 ч назад», «1 день назад», «2 дня назад»,
/// «5 дней назад». A `date` after `now` counts as «только что».
+ (NSString *)relativeTimeForDate:(NSDate *)date now:(NSDate *)now;

@end

NS_ASSUME_NONNULL_END
