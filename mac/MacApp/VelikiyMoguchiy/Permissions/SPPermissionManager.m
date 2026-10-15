#import "SPPermissionManager.h"
#import "SPLocalization.h"
#import <AVFoundation/AVFoundation.h>
#import <ApplicationServices/ApplicationServices.h>
#import <Cocoa/Cocoa.h>
#import <Speech/Speech.h>
#import <UserNotifications/UserNotifications.h>

static NSString *const kDontRemindPrefix = @"SPPermissionDontRemind_";
static NSString *const kWasGrantedPrefix = @"SPPermissionWasGranted_";

NSNotificationName const SPPermissionManagerWillResetAccessNotification = @"SPPermissionManagerWillResetAccessNotification";
NSString *const SPPermissionRepairServicesKey = @"services";

static NSString *const SPRepairExplanation =
    @"«Исправить» сбросит только строку «Великого & Могучего» в списке разрешений, больше ничего не удаляется.";

@implementation SPPermissionManager

- (void)checkAllPermissionsWithCompletion:(SPPermissionCheckCompletion)completion {
    [self checkAllPermissionsPromptingAccessibility:YES completion:completion];
}

- (void)checkAllPermissionsPromptingAccessibility:(BOOL)prompt completion:(SPPermissionCheckCompletion)completion {
    // Check microphone permission (async)
    [self requestMicrophonePermissionWithCompletion:^(BOOL micGranted) {
        if (prompt) [self requestAccessibilityPermission];
        BOOL accessibility = [self isAccessibilityGranted];
        BOOL inputMonitoring = [self isInputMonitoringGranted];
        dispatch_async(dispatch_get_main_queue(), ^{
            completion(micGranted, accessibility, inputMonitoring);
        });
    }];
}

- (void)requestMicrophonePermissionWithCompletion:(void (^)(BOOL))completion {
    AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
    if (status == AVAuthorizationStatusAuthorized) {
        completion(YES);
    } else if (status == AVAuthorizationStatusNotDetermined) {
        [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) {
            completion(granted);
        }];
    } else {
        NSLog(@"[VelikiyMoguchiy] Microphone permission denied or restricted");
        completion(NO);
    }
}

- (BOOL)isMicrophoneGranted {
    return [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio] == AVAuthorizationStatusAuthorized;
}

- (BOOL)isMicrophoneUndetermined {
    return [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio] == AVAuthorizationStatusNotDetermined;
}

- (BOOL)isCameraGranted {
    return [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeVideo] == AVAuthorizationStatusAuthorized;
}

- (BOOL)isAccessibilityGranted {
    NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @NO};
    return AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
}

- (void)requestAccessibilityPermission {
    NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES};
    AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
}

static CGEventRef inputMonitoringProbeCallback(CGEventTapProxy proxy,
                                                CGEventType type,
                                                CGEventRef event,
                                                void *userInfo) {
    return event;
}

- (BOOL)isInputMonitoringGranted {
    // Probe by attempting to create a CGEventTap.
    // Must provide a valid callback — NULL callback can return NULL even with permission.
    CGEventMask mask = CGEventMaskBit(kCGEventFlagsChanged);
    CFMachPortRef tap = CGEventTapCreate(kCGHIDEventTap,
                                         kCGHeadInsertEventTap,
                                         kCGEventTapOptionListenOnly,
                                         mask,
                                         inputMonitoringProbeCallback,
                                         NULL);
    if (tap) {
        // CFRelease alone does NOT destroy the WindowServer side of the tap:
        // it stays alive and ENABLED with nobody draining its queue, one more
        // orphan per probe (startup + every status-bar menu open). Those
        // orphans buffer a copy of every FlagsChanged in the session, and
        // when the process finally dies WindowServer's cleanup of the wedged
        // taps emits phantom modifier events that other apps' hotkey
        // detectors treat as real presses (issues #57/#65 — THE phantom-keys-
        // at-quit bug). The port must be explicitly invalidated.
        CGEventTapEnable(tap, false);
        CFMachPortInvalidate(tap);
        CFRelease(tap);
        return YES;
    }
    return NO;
}

- (BOOL)isSpeechRecognitionGranted {
    return [SFSpeechRecognizer authorizationStatus] == SFSpeechRecognizerAuthorizationStatusAuthorized;
}

- (void)requestSpeechRecognitionPermissionWithCompletion:(void (^)(BOOL))completion {
    SFSpeechRecognizerAuthorizationStatus status = [SFSpeechRecognizer authorizationStatus];
    if (status == SFSpeechRecognizerAuthorizationStatusAuthorized) {
        completion(YES);
    } else if (status == SFSpeechRecognizerAuthorizationStatusNotDetermined) {
        [SFSpeechRecognizer requestAuthorization:^(SFSpeechRecognizerAuthorizationStatus newStatus) {
            dispatch_async(dispatch_get_main_queue(), ^{
                completion(newStatus == SFSpeechRecognizerAuthorizationStatusAuthorized);
            });
        }];
    } else {
        NSLog(@"[VelikiyMoguchiy] Speech recognition permission denied or restricted");
        completion(NO);
    }
}

