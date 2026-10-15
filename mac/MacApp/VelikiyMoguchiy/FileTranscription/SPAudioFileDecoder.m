#import "SPAudioFileDecoder.h"
#import <AVFoundation/AVFoundation.h>

static NSString *const kDecoderErrorDomain = @"SPAudioFileDecoder";

static NSError *decoderError(NSString *description) {
    return [NSError errorWithDomain:kDecoderErrorDomain
                               code:1
                           userInfo:@{NSLocalizedDescriptionKey : description}];
}

@implementation SPAudioFileDecoder

+ (NSData *)pcmFromURL:(NSURL *)url isCancelled:(BOOL (^)(void))isCancelled error:(NSError **)error {
    AVURLAsset *asset = [AVURLAsset URLAssetWithURL:url options:nil];
    // The synchronous track list is deprecated but this runs on a background
    // queue, and the async loader would only wrap the same wait.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    NSArray<AVAssetTrack *> *tracks = [asset tracksWithMediaType:AVMediaTypeAudio];
#pragma clang diagnostic pop
    if (tracks.count == 0) {
        if (error) *error = decoderError(@"в файле нет звуковой дорожки, или macOS не умеет его читать");
        return nil;
    }

    NSError *readerError = nil;
    AVAssetReader *reader = [AVAssetReader assetReaderWithAsset:asset error:&readerError];
    if (!reader) {
        if (error) *error = readerError ?: decoderError(@"не удалось открыть файл");
        return nil;
    }
    NSDictionary *settings = @{
        AVFormatIDKey : @(kAudioFormatLinearPCM),
        AVSampleRateKey : @16000,
        AVNumberOfChannelsKey : @1,
        AVLinearPCMBitDepthKey : @16,
        AVLinearPCMIsFloatKey : @NO,
        AVLinearPCMIsBigEndianKey : @NO,
        AVLinearPCMIsNonInterleaved : @NO,
    };
    // The mix output downmixes every audio track (a video's several
    // languages or a call's two sides) into the one mono stream.
    AVAssetReaderAudioMixOutput *output = [AVAssetReaderAudioMixOutput assetReaderAudioMixOutputWithAudioTracks:tracks
                                                                                                audioSettings:settings];
    output.alwaysCopiesSampleData = NO;
    if (![reader canAddOutput:output]) {
        if (error) *error = decoderError(@"macOS не может перевести звук этого файла в нужный формат");
        return nil;
    }
    [reader addOutput:output];
    if (![reader startReading]) {
        if (error) *error = reader.error ?: decoderError(@"не удалось прочитать файл");
        return nil;
    }

    NSMutableData *pcm = [NSMutableData data];
    while (reader.status == AVAssetReaderStatusReading) {
        if (isCancelled && isCancelled()) {
            [reader cancelReading];
            return nil;
        }
        CMSampleBufferRef buffer = [output copyNextSampleBuffer];
        if (!buffer) break;
        CMBlockBufferRef block = CMSampleBufferGetDataBuffer(buffer);
        size_t length = block ? CMBlockBufferGetDataLength(block) : 0;
        if (length > 0) {
            NSUInteger offset = pcm.length;
            pcm.length = offset + length;
            CMBlockBufferCopyDataBytes(block, 0, length, (uint8_t *)pcm.mutableBytes + offset);
        }
        CFRelease(buffer);
    }
    if (reader.status == AVAssetReaderStatusFailed) {
        if (error) *error = reader.error ?: decoderError(@"чтение файла прервалось");
        return nil;
    }
    if (pcm.length < 2) {
        if (error) *error = decoderError(@"в файле нет звука");
        return nil;
    }
    return pcm;
}

@end
