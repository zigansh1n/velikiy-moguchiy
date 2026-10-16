#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// The text the model window and the model check show, from the JSON
/// the core returns. No AppKit, so the logic tests can cover it.
@interface SPHfModelPresenter : NSObject

/// The variants of an inspection, the suggested one first; empty when the
/// repository has nothing to run.
+ (NSArray<NSDictionary *> *)variantsOfInspection:(NSDictionary *)inspection;

/// A popup title: «GigaAM (трансдьюсер) · gigaam_v3_e2e_rnnt · 330 МБ».
+ (NSString *)menuTitleForVariant:(NSDictionary *)variant;

/// (caption, value) rows for the chosen variant: type, size, languages, licence, commit.
+ (NSArray<NSArray<NSString *> *> *)rowsForInspection:(NSDictionary *)inspection
                                              variant:(nullable NSDictionary *)variant;

/// Warnings to show before the download, one per line; empty when none.
+ (NSString *)warningsTextForInspection:(NSDictionary *)inspection;

/// Why nothing can be installed and what to search for; nil when something can.
+ (nullable NSString *)problemTextForInspection:(NSDictionary *)inspection;

/// A download failure from the core, in the window's words.
+ (NSString *)textForDownloadError:(NSString *)message;

/// Reads what the check child printed. `exitedNormally` is NO when it was
/// killed by a signal (sherpa-onnx aborting on a model it cannot load).
+ (NSDictionary *)checkResultFromOutput:(nullable NSData *)output
                                 status:(int)status
                         exitedNormally:(BOOL)exitedNormally;

/// «Работает: «…» (за 1,8 с)» or why it does not.
+ (NSString *)summaryForCheckResult:(NSDictionary *)result;

@end

/// The line prefix the check child puts before its JSON result.
extern NSString *const SPLocalModelCheckResultPrefix;

NS_ASSUME_NONNULL_END
