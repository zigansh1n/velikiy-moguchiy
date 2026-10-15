#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// Permissions whose grant can go stale: macOS keeps the switch on for an
/// earlier signature of the app and refuses this one, and switching it on
/// again snaps back until the row is reset.
typedef NS_OPTIONS(NSUInteger, SPAccessRepairServices) {
    SPAccessRepairServiceNone = 0,
    SPAccessRepairServiceAccessibility = 1 << 0,
    SPAccessRepairServiceInputMonitoring = 1 << 1,
};

/// What the system says about the app right now.
@protocol SPTrustProvider <NSObject>
/// AXIsProcessTrusted().
- (BOOL)isAccessibilityGranted;
/// Whether the app can listen to key events (TCC service ListenEvent).
- (BOOL)isInputMonitoringGranted;
@end

/// Pure decisions of the «Исправить» repair, apart from TCC so they can be unit tested.
@interface SPAccessRepairPolicy : NSObject

/// The services the app needs and the system refuses now.
+ (SPAccessRepairServices)brokenServicesWithTrust:(id<SPTrustProvider>)trust;

/// The notice for the menu: «Доступ к управлению не работает — исправить»,
/// or nil when nothing is broken.
+ (nullable NSString *)noticeForServices:(SPAccessRepairServices)services;

/// `/usr/bin/tccutil` arguments that reset one service for this bundle id only.
/// Nil for anything but exactly one service, or for a bundle id that is not a
/// dotted name: without one, tccutil resets the service for every app.
+ (nullable NSArray<NSString *> *)tccutilArgumentsForService:(SPAccessRepairServices)service
                                            bundleIdentifier:(nullable NSString *)bundleIdentifier;

@end

NS_ASSUME_NONNULL_END
