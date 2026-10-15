#import <Foundation/Foundation.h>

/// CSV in the dialect Russian-locale Excel opens without the import wizard:
/// `;` between fields (the list separator there; a comma would leave everything
/// in one column) and a decimal comma in numbers.
@interface SPDictationsCSV : NSObject

+ (NSString *)headerLine;

/// One CRLF-terminated row. `durationMs` is written in seconds with one decimal
/// and a comma as the decimal mark. `type` is the Russian name of the session's
/// purpose; `original` is the selection an Ask or Edit worked on, or the text a
/// re-transcription replaced.
+ (NSString *)rowWithDate:(NSString *)date
               durationMs:(double)durationMs
                     type:(NSString *)type
                   status:(NSString *)status
                     text:(NSString *)text
                 original:(NSString *)original
                     file:(NSString *)file;

/// Quote a field: wrapped in double quotes, inner quotes doubled. A value that
/// starts with `=`, `+`, `-`, `@`, a tab or a carriage return gets a leading
/// `'`, so a spreadsheet shows it as text instead of running it as a formula
/// («+7 999…», «=HYPERLINK(…)»).
+ (NSString *)quotedField:(NSString *)value;

@end
