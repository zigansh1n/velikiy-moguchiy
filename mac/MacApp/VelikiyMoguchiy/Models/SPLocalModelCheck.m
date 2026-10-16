#import "SPLocalModelCheck.h"
#import "SPHfModelPresenter.h"
#import "velikiy_moguchiy_core.h"
#import <AppKit/AppKit.h>
#import <signal.h>
#import <unistd.h>

static NSString *const kCheckChildFlag = @"--velikiy-moguchiy-check-model";
/// Data asset with the clip: «и давно определил про себя», 16 kHz mono.
static NSString *const kCheckClipAsset = @"CheckClip";
/// Loading the largest Whisper exports takes tens of seconds; the core gives
/// up after three minutes, the parent a little later.
static const unsigned kChildLifetimeSeconds = 240;
static const NSTimeInterval kParentDeadlineSeconds = 230;

@implementation SPLocalModelCheck

+ (int)runChildIfRequestedWithArgc:(int)argc argv:(const char *[])argv {
    if (argc != 3 || strcmp(argv[1], kCheckChildFlag.UTF8String) != 0) return -1;
    @autoreleasepool {
        signal(SIGALRM, SIG_DFL);
        alarm(kChildLifetimeSeconds);
        NSData *clip = [[NSDataAsset alloc] initWithName:kCheckClipAsset].data;
        char *json = sp_local_model_check_json(argv[2], clip.bytes, clip.length);
        if (json) {
            printf("%s%s\n", SPLocalModelCheckResultPrefix.UTF8String, json);
            sp_core_free_string(json);
        }
        fflush(stdout);
    }
    return 0;
}

+ (void)checkModelAtPath:(NSString *)modelPath completion:(void (^)(NSDictionary *))completion {
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSDictionary *result = [self resultOfChildForModelAtPath:modelPath];
        dispatch_async(dispatch_get_main_queue(), ^{
            completion(result);
        });
    });
}

+ (NSDictionary *)resultOfChildForModelAtPath:(NSString *)modelPath {
    NSString *executable = [NSBundle mainBundle].executablePath;
    NSTask *task = [[NSTask alloc] init];
    task.executableURL = [NSURL fileURLWithPath:executable ?: @""];
    task.arguments = @[ kCheckChildFlag, modelPath ];
    NSPipe *pipe = [NSPipe pipe];
    task.standardOutput = pipe;
    task.standardInput = [NSFileHandle fileHandleWithNullDevice];
    task.standardError = [NSFileHandle fileHandleWithNullDevice];

    dispatch_semaphore_t exited = dispatch_semaphore_create(0);
    task.terminationHandler = ^(NSTask *ended) {
        dispatch_semaphore_signal(exited);
    };
    NSError *launchError = nil;
    if (executable.length == 0 || ![task launchAndReturnError:&launchError]) {
        NSString *reason = launchError.localizedDescription ?: @"нет исполняемого файла";
        return @{ @"ok" : @NO, @"text" : @"", @"error" : [@"проверка не запустилась: " stringByAppendingString:reason] };
    }
    [pipe.fileHandleForWriting closeFile];

    __block NSData *output = nil;
    dispatch_semaphore_t drained = dispatch_semaphore_create(0);
    NSFileHandle *reader = pipe.fileHandleForReading;
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
        output = [reader readDataToEndOfFile];
        dispatch_semaphore_signal(drained);
    });
    if (dispatch_semaphore_wait(drained, dispatch_time(DISPATCH_TIME_NOW,
                                                       (int64_t)(kParentDeadlineSeconds * NSEC_PER_SEC)))) {
        // SIGKILL closes the child's end of the pipe, which ends the read.
        if (task.isRunning) kill(task.processIdentifier, SIGKILL);
        return @{ @"ok" : @NO, @"text" : @"", @"error" : @"модель не ответила за четыре минуты" };
    }
    if (dispatch_semaphore_wait(exited, dispatch_time(DISPATCH_TIME_NOW, (int64_t)(5 * NSEC_PER_SEC)))) {
        if (task.isRunning) kill(task.processIdentifier, SIGKILL);
        dispatch_semaphore_wait(exited, DISPATCH_TIME_FOREVER);
    }
    return [SPHfModelPresenter checkResultFromOutput:output
                                              status:task.terminationStatus
                                      exitedNormally:task.terminationReason == NSTaskTerminationReasonExit];
}

@end
