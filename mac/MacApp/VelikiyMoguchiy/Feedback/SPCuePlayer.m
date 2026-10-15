#import "SPCuePlayer.h"
#import <AppKit/AppKit.h>
#import "velikiy_moguchiy_core.h"

@interface SPCuePlayer ()

@property (nonatomic, assign) BOOL startSoundEnabled;
@property (nonatomic, assign) BOOL stopSoundEnabled;
@property (nonatomic, assign) BOOL errorSoundEnabled;
@property (nonatomic, strong) NSMutableDictionary<NSString *, NSSound *> *sounds;

@end

@implementation SPCuePlayer

- (instancetype)init {
    self = [super init];
    if (self) {
        _startSoundEnabled = YES;
        _stopSoundEnabled = YES;
        _errorSoundEnabled = YES;
        _sounds = [NSMutableDictionary dictionary];
    }
    return self;
}

- (void)reloadFeedbackConfig {
    struct SPFeedbackConfig cfg = sp_core_get_feedback_config();
    self.startSoundEnabled = cfg.start_sound;
    // The stop cue follows the start cue's switch; stop_sound alone is an older separate switch.
    self.stopSoundEnabled = cfg.start_sound || cfg.stop_sound;
    self.errorSoundEnabled = cfg.error_sound;
}

- (void)playStart {
    if (self.startSoundEnabled) {
        [self playSystemSound:@"Tink" volume:1.0];
    }
}

- (void)playStop {
    if (self.stopSoundEnabled) {
        [self playSystemSound:@"Pop" volume:0.45];
    }
}

- (void)playError {
    if (self.errorSoundEnabled) {
        [self playSystemSound:@"Basso" volume:1.0];
    }
}

- (void)playSystemSound:(NSString *)name volume:(float)volume {
    NSSound *sound = self.sounds[name];
    if (!sound) {
        // A copy, so the volume set here stays off the shared named instance.
        sound = [[NSSound soundNamed:name] copy];
        if (!sound) return;
        self.sounds[name] = sound;
    }
    sound.volume = volume;
    [sound play];
}

@end
