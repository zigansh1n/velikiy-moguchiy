#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// What the core reported for one file job.
@interface SPFileTranscriptionOutcome : NSObject
/// "ok", "error" or "cancelled".
@property (nonatomic, copy) NSString *status;
@property (nonatomic, copy) NSString *text;
/// SubRip subtitles; empty when no speech was found.
@property (nonatomic, copy) NSString *srt;
@property (nonatomic, copy) NSString *provider;
@property (nonatomic, assign) NSTimeInterval duration;
/// The core's (English) error message for "error".
@property (nonatomic, copy) NSString *error;
@end

/// Decisions and wording of «Расшифровать файл…», kept apart from the window
/// so they can be unit tested.
@interface SPFileTranscriptionPolicy : NSObject

/// Extensions of Ogg Opus voice messages, which the core decodes itself.
+ (BOOL)isOggOpusURL:(NSURL *)url;

/// Audio, video, or an Ogg voice message: worth trying to transcribe.
+ (BOOL)canTranscribeURL:(NSURL *)url;

/// The files among `urls` worth transcribing.
+ (NSArray<NSURL *> *)transcribableURLs:(NSArray<NSURL *> *)urls;

/// "Распознано 3 из 12 частей"; "Готовлю звук…" before the first count.
+ (NSString *)progressTextForDone:(NSUInteger)done total:(NSUInteger)total;

/// "0:47", "12:05", "1:02:05".
+ (NSString *)durationText:(NSTimeInterval)seconds;

/// "Голосовое сообщение.ogg" + "srt" → "Голосовое сообщение.srt".
+ (NSString *)saveNameForSourceName:(NSString *)name extension:(NSString *)extension;

/// How the subtitle names the provider that did the work: "sherpa-onnx" →
/// "локальная модель".
+ (NSString *)providerTitle:(NSString *)provider;

/// The JSON of `sp_file_transcribe_start`'s completion callback.
+ (SPFileTranscriptionOutcome *)outcomeFromJSON:(nullable NSString *)json;

@end

NS_ASSUME_NONNULL_END
