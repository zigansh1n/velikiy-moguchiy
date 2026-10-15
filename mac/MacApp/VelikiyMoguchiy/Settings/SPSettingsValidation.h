#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// Why a `hotkey.trigger_key` value cannot be the dictation key: a bare typing
/// key, which the trigger would swallow everywhere. Nil when it can.
NSString *_Nullable SPTriggerKeyProblem(NSString *triggerValue);

/// The first two roles that share one key, as a sentence; nil when every
/// non-empty key differs. `roles` are @[title, config value] pairs in the
/// order the page shows them.
NSString *_Nullable SPSharedHotkeyProblem(NSArray<NSArray<NSString *> *> *roles);

/// What is missing for `provider` to work, given its keys as typed
/// (@"api_key", @"api_keys" joined by newlines, @"app_key", @"access_key");
/// nil when nothing is, or the provider needs no key.
NSString *_Nullable SPMissingProviderKeyProblem(NSString *provider,
                                                NSDictionary<NSString *, NSString *> *keys);

/// Title and explanation for a config.yaml that does not parse, from the
/// core's {line, column, message, backup}.
NSString *SPConfigProblemTitle(NSDictionary *problem);
NSString *SPConfigProblemDetail(NSDictionary *problem);

NS_ASSUME_NONNULL_END
