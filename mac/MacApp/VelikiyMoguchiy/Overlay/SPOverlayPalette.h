#import <Cocoa/Cocoa.h>

// An ink plate edged in ochre, with parchment-coloured marks: the overlay is
// the same old book page as the Settings window, in its dark appearance.
static inline NSColor *SPOverlaySurfaceTintColor(void) {
    return [NSColor colorWithSRGBRed:0x0E / 255.0 green:0x0A / 255.0 blue:0x08 / 255.0 alpha:0.96];
}

static inline NSColor *SPOverlayInkColor(void) {
    return [NSColor colorWithSRGBRed:0xE8 / 255.0 green:0xDC / 255.0 blue:0xC8 / 255.0 alpha:1.0];
}

static inline NSColor *SPOverlayOchreColor(void) {
    return [NSColor colorWithSRGBRed:0xD9 / 255.0 green:0xB8 / 255.0 blue:0x65 / 255.0 alpha:1.0];
}

static inline NSColor *SPOverlayCinnabarColor(void) {
    return [NSColor colorWithSRGBRed:0xD9 / 255.0 green:0x82 / 255.0 blue:0x6C / 255.0 alpha:1.0];
}

static inline NSColor *SPOverlayShadowColor(void) {
    return [NSColor colorWithSRGBRed:0.04 green:0.035 blue:0.015 alpha:1.0];
}

/// The plate of the caption and the card: the pill's tint inside an ochre rule.
static inline void SPOverlayDrawPlate(NSRect bounds, CGFloat radius) {
    [SPOverlaySurfaceTintColor() setFill];
    [[NSBezierPath bezierPathWithRoundedRect:bounds xRadius:radius yRadius:radius] fill];
    NSBezierPath *rule = [NSBezierPath bezierPathWithRoundedRect:NSInsetRect(bounds, 1.0, 1.0)
                                                         xRadius:MAX(radius - 1.0, 0)
                                                         yRadius:MAX(radius - 1.0, 0)];
    rule.lineWidth = 1.0;
    [[SPOverlayOchreColor() colorWithAlphaComponent:0.9] setStroke];
    [rule stroke];
}
