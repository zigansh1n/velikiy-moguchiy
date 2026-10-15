#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

/// «Расшифровать файл…»: one window per file, showing progress and then the
/// text with «Копировать», «Сохранить как .txt» and subtitles.
@interface SPFileTranscriptionWindowController : NSWindowController

/// Opens a window for each file worth transcribing among `urls`. Returns
/// how many were started.
+ (NSUInteger)transcribeURLs:(NSArray<NSURL *> *)urls;

/// Asks for files with an open panel, then transcribes them.
+ (void)chooseFilesAndTranscribe;

@end

NS_ASSUME_NONNULL_END