- (void)requestNotificationPermission {
    UNUserNotificationCenter *center = [UNUserNotificationCenter currentNotificationCenter];
    [center requestAuthorizationWithOptions:(UNAuthorizationOptionAlert | UNAuthorizationOptionSound)
                          completionHandler:^(BOOL granted, NSError * _Nullable error) {
        if (error) {
            NSLog(@"[VelikiyMoguchiy] Notification permission request error: %@", error.localizedDescription);
        } else {
            NSLog(@"[VelikiyMoguchiy] Notification permission %@", granted ? @"granted" : @"denied");
        }
    }];
}

- (void)checkNotificationPermissionWithCompletion:(void (^)(BOOL granted))completion {
    UNUserNotificationCenter *center = [UNUserNotificationCenter currentNotificationCenter];
    [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings * _Nonnull settings) {
        BOOL granted = (settings.authorizationStatus == UNAuthorizationStatusAuthorized);
        dispatch_async(dispatch_get_main_queue(), ^{
            completion(granted);
        });
    }];
}

#pragma mark - Permission Alerts

- (NSString *)dontRemindKeyForType:(SPPermissionType)type {
    return [NSString stringWithFormat:@"%@%ld", kDontRemindPrefix, (long)type];
}

- (BOOL)isDontRemindSetForType:(SPPermissionType)type {
    return [[NSUserDefaults standardUserDefaults] boolForKey:[self dontRemindKeyForType:type]];
}

- (void)setDontRemindForType:(SPPermissionType)type {
    [[NSUserDefaults standardUserDefaults] setBool:YES forKey:[self dontRemindKeyForType:type]];
}

- (void)resetDontRemindForType:(SPPermissionType)type {
    [[NSUserDefaults standardUserDefaults] removeObjectForKey:[self dontRemindKeyForType:type]];
}

- (BOOL)showPermissionAlertForType:(SPPermissionType)type
                       settingsURL:(nullable NSURL *)settingsURL {
    if ([self isDontRemindSetForType:type]) {
        return NO;
    }

    NSString *title = nil;
    NSString *message = nil;

    switch (type) {
        case SPPermissionTypeMicrophone:
            title = SPLocalizedString(@"permission.microphone.title");
            message = SPLocalizedString(@"permission.microphone.message");
            break;
        case SPPermissionTypeAccessibility:
            title = SPLocalizedString(@"permission.accessibility.title");
            message = SPLocalizedString(@"permission.accessibility.message");
            break;
        case SPPermissionTypeInputMonitoring:
            title = SPLocalizedString(@"permission.inputMonitoring.title");
            message = SPLocalizedString(@"permission.inputMonitoring.message");
            break;
        case SPPermissionTypeSpeechRecognition:
            title = SPLocalizedString(@"permission.speechRecognition.title");
            message = SPLocalizedString(@"permission.speechRecognition.message");
            break;
    }

    [NSApp activateIgnoringOtherApps:YES];

    NSAlert *alert = [[NSAlert alloc] init];
    alert.alertStyle = NSAlertStyleWarning;
    alert.messageText = title;
    alert.informativeText = message;

    if (settingsURL) {
        [alert addButtonWithTitle:SPLocalizedString(@"permission.button.openSettings")];
    }
    [alert addButtonWithTitle:SPLocalizedString(@"permission.button.dismiss")];
    [alert addButtonWithTitle:SPLocalizedString(@"permission.button.dontRemind")];

    NSModalResponse response = [alert runModal];

    if (settingsURL && response == NSAlertFirstButtonReturn) {
        [[NSWorkspace sharedWorkspace] openURL:settingsURL];
    }

    // "Don't Remind Again" is the last button
    NSModalResponse dontRemindResponse = settingsURL ? NSAlertThirdButtonReturn : NSAlertSecondButtonReturn;
    if (response == dontRemindResponse) {
        [self setDontRemindForType:type];
    }

    return YES;
}

#pragma mark - Grants lost after an update

+ (NSURL *)settingsURLForType:(SPPermissionType)type {
    NSString *anchor = @"Privacy_Microphone";
    switch (type) {
        case SPPermissionTypeMicrophone: anchor = @"Privacy_Microphone"; break;
        case SPPermissionTypeAccessibility: anchor = @"Privacy_Accessibility"; break;
        case SPPermissionTypeInputMonitoring: anchor = @"Privacy_ListenEvent"; break;
        case SPPermissionTypeSpeechRecognition: anchor = @"Privacy_SpeechRecognition"; break;
    }
    return [NSURL URLWithString:[@"x-apple.systempreferences:com.apple.preference.security?"
                                    stringByAppendingString:anchor]];
}

