#import "SPCorrectionWatcher.h"
#import "SPCorrectionTracker.h"
#import "SPSelectionEditor.h"
#import "velikiy_moguchiy_core.h"

static const NSTimeInterval kReadInterval = 1.0;
/// Edits started later than this after the paste are not followed.
static const NSTimeInterval kWatchDuration = 35.0;
static const NSUInteger kMaxFieldChars = 20000;

@interface SPCorrectionWatcher ()
@property (nonatomic, strong) dispatch_queue_t queue;
@property (nonatomic, strong, nullable) dispatch_source_t timer;
/// Touched only on `queue`.
@property (nonatomic, strong, nullable) SPInsertionRecord *record;
@property (nonatomic, strong, nullable) SPCorrectionTracker *tracker;
@end

@implementation SPCorrectionWatcher

- (instancetype)init {
    self = [super init];
    if (self) {
        _queue = dispatch_queue_create("velikiy-moguchiy.correction-watch", DISPATCH_QUEUE_SERIAL);
    }
    return self;
}

- (void)dealloc {
    if (_timer) dispatch_source_cancel(_timer);
}

- (void)watchInsertion:(SPInsertionRecord *)record {
    [self stop];
    if (record.text.length == 0) return;
    dispatch_source_t timer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, self.queue);
    dispatch_source_set_timer(timer, dispatch_time(DISPATCH_TIME_NOW, (int64_t)(kReadInterval * NSEC_PER_SEC)),
                              (uint64_t)(kReadInterval * NSEC_PER_SEC), (uint64_t)(0.2 * NSEC_PER_SEC));
    __weak typeof(self) weakSelf = self;
    dispatch_async(self.queue, ^{
        weakSelf.record = record;
        weakSelf.tracker = [[SPCorrectionTracker alloc] initWithInsertedText:record.text];
    });
    dispatch_source_set_event_handler(timer, ^{
        [weakSelf readField];
    });
    self.timer = timer;
    dispatch_resume(timer);
}

- (void)stop {
    if (self.timer) {
        dispatch_source_cancel(self.timer);
        self.timer = nil;
    }
    dispatch_async(self.queue, ^{
        self.record = nil;
        self.tracker = nil;
    });
}

/// Runs on `queue`.
- (void)readField {
    SPInsertionRecord *record = self.record;
    SPCorrectionTracker *tracker = self.tracker;
    if (!record || !tracker) return;
    if (-[record.insertedAt timeIntervalSinceNow] > kWatchDuration || tracker.gaveUp) {
        dispatch_async(dispatch_get_main_queue(), ^{
            [self stop];
        });
        return;
    }

    NSString *settled = [tracker settledTextAfterReading:[record fieldTextWithMaxLength:kMaxFieldChars]];
    if (!settled) return;
    char *json = sp_dictionary_detect_correction(tracker.baseline.UTF8String, settled.UTF8String,
                                                 record.text.UTF8String);
    if (!json) {
        [tracker noCorrectionIn:settled];
        return;
    }
    NSData *data = [NSData dataWithBytes:json length:strlen(json)];
    sp_core_free_string(json);
    NSDictionary *found = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    NSString *term = [found isKindOfClass:[NSDictionary class]] ? found[@"term"] : nil;
    NSString *heard = [found isKindOfClass:[NSDictionary class]] ? found[@"heard"] : nil;
    if (![term isKindOfClass:[NSString class]] || term.length == 0) return;
    if (![heard isKindOfClass:[NSString class]]) heard = @"";
    dispatch_async(dispatch_get_main_queue(), ^{
        if (self.onCorrection) self.onCorrection(term, heard);
    });
}

@end
