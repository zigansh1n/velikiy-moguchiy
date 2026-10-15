#import "SPAccessRepairPolicy.h"

@implementation SPAccessRepairPolicy

+ (SPAccessRepairServices)brokenServicesWithTrust:(id<SPTrustProvider>)trust {
    SPAccessRepairServices broken = SPAccessRepairServiceNone;
    if (![trust isAccessibilityGranted]) broken |= SPAccessRepairServiceAccessibility;
    if (![trust isInputMonitoringGranted]) broken |= SPAccessRepairServiceInputMonitoring;
    return broken;
}

+ (nullable NSString *)noticeForServices:(SPAccessRepairServices)services {
    if (services & SPAccessRepairServiceAccessibility) return @"Доступ к управлению не работает — исправить";
    if (services & SPAccessRepairServiceInputMonitoring) return @"Мониторинг ввода не работает — исправить";
    return nil;
}

+ (nullable NSArray<NSString *> *)tccutilArgumentsForService:(SPAccessRepairServices)service
                                            bundleIdentifier:(nullable NSString *)bundleIdentifier {
    NSString *name = nil;
    if (service == SPAccessRepairServiceAccessibility) name = @"Accessibility";
    if (service == SPAccessRepairServiceInputMonitoring) name = @"ListenEvent";
    if (!name || bundleIdentifier.length == 0) return nil;
    NSRegularExpression *dotted =
        [NSRegularExpression regularExpressionWithPattern:@"^[A-Za-z0-9-]+(\\.[A-Za-z0-9-]+)+$" options:0 error:nil];
    if ([dotted numberOfMatchesInString:bundleIdentifier options:0 range:NSMakeRange(0, bundleIdentifier.length)] != 1) {
        return nil;
    }
    return @[ @"reset", name, bundleIdentifier ];
}

@end
