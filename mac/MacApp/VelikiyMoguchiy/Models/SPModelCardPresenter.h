#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// The one button a model card leads with.
typedef NS_ENUM(NSInteger, SPModelAction) {
    SPModelActionNone,
    SPModelActionDownload,
    SPModelActionResume,
    SPModelActionStop,
    SPModelActionMakePrimary,
    SPModelActionUseInPlace,
};

/// The text and buttons of the cards in «Модели распознавания», from the JSON
/// the core returns. No AppKit, so the logic tests can cover it.
@interface SPModelCardPresenter : NSObject

/// «Скачать», «Продолжить», «Остановить», «Сделать основной», «Использовать на месте».
+ (NSString *)titleForAction:(SPModelAction)action;

/// «sherpa-onnx», «whisper.cpp», «MLX».
+ (NSString *)engineNameForProvider:(nullable NSString *)provider;

/// Whether dictation uses this model: `asr.provider` names its engine and
/// that engine's `model` names its path.
+ (BOOL)isPrimaryModel:(nullable NSString *)modelPath
              provider:(nullable NSString *)provider
        configProvider:(nullable NSString *)configProvider
           configModel:(nullable NSString *)configModel;

/// «русский», «русский, английский», «12 языков, включая русский».
+ (NSString *)languagesTextForCodes:(NSArray *)codes;

/// 950, «12 тыс.», «1,2 млн».
+ (NSString *)countText:(uint64_t)count;

/// «~/.cache/…» for paths in the home folder.
+ (NSString *)displayPath:(NSString *)path home:(NSString *)home;

#pragma mark Рекомендуемые

/// «232 МБ · качество высокая · быстро · текст после паузы · русский · MIT».
+ (NSString *)detailLineForRecommended:(NSDictionary *)entry;
+ (SPModelAction)actionForRecommended:(NSDictionary *)entry downloading:(BOOL)downloading primary:(BOOL)primary;

#pragma mark Поиск

/// «↓ 12 тыс. · ♥ 34 · русский, английский · MIT».
+ (NSString *)detailLineForSearchItem:(NSDictionary *)item;
/// «Найдено: 12. Ещё 5 без русского скрыты.»
+ (NSString *)summaryForSearchResults:(NSDictionary *)results;

#pragma mark На этом Mac

/// «Кэш Hugging Face · ~/.cache/huggingface/hub/…».
+ (NSString *)sourceLineForFind:(NSDictionary *)find home:(NSString *)home;
/// «GigaAM (MLX) · 891 МБ · русский, английский · MIT».
+ (NSString *)detailLineForFind:(NSDictionary *)find variant:(nullable NSDictionary *)variant;
/// The model path a variant was added as, or nil.
+ (nullable NSString *)addedModelPathForFind:(NSDictionary *)find variant:(nullable NSDictionary *)variant;
+ (SPModelAction)actionForFind:(NSDictionary *)find variant:(nullable NSDictionary *)variant primary:(BOOL)primary;

#pragma mark Мои модели

/// «Встроенная», «С Hugging Face», «На этом Mac: ~/…», «Папка пропала: ~/…».
+ (NSString *)originLineForModel:(NSDictionary *)model home:(NSString *)home;
/// «232 МБ · русский · MIT».
+ (NSString *)detailLineForModel:(NSDictionary *)model;
/// «Удалить» or «Убрать из списка».
+ (NSString *)removalTitleForModel:(NSDictionary *)model;
+ (NSString *)removalQuestionForModel:(NSDictionary *)model;
+ (NSString *)removalExplanationForModel:(NSDictionary *)model primary:(BOOL)primary;
+ (SPModelAction)actionForModel:(NSDictionary *)model downloading:(BOOL)downloading primary:(BOOL)primary;

@end

NS_ASSUME_NONNULL_END
