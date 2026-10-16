#import <Cocoa/Cocoa.h>

@class SPRustBridge;

NS_ASSUME_NONNULL_BEGIN

@protocol SPModelBrowserDelegate <NSObject>
/// A model was added, downloaded or removed: model lists should reload.
/// Nothing is selected for the user.
- (void)modelBrowserDidChangeModels;
/// The user pressed «Сделать основной»: save `asr.provider` and its model
/// and apply them. Returns NO when they could not be saved.
- (BOOL)modelBrowserMakePrimaryModel:(NSString *)modelPath provider:(NSString *)provider;
@end

/// «Модели распознавания»: recommended models, Hugging Face search, models
/// already on this Mac used in place, and the user's own models.
@interface SPModelBrowserWindowController : NSWindowController

+ (void)showWithBridge:(SPRustBridge *)bridge
              delegate:(nullable id<SPModelBrowserDelegate>)delegate
          parentWindow:(nullable NSWindow *)parent;

@end

NS_ASSUME_NONNULL_END
