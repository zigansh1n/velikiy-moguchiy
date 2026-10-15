#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

/// Posted on the main queue after the app itself changed dictionary.txt.
extern NSNotificationName const SPDictionaryDidChangeNotification;

/// «Добавить слово в словарь…»: the dictation's text to pick a word from, the
/// correct spelling and how it is pronounced.
@interface SPAddWordDialog : NSObject

/// The word to prefill when nothing is selected: the first one with a Latin
/// letter or a digit (the likeliest term in Russian speech), else nil.
+ (nullable NSString *)guessedTermInText:(nullable NSString *)text;

/// Shows the dialog as a sheet on `window`, or app-modal when it is nil.
/// `word` prefills the spelling (a guess from `text` when nil). `completion`
/// gets the added term, or nil when the user cancelled or nothing changed,
/// and a message for the user.
+ (void)runWithText:(nullable NSString *)text
               word:(nullable NSString *)word
             window:(nullable NSWindow *)window
         completion:(nullable void (^)(NSString *_Nullable term, NSString *message))completion;

@end

NS_ASSUME_NONNULL_END
