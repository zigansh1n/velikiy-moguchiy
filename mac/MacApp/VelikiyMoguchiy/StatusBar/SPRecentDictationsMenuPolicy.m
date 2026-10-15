#import "SPRecentDictationsMenuPolicy.h"

// A cut moves back to a space only this many characters, so a long first word
// still leaves a useful preview.
static const NSUInteger kWordBoundaryWindow = 15;

static BOOL isBlank(NSString *text) {
    return [text stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet].length == 0;
}

static NSString *daysWord(NSInteger days) {
    NSInteger mod10 = days % 10, mod100 = days % 100;
    if (mod10 == 1 && mod100 != 11) return @"день";
    if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return @"дня";
    return @"дней";
}

@implementation SPRecentDictationsMenuPolicy

+ (NSUInteger)itemLimit {
    return 5;
}

+ (NSUInteger)previewLength {
    return 50;
}

+ (NSArray<NSDictionary *> *)menuRowsFromHistoryRows:(NSArray<NSDictionary *> *)historyRows
                                               limit:(NSUInteger)limit {
    NSMutableArray<NSDictionary *> *rows = [NSMutableArray array];
    for (NSDictionary *row in historyRows) {
        if (rows.count >= limit) break;
        NSString *status = row[@"status"];
        NSString *text = row[@"text"];
        if (![status isKindOfClass:[NSString class]] || ![status isEqualToString:@"ok"]) continue;
        if (![text isKindOfClass:[NSString class]] || isBlank(text)) continue;
        [rows addObject:row];
    }
    return rows;
}

+ (NSString *)previewForText:(NSString *)text length:(NSUInteger)length {
    NSArray<NSString *> *words =
        [text componentsSeparatedByCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    NSString *line = [[words filteredArrayUsingPredicate:[NSPredicate predicateWithFormat:@"length > 0"]]
        componentsJoinedByString:@" "];
    if (length == 0) return @"";

    // Characters as the user sees them: an emoji or a letter with a combining
    // accent is one, never split in half.
    __block NSUInteger count = 0;
    __block NSUInteger keepEnd = 0;
    NSUInteger keep = length - 1;
    [line enumerateSubstringsInRange:NSMakeRange(0, line.length)
                             options:NSStringEnumerationByComposedCharacterSequences |
                                     NSStringEnumerationSubstringNotRequired
                          usingBlock:^(NSString *substring, NSRange range, NSRange enclosing, BOOL *stop) {
        count++;
        if (count <= keep) keepEnd = NSMaxRange(range);
        if (count > length) *stop = YES;
    }];
    if (count <= length) return line;

    NSString *head = [line substringToIndex:keepEnd];
    BOOL endsAtWord = [line characterAtIndex:keepEnd] == ' ';
    NSRange space = [head rangeOfString:@" " options:NSBackwardsSearch];
    if (!endsAtWord && space.location != NSNotFound && space.location > 0 &&
        head.length - space.location <= kWordBoundaryWindow) {
        head = [head substringToIndex:space.location];
    }
    NSMutableCharacterSet *trailing = [NSMutableCharacterSet whitespaceCharacterSet];
    [trailing addCharactersInString:@",;:—–-"];
    head = [head stringByTrimmingCharactersInSet:trailing];
    return [head stringByAppendingString:@"…"];
}

+ (NSString *)relativeTimeForDate:(NSDate *)date now:(NSDate *)now {
    NSTimeInterval elapsed = [now timeIntervalSinceDate:date];
    if (elapsed < 60) return @"только что";
    NSInteger minutes = (NSInteger)(elapsed / 60);
    if (minutes < 60) return [NSString stringWithFormat:@"%ld мин назад", (long)minutes];
    NSInteger hours = minutes / 60;
    if (hours < 24) return [NSString stringWithFormat:@"%ld ч назад", (long)hours];
    NSInteger days = hours / 24;
    return [NSString stringWithFormat:@"%ld %@ назад", (long)days, daysWord(days)];
}

@end