- (NSString *)wasGrantedKeyForType:(SPPermissionType)type {
    return [NSString stringWithFormat:@"%@%ld", kWasGrantedPrefix, (long)type];
}

- (void)rememberGrantedPermissions {
    NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
    if ([self isMicrophoneGranted]) [defaults setBool:YES forKey:[self wasGrantedKeyForType:SPPermissionTypeMicrophone]];
    if ([self isAccessibilityGranted]) [defaults setBool:YES forKey:[self wasGrantedKeyForType:SPPermissionTypeAccessibility]];
    if ([self isInputMonitoringGranted]) {
        [defaults setBool:YES forKey:[self wasGrantedKeyForType:SPPermissionTypeInputMonitoring]];
    }
}

- (NSArray<NSNumber *> *)lostPermissionTypes {
    NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
    NSMutableArray<NSNumber *> *lost = [NSMutableArray array];
    if ([defaults boolForKey:[self wasGrantedKeyForType:SPPermissionTypeAccessibility]] &&
        ![self isAccessibilityGranted]) {
        [lost addObject:@(SPPermissionTypeAccessibility)];
    }
    if ([defaults boolForKey:[self wasGrantedKeyForType:SPPermissionTypeInputMonitoring]] &&
        ![self isInputMonitoringGranted]) {
        [lost addObject:@(SPPermissionTypeInputMonitoring)];
    }
    AVAuthorizationStatus mic = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
    if ([defaults boolForKey:[self wasGrantedKeyForType:SPPermissionTypeMicrophone]] &&
        (mic == AVAuthorizationStatusDenied || mic == AVAuthorizationStatusRestricted)) {
        [lost addObject:@(SPPermissionTypeMicrophone)];
    }
    return lost;
}

static NSString *SPPermissionTitle(SPPermissionType type) {
    switch (type) {
        case SPPermissionTypeMicrophone: return @"«Микрофон»";
        case SPPermissionTypeAccessibility: return @"«Универсальный доступ»";
        case SPPermissionTypeInputMonitoring: return @"«Мониторинг ввода»";
        case SPPermissionTypeSpeechRecognition: return @"«Распознавание речи»";
    }
    return @"";
}

- (void)showLostAccessAlertForTypes:(NSArray<NSNumber *> *)types {
    if (types.count == 0) return;
    NSMutableArray<NSString *> *names = [NSMutableArray array];
    for (NSNumber *type in types) [names addObject:SPPermissionTitle((SPPermissionType)type.integerValue)];
    SPPermissionType first = (SPPermissionType)types.firstObject.integerValue;
    NSString *list = [names componentsJoinedByString:@" и "];
    SPAccessRepairServices repairable = SPAccessRepairServiceNone;
    if ([types containsObject:@(SPPermissionTypeAccessibility)]) repairable |= SPAccessRepairServiceAccessibility;
    if ([types containsObject:@(SPPermissionTypeInputMonitoring)]) repairable |= SPAccessRepairServiceInputMonitoring;

    [NSApp activateIgnoringOtherApps:YES];
    NSAlert *alert = [[NSAlert alloc] init];
    alert.alertStyle = NSAlertStyleWarning;
    alert.messageText = @"macOS сбросила доступ после обновления";
    NSMutableString *text = [NSMutableString stringWithFormat:
        @"Раньше у «Великого & Могучего» %@ %@, а у этой версии нет. Так бывает после обновления: "
        @"галочка в Системных настройках может стоять, но относится к прежней версии.",
        types.count > 1 ? @"были разрешения" : @"было разрешение", list];
    if (repairable) {
        [text appendFormat:@"\n\n%@ Затем включите переключатель заново.", SPRepairExplanation];
    }
    if ([types containsObject:@(SPPermissionTypeMicrophone)]) {
        [text appendString:repairable ? @" В разделе «Микрофон» включите переключатель «Великий & Могучий»."
                                      : @"\n\nВ разделе «Микрофон» включите переключатель «Великий & Могучий»."];
    }
    [text appendString:@" Приложение подхватит доступ само; если клавиша не заработает через несколько секунд, "
                       @"перезапустите его."];
    alert.informativeText = text;
    [alert addButtonWithTitle:repairable ? @"Исправить" : SPLocalizedString(@"permission.button.openSettings")];
    [alert addButtonWithTitle:@"Позже"];
    if ([alert runModal] != NSAlertFirstButtonReturn) return;
    if (repairable) {
        [self repairAccessForServices:repairable];
    } else {
        [[NSWorkspace sharedWorkspace] openURL:[SPPermissionManager settingsURLForType:first]];
    }
}

