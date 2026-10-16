#import <XCTest/XCTest.h>
#import <objc/runtime.h>
#import "SPSettingsViews.h"

@interface SPSettingsViewsTests : XCTestCase
@end

static NSButton *SPPushButton(NSString *title) {
    NSButton *button = [NSButton buttonWithTitle:title target:nil action:nil];
    button.bezelStyle = NSBezelStyleRounded;
    return button;
}

static NSString *SPRealClassName(id object) {
    return NSStringFromClass(object_getClass(object));
}

@implementation SPSettingsViewsTests

// The About pane: a row of push buttons in an NSStackView, restyled, then torn
// down when another pane is chosen. The stack used to raise in -dealloc.
- (void)testHouseStyleInsideAStackSurvivesTeardown {
    __weak NSStackView *stack = nil;
    XCTAssertNoThrow(stack = [self restyleStackedPaneAndTearItDown]);
    XCTAssertNil(stack);
}

- (NSStackView *)restyleStackedPaneAndTearItDown {
    __weak NSStackView *weakStack = nil;
    @autoreleasepool {
        NSButton *github = SPPushButton(@"Репозиторий на GitHub");
        NSPopUpButton *popup = [[NSPopUpButton alloc] initWithFrame:NSZeroRect pullsDown:NO];
        NSButton *licenses = SPPushButton(@"Лицензии");
        NSStackView *stack = [NSStackView stackViewWithViews:@[ github, popup, licenses ]];
        [stack setCustomSpacing:20 afterView:popup];
        NSButton *save = SPPushButton(@"Сохранить");
        NSView *pane = [NSView new];
        [pane addSubview:stack];
        [pane addSubview:save];

        SPApplyHouseStyle(pane);

        XCTAssertEqualObjects(SPRealClassName(github), @"SPHouseButton");
        XCTAssertEqualObjects(SPRealClassName(licenses), @"SPHouseButton");
        XCTAssertEqualObjects(SPRealClassName(popup), @"SPHousePopUpButton");
        XCTAssertEqualObjects(SPRealClassName(save), @"SPHouseButton");
        XCTAssertEqualObjects(stack.arrangedSubviews, (@[ github, popup, licenses ]));
        XCTAssertEqual([stack customSpacingAfterView:popup], 20);
        weakStack = stack;
    }
    return weakStack;
}

- (void)testAControlObservedElsewhereKeepsItsClass {
    NSButton *button = SPPushButton(@"Сохранить");
    NSView *pane = [NSView new];
    [pane addSubview:button];
    [button addObserver:self forKeyPath:@"hidden" options:0 context:NULL];

    SPApplyHouseStyle(pane);

    XCTAssertNotEqualObjects(SPRealClassName(button), @"SPHouseButton");
    XCTAssertTrue(button.isBordered);
    XCTAssertNoThrow([button removeObserver:self forKeyPath:@"hidden"]);
}

@end
