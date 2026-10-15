#import <Foundation/Foundation.h>
#import "SPTextEditPolicy.h"

NS_ASSUME_NONNULL_BEGIN

/// A one-press key whose action is hard to take back (undo the insertion,
/// paste the last dictation again) acts on the second press within `window`;
/// a lone press, such as a stray touch of a modifier, only arms it.
@interface SPDoublePressGate : NSObject
- (instancetype)initWithWindow:(NSTimeInterval)window;
/// YES when this press, at `time` seconds on any monotonic clock, completes a
/// double press; the gate is then disarmed again.
- (BOOL)pressAt:(NSTimeInterval)time;
@end

/// «Для перевода нужен ключ DashScope» for a translate, ask or edit key
/// pressed without a DashScope key; nil for dictation.
NSString *_Nullable SPAssistantNeedsKeyMessage(SPSessionPurpose purpose);

/// One line for «Начало работы» naming the translate and ask keys
/// (`hotkey.translate_key`, `hotkey.ask_key` values); nil when neither is set
/// or named.
NSString *_Nullable SPVoiceKeysSummary(NSString *translateKey, NSString *askKey, NSString *translateTo);

NS_ASSUME_NONNULL_END
