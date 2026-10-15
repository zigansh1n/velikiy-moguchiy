#import "SPDictationsCSV.h"

@implementation SPDictationsCSV

+ (NSString *)headerLine {
    return @"date;duration_s;type;status;text;original_text;file\r\n";
}

+ (NSString *)quotedField:(NSString *)value {
    value = value ?: @"";
    static NSCharacterSet *formulaStarts;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        formulaStarts = [NSCharacterSet characterSetWithCharactersInString:@"=+-@\t\r"];
    });
    if (value.length > 0 && [formulaStarts characterIsMember:[value characterAtIndex:0]]) {
        value = [@"'" stringByAppendingString:value];
    }
    return [NSString stringWithFormat:@"\"%@\"",
            [value stringByReplacingOccurrencesOfString:@"\"" withString:@"\"\""]];
}

+ (NSString *)rowWithDate:(NSString *)date
               durationMs:(double)durationMs
                     type:(NSString *)type
                   status:(NSString *)status
                     text:(NSString *)text
                 original:(NSString *)original
                     file:(NSString *)file {
    // %.1f always uses '.' (no locale), so swap in the decimal comma by hand.
    NSString *seconds = [[NSString stringWithFormat:@"%.1f", durationMs / 1000.0]
        stringByReplacingOccurrencesOfString:@"." withString:@","];
    return [NSString stringWithFormat:@"%@;%@;%@;%@;%@;%@;%@\r\n",
            [self quotedField:date],
            [self quotedField:seconds],
            [self quotedField:type],
            [self quotedField:status],
            [self quotedField:text],
            [self quotedField:original],
            [self quotedField:file]];
}

@end
