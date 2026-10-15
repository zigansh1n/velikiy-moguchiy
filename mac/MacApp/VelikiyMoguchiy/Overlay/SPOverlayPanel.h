#import <Cocoa/Cocoa.h>

@protocol SPOverlayPanelDelegate <NSObject>
@optional
/// Called when user selects a prompt template (by click or keyboard shortcut).
/// templateIndex is 0-based index into the prompt_templates array.
- (void)overlayPanel:(id)panel didSelectTemplateAtIndex:(NSInteger)templateIndex;
/// Called when the overlay dismisses itself (e.g. linger timer expired).
- (void)overlayPanelDidDismiss:(id)panel;
/// Called when the user clicks the pill to use raw ASR text.
- (void)overlayPanelDidRequestRawAsrFallback:(id)panel;
/// The card is on screen until overlayPanelDidCloseCard:.
- (void)overlayPanelDidShowCard:(id)panel;
- (void)overlayPanelDidCloseCard:(id)panel;
/// «Копировать» on the card.
- (void)overlayPanel:(id)panel didCopyCardText:(NSString *)text;
@end

/// The icon in front of a message on the pill.
typedef NS_ENUM(NSInteger, SPOverlayMessageIcon) {
    SPOverlayMessageIconError,
    SPOverlayMessageIconClipboard,
    SPOverlayMessageIconSuccess,
    SPOverlayMessageIconNoSpeech,
    SPOverlayMessageIconNotice,
};

/// Small status pill displayed at bottom-center of the display the user works
/// on (SPWorkScreen, taken each time the pill appears), above the Dock:
/// mic bars while recording, pulsing lozenges while processing, then a checkmark
/// (delivered), a clipboard (copied only), an xmark (cancelled) or a red xmark
/// (error) before it hides. The pill never widens: a short line ("Скопировано",
/// an error, "nothing heard") goes into a caption under it, or over it when the
/// pill sits too low, and a text to read goes into a card above it.
@interface SPOverlayPanel : NSObject

@property (nonatomic, weak) id<SPOverlayPanelDelegate> delegate;
/// The screen the pill always sits on; nil follows the work screen.
@property (nonatomic, strong) NSScreen *screenOverride;

- (instancetype)init;

/// Update displayed state.
- (void)updateState:(NSString *)state;

/// No-op: the pill shows no text.
- (void)updateInterimText:(NSString *)text;

/// No-op: the pill shows no text.
- (void)updateDisplayText:(NSString *)text;

/// Enable clicking the pill to accept the raw ASR result.
- (void)setRawAsrFallbackClickEnabled:(BOOL)enabled;

/// A result icon with `badgeText` in the caption, without its leading mark:
/// the clipboard when the text is on the clipboard only ("✓ Скопировано",
/// "… в буфере"), a checkmark otherwise ("✓ Заменено"). Call after updateState:.
- (void)showResultBadge:(NSString *)badgeText;

/// `icon` on the pill and `text` in the caption, hidden once the text had time
/// to be read. A session going idle meanwhile does not cut it short.
- (void)showMessage:(NSString *)text icon:(SPOverlayMessageIcon)icon;

/// A card above the pill with `text` under `title`, scrolling when long, with
/// «Копировать» when `copyable`. It outlives the pill: it closes on a click,
/// dismissCard, a new recording or after 20 s.
- (void)showCardWithTitle:(NSString *)title text:(NSString *)text copyable:(BOOL)copyable;
- (void)dismissCard;
@property (nonatomic, readonly, getter=isShowingCard) BOOL showingCard;

/// While recording: a caption for the level bars ("Микрофон молчит"), nil to drop it.
- (void)setRecordingHint:(NSString *)hint;

/// While processing: a caption for the pulsing lozenges ("Распознаю…"), nil to drop it.
- (void)setProcessingHint:(NSString *)hint;

/// Shake the pill sideways: a key press that starts nothing.
- (void)shake;

/// Whether a message from showMessage:icon: is on screen.
@property (nonatomic, readonly, getter=isShowingMessage) BOOL showingMessage;

/// Flash one letter (the new text mode: R, L, S, F) in the pill for a moment.
/// Same pill size as every other state; call only while no dictation is running.
- (void)showModeIndicator:(NSString *)letter;

/// Dismiss the overlay after the linger period of the icon on screen.
- (void)lingerAndDismiss;
/// Same as lingerAndDismiss; the duration is ignored, the icon decides.
- (void)lingerAndDismissWithDuration:(NSTimeInterval)duration;

/// Dismiss the overlay immediately (with exit animation, no linger delay).
- (void)dismissToIdle;

/// Reload the bottom position from config.yaml.
- (void)reloadAppearanceFromConfig;

/// Show a temporary on-screen preview of the pill at the unsaved bottom margin.
/// Text and typography arguments are ignored.
- (void)showPreviewWithText:(NSString *)text
                   fontSize:(CGFloat)fontSize
                 fontFamily:(NSString *)fontFamily
               bottomMargin:(CGFloat)bottomMargin
          limitVisibleLines:(BOOL)limitVisibleLines
            maxVisibleLines:(NSInteger)maxVisibleLines;

/// Hide the temporary on-screen preview and restore configured appearance.
- (void)hidePreview;

/// Template buttons are gone; these only dismiss the pill.
- (void)showTemplateButtons:(NSArray<NSDictionary *> *)templates;
- (void)showTemplateButtons:(NSArray<NSDictionary *> *)templates lingerDuration:(NSTimeInterval)lingerDuration;

/// No-op.
- (void)hideTemplateButtons;

/// Always NO: there are no templates to trigger.
- (BOOL)handleNumberKey:(NSInteger)number;

@end
