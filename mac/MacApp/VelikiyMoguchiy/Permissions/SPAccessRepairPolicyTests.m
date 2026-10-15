#import <XCTest/XCTest.h>
#import "SPAccessRepairPolicy.h"

@interface SPFakeTrustProvider : NSObject <SPTrustProvider>
@property (nonatomic) BOOL accessibility;
@property (nonatomic) BOOL inputMonitoring;
@end

@implementation SPFakeTrustProvider
- (BOOL)isAccessibilityGranted { return self.accessibility; }
- (BOOL)isInputMonitoringGranted { return self.inputMonitoring; }
@end

@interface SPAccessRepairPolicyTests : XCTestCase
@end

@implementation SPAccessRepairPolicyTests

- (SPAccessRepairServices)brokenWithAccessibility:(BOOL)accessibility inputMonitoring:(BOOL)inputMonitoring {
    SPFakeTrustProvider *trust = [SPFakeTrustProvider new];
    trust.accessibility = accessibility;
    trust.inputMonitoring = inputMonitoring;
    return [SPAccessRepairPolicy brokenServicesWithTrust:trust];
}

- (void)testNoNoticeWhileEverythingIsGranted {
    SPAccessRepairServices broken = [self brokenWithAccessibility:YES inputMonitoring:YES];
    XCTAssertEqual(broken, SPAccessRepairServiceNone);
    XCTAssertNil([SPAccessRepairPolicy noticeForServices:broken]);
}

- (void)testUntrustedAccessibilityShowsTheNotice {
    SPAccessRepairServices broken = [self brokenWithAccessibility:NO inputMonitoring:YES];
    XCTAssertEqual(broken, SPAccessRepairServiceAccessibility);
    XCTAssertEqualObjects([SPAccessRepairPolicy noticeForServices:broken],
                          @"Доступ к управлению не работает — исправить");
}

- (void)testInputMonitoringAloneHasItsOwnNotice {
    SPAccessRepairServices broken = [self brokenWithAccessibility:YES inputMonitoring:NO];
    XCTAssertEqual(broken, SPAccessRepairServiceInputMonitoring);
    XCTAssertEqualObjects([SPAccessRepairPolicy noticeForServices:broken],
                          @"Мониторинг ввода не работает — исправить");
}

- (void)testBothBrokenRepairsBothAndNamesAccessibility {
    SPAccessRepairServices broken = [self brokenWithAccessibility:NO inputMonitoring:NO];
    XCTAssertEqual(broken, SPAccessRepairServiceAccessibility | SPAccessRepairServiceInputMonitoring);
    XCTAssertEqualObjects([SPAccessRepairPolicy noticeForServices:broken],
                          @"Доступ к управлению не работает — исправить");
}

- (void)testResetNamesOneServiceAndOurBundleOnly {
    NSString *bundle = @"io.github.zigansh1n.velikiy-moguchiy";
    XCTAssertEqualObjects([SPAccessRepairPolicy tccutilArgumentsForService:SPAccessRepairServiceAccessibility
                                                          bundleIdentifier:bundle],
                          (@[ @"reset", @"Accessibility", bundle ]));
    XCTAssertEqualObjects([SPAccessRepairPolicy tccutilArgumentsForService:SPAccessRepairServiceInputMonitoring
                                                          bundleIdentifier:bundle],
                          (@[ @"reset", @"ListenEvent", bundle ]));
}

- (void)testNeverResetsAWholeService {
    NSString *bundle = @"io.github.zigansh1n.velikiy-moguchiy";
    XCTAssertNil([SPAccessRepairPolicy tccutilArgumentsForService:SPAccessRepairServiceAccessibility bundleIdentifier:nil]);
    XCTAssertNil([SPAccessRepairPolicy tccutilArgumentsForService:SPAccessRepairServiceAccessibility bundleIdentifier:@""]);
    XCTAssertNil([SPAccessRepairPolicy tccutilArgumentsForService:SPAccessRepairServiceAccessibility bundleIdentifier:@"velikiy"]);
    XCTAssertNil([SPAccessRepairPolicy tccutilArgumentsForService:SPAccessRepairServiceAccessibility
                                                 bundleIdentifier:@"io.github.x All"]);
    XCTAssertNil([SPAccessRepairPolicy tccutilArgumentsForService:SPAccessRepairServiceNone bundleIdentifier:bundle]);
    XCTAssertNil([SPAccessRepairPolicy
        tccutilArgumentsForService:SPAccessRepairServiceAccessibility | SPAccessRepairServiceInputMonitoring
                  bundleIdentifier:bundle]);
}

@end