#pragma mark - Stale Accessibility / Input Monitoring grants

- (SPAccessRepairServices)brokenAccessServices {
    return [SPAccessRepairPolicy brokenServicesWithTrust:self];
}

- (void)offerAccessRepairForServices:(SPAccessRepairServices)services {
    if (services == SPAccessRepairServiceNone) return;
    BOOL accessibility = (services & SPAccessRepairServiceAccessibility) != 0;
    [NSApp activateIgnoringOtherApps:YES];
    NSAlert *alert = [[NSAlert alloc] init];
    alert.alertStyle = NSAlertStyleWarning;
    alert.messageText = accessibility ? @"Доступ к управлению не работает" : @"Мониторинг ввода не работает";
    alert.informativeText = [NSString stringWithFormat:
        @"Переключатель у «Великого & Могучего» может стоять, но macOS относит его к прежней версии приложения: "
        @"доступа нет, а включить заново не выходит.\n\n%@ Затем включите переключатель снова, "
        @"перезапускать приложение не нужно.", SPRepairExplanation];
    [alert addButtonWithTitle:@"Исправить"];
    [alert addButtonWithTitle:@"Отмена"];
    if ([alert runModal] == NSAlertFirstButtonReturn) {
        [self repairAccessForServices:services];
    }
}

static BOOL SPResetAccess(SPAccessRepairServices service, NSString *bundleIdentifier) {
    NSArray<NSString *> *arguments = [SPAccessRepairPolicy tccutilArgumentsForService:service
                                                                     bundleIdentifier:bundleIdentifier];
    if (!arguments) {
        NSLog(@"[VelikiyMoguchiy] Access repair: no reset for bundle id %@", bundleIdentifier);
        return NO;
    }
    NSTask *task = [[NSTask alloc] init];
    task.executableURL = [NSURL fileURLWithPath:@"/usr/bin/tccutil"];
    task.arguments = arguments;
    task.standardOutput = [NSFileHandle fileHandleWithNullDevice];
    task.standardError = [NSFileHandle fileHandleWithNullDevice];
    NSError *error = nil;
    if (![task launchAndReturnError:&error]) {
        NSLog(@"[VelikiyMoguchiy] Access repair: tccutil did not start: %@", error.localizedDescription);
        return NO;
    }
    [task waitUntilExit];
    NSLog(@"[VelikiyMoguchiy] Access repair: tccutil %@ -> %d",
          [arguments componentsJoinedByString:@" "], task.terminationStatus);
    return task.terminationStatus == 0;
}

/// Accessibility goes first: its pane is the one opened when both were reset.
- (void)repairAccessForServices:(SPAccessRepairServices)services {
    NSString *bundleIdentifier = NSBundle.mainBundle.bundleIdentifier;
    [[NSNotificationCenter defaultCenter] postNotificationName:SPPermissionManagerWillResetAccessNotification
                                                        object:self
                                                      userInfo:@{SPPermissionRepairServicesKey : @(services)}];
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        SPAccessRepairServices reset = SPAccessRepairServiceNone;
        for (NSNumber *each in @[ @(SPAccessRepairServiceAccessibility), @(SPAccessRepairServiceInputMonitoring) ]) {
            SPAccessRepairServices service = each.unsignedIntegerValue;
            if ((services & service) && SPResetAccess(service, bundleIdentifier)) reset |= service;
        }
        dispatch_async(dispatch_get_main_queue(), ^{
            BOOL accessibility = (services & SPAccessRepairServiceAccessibility) != 0;
            // Both prompts put the app back into their list, switched off.
            if (accessibility) [self requestAccessibilityPermission];
            if (services & SPAccessRepairServiceInputMonitoring) CGRequestListenEventAccess();
            SPPermissionType pane = accessibility ? SPPermissionTypeAccessibility : SPPermissionTypeInputMonitoring;
            [[NSWorkspace sharedWorkspace] openURL:[SPPermissionManager settingsURLForType:pane]];
            if (reset != services) [self showRepairFailedAlert];
        });
    });
}

- (void)showRepairFailedAlert {
    NSAlert *alert = [[NSAlert alloc] init];
    alert.alertStyle = NSAlertStyleWarning;
    alert.messageText = @"Не получилось сбросить разрешение";
    alert.informativeText = @"Сделайте это вручную: в открывшемся списке выделите «Великий & Могучий», удалите его "
                            @"кнопкой «−» и добавьте заново кнопкой «+» из папки «Программы».";
    [alert addButtonWithTitle:@"OK"];
    [alert runModal];
}

@end
