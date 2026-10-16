#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// «Проверить»: the built-in two-second recording through a local model. It
/// runs in a child of this executable, because sherpa-onnx exits
/// the whole process when a model file is not what it expects.
@interface SPLocalModelCheck : NSObject

/// In main(), before AppKit: when this process is the check child, runs the
/// check, prints the result and returns the exit status; otherwise -1.
+ (int)runChildIfRequestedWithArgc:(int)argc argv:(const char *_Nonnull[_Nonnull])argv;

/// Checks the model at `modelPath` (relative to the models folder) and calls
/// `completion` on the main queue with {ok, text, seconds, error}.
+ (void)checkModelAtPath:(NSString *)modelPath completion:(void (^)(NSDictionary *result))completion;

@end

NS_ASSUME_NONNULL_END
