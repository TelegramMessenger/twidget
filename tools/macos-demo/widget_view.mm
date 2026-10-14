#import "widget_view.h"

#import <QuartzCore/QuartzCore.h>

#include "mac_renderer.h"
#include "twidget/widget_host.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <map>
#include <optional>

namespace {

int modifiers(NSEventModifierFlags flags) {
    // The core's bits: alt, ctrl, meta, shift.
    return (flags & NSEventModifierFlagOption ? 1 : 0) | (flags & NSEventModifierFlagControl ? 2 : 0) | (flags & NSEventModifierFlagCommand ? 4 : 0) |
           (flags & NSEventModifierFlagShift ? 8 : 0);
}

/// The DOM `key` and `code` of a key event; `code` assumes a US layout.
std::pair<std::string, std::string> dom_key(NSEvent* event) {
    struct Named { unsigned short code; const char* key; const char* dom; };
    static const Named named[] = {
        {36, "Enter", "Enter"}, {76, "Enter", "NumpadEnter"}, {48, "Tab", "Tab"}, {51, "Backspace", "Backspace"}, {53, "Escape", "Escape"},
        {117, "Delete", "Delete"}, {123, "ArrowLeft", "ArrowLeft"}, {124, "ArrowRight", "ArrowRight"}, {125, "ArrowDown", "ArrowDown"},
        {126, "ArrowUp", "ArrowUp"}, {115, "Home", "Home"}, {119, "End", "End"}, {116, "PageUp", "PageUp"}, {121, "PageDown", "PageDown"},
        {49, " ", "Space"},
    };
    for (const Named& n : named)
        if (event.keyCode == n.code) return {n.key, n.dom};
    // kVK_ANSI_*: letters and digits are not in key order on a Mac keyboard.
    static const char* const letters = "asdfhgzxcv?bqweryt123465=97-80]ou[ip?lj'k;\\,/nm.";
    std::string code;
    if (event.keyCode < 48) {
        char c = letters[event.keyCode];
        if (c >= 'a' && c <= 'z') code = std::string("Key") + char(c - 'a' + 'A');
        else if (c >= '0' && c <= '9') code = std::string("Digit") + c;
    }
    NSString* characters = event.characters ?: @"";
    std::string key = characters.UTF8String ?: "";
    if (key.empty() || (unsigned char)key[0] < 0x20 || (unsigned char)key[0] == 0x7f || [characters characterAtIndex:0] >= 0xF700) key = "Unidentified";
    return {key, code};
}

/// A compositor layer as Core Animation shows it: the content under a container that clips it when
/// an ancestor of the element clips.
struct Shown {
    CALayer* content = nil;
    CALayer* container = nil;  // carries the clip's mask; nil when unclipped
    twlite::Rect bounds;
    bool has_clip = false;
    twlite::Rect clip;
    twlite::Radii clip_radii;
    twlite::LayerTrack track;
    CALayer* top() const { return container ?: content; }
};

} // namespace

@implementation TWWidgetView {
    std::unique_ptr<twidget::WidgetHost> _host;
    MacRenderer* _renderer;  // owned by _host
    CALayer* _root;
    CALayer* _stage;  // the layers' parent: document coordinates, y down
    std::map<int64_t, Shown> _layers;
    CGFloat _width;
    CGFloat _contentHeight;
    BOOL _onScreen;
    NSTrackingArea* _tracking;
}

- (instancetype)initWithHTML:(const std::string&)html dark:(BOOL)dark width:(CGFloat)width resources:(std::shared_ptr<twidget::SandboxResources>)resources {
    if ((self = [super initWithFrame:NSMakeRect(0, 0, width, 24)])) {
        _width = width;
        _contentHeight = 24;
        _onScreen = YES;

        // A layer-hosting view: AppKit leaves the tree's geometry to us.
        _root = [CALayer layer];
        _root.contentsGravity = kCAGravityTopLeft;
        _root.geometryFlipped = NO;
        _stage = [CALayer layer];
        _stage.anchorPoint = CGPointZero;
        _stage.position = CGPointZero;
        [_root addSublayer:_stage];
        self.layer = _root;
        self.wantsLayer = YES;

        double dpr = NSScreen.mainScreen.backingScaleFactor ?: 2;
        twidget::WidgetHostOptions options;
        options.devicePixelRatio = dpr;
        options.features = {"webAnimations"};

        __weak TWWidgetView* weakSelf = self;
        // WidgetHost calls the factory once, in its constructor.
        MacRenderer* created = nullptr;
        auto factory = [weakSelf, dpr, &created](std::shared_ptr<twidget::SandboxResources>, const std::string& theme, double w) -> std::unique_ptr<twidget::Renderer> {
            auto renderer = std::make_unique<MacRenderer>(theme, w, dpr, [weakSelf](std::shared_ptr<Frame> frame) {
                dispatch_async(dispatch_get_main_queue(), ^{
                    [weakSelf showFrame:frame];
                });
            });
            created = renderer.get();
            return renderer;
        };
        _host = std::make_unique<twidget::WidgetHost>(html, dark ? "dark" : "light", width, options, std::move(resources), factory);
        _renderer = created;
        _host->onChange = [weakSelf](const twidget::WidgetDiagnostics& d) {
            NSString* status;
            auto ms = [](const std::optional<double>& v) { return v ? [NSString stringWithFormat:@"%.0f ms", *v] : @"–"; };
            if (d.dead) {
                status = [NSString stringWithFormat:@"dead: %s", d.dead->c_str()];
            } else {
                status = [NSString stringWithFormat:@"loaded %@ | content %@ | js %.0f ms | ops %d", ms(d.loadedMs), ms(d.firstContentMs), d.stats.jsMs, int(d.stats.ops)];
                if (d.lastError) status = [status stringByAppendingFormat:@" | error: %s", d.lastError->c_str()];
            }
            CGFloat height = std::max(d.height, 24.0);
            dispatch_async(dispatch_get_main_queue(), ^{
                [weakSelf changed:status height:height];
            });
        };
    }
    return self;
}

- (void)dealloc {
    [self close];
}

- (void)close {
    if (!_host) return;
    // destroy() joins the host, engine and clock threads; frames already queued find the view closed.
    _host->destroy();
    _host.reset();
    _renderer = nullptr;
}

- (void)start {
    if (_host) _host->start();
}

- (void)setDark:(BOOL)dark {
    if (_host) _host->setTheme(dark ? "dark" : "light");
}

- (void)setContentWidth:(CGFloat)width {
    _width = width;
    [self setFrameSize:NSMakeSize(width, _contentHeight)];
    // WidgetHost has no width of its own: the renderer reports the new size to it as it would after a batch.
    MacRenderer* renderer = _renderer;
    if (_host && renderer) _host->onHostThread([renderer, width] { renderer->setWidth(width); });
}

- (void)setOnScreen:(BOOL)visible {
    if (visible == _onScreen || !_host) return;
    _onScreen = visible;
    _host->setVisible(visible);
    if (_renderer) _renderer->setVisible(visible);
}

- (CGFloat)contentHeight {
    return _contentHeight;
}

- (void)changed:(NSString*)status height:(CGFloat)height {
    if (!_host) return;
    if (_renderer) status = [status stringByAppendingFormat:@" | paints %d (%.1f ms) | build %.1f ms | layers %d", _renderer->paints(), _renderer->lastPaintMs(), _renderer->lastBuildMs(), _renderer->layers()];
    _contentHeight = height;
    if (self.onChange) self.onChange(status);
}

- (void)setFrame:(NSRect)frame {
    [super setFrame:frame];
    [self fitLayers:frame.size];
}

- (void)setFrameSize:(NSSize)size {
    [super setFrameSize:size];
    [self fitLayers:size];
}

- (void)fitLayers:(NSSize)size {
    [CATransaction begin];
    [CATransaction setDisableActions:YES];
    _root.frame = NSMakeRect(0, 0, size.width, size.height);
    _stage.bounds = CGRectMake(0, 0, size.width, size.height);
    // Document y runs down from the top of the view.
    _stage.sublayerTransform = CATransform3DMakeAffineTransform(CGAffineTransformMake(1, 0, 0, -1, 0, size.height));
    [CATransaction commit];
}

// ───────────────────────── frames ─────────────────────────

- (void)showFrame:(const std::shared_ptr<Frame>&)frame {
    if (!_host || !frame) return;
    [CATransaction begin];
    [CATransaction setDisableActions:YES];
    if (frame->document) {
        _root.contents = (__bridge id)frame->document.get();
        _root.contentsScale = frame->dpr;
    }
    if (frame->layers) [self showLayers:*frame->layers];
    [CATransaction commit];
}

- (void)showLayers:(const std::vector<LayerFrame>&)layers {
    std::map<int64_t, Shown> next;
    NSMutableArray<CALayer*>* order = [NSMutableArray array];
    for (const LayerFrame& source : layers) {
        Shown shown;
        auto old = _layers.find(source.id);
        bool fresh = old == _layers.end();
        if (!fresh) {
            shown = old->second;
            _layers.erase(old);
        } else {
            shown.content = [CALayer layer];
            shown.content.anchorPoint = CGPointZero;
            shown.content.position = CGPointZero;
        }
        bool moved = fresh || std::memcmp(&shown.bounds, &source.bounds, sizeof(twlite::Rect)) != 0;
        shown.bounds = source.bounds;
        shown.content.bounds = CGRectMake(0, 0, source.bounds.w, source.bounds.h);
        if (source.content) {
            shown.content.contents = (__bridge id)source.content.get();
            shown.content.contentsScale = source.scale;
        }

        bool reclipped = fresh || shown.has_clip != source.has_clip || std::memcmp(&shown.clip, &source.clip, sizeof(twlite::Rect)) != 0 ||
                         std::memcmp(&shown.clip_radii, &source.clip_radii, sizeof(twlite::Radii)) != 0;
        if (reclipped) {
            [shown.content removeFromSuperlayer];
            [shown.container removeFromSuperlayer];
            shown.container = nil;
            shown.has_clip = source.has_clip;
            shown.clip = source.clip;
            shown.clip_radii = source.clip_radii;
            if (source.has_clip) {
                // The container's space is the stage's, document coordinates: so is the mask's path.
                shown.container = [CALayer layer];
                shown.container.anchorPoint = CGPointZero;
                shown.container.position = CGPointZero;
                shown.container.bounds = CGRectMake(0, 0, source.clip.x + source.clip.w, source.clip.y + source.clip.h);
                CAShapeLayer* mask = [CAShapeLayer layer];
                mask.frame = shown.container.bounds;
                CGPathRef path = clip_path(source.clip, source.clip_radii);
                mask.path = path;
                CGPathRelease(path);
                shown.container.mask = mask;
                [shown.container addSublayer:shown.content];
            }
        }
        shown.track = source.track;
        if (source.track_changed || moved || reclipped) [self play:shown];
        [order addObject:shown.top()];
        next.emplace(source.id, shown);
    }
    // Layers the composition no longer has: their elements are back in the document's paint.
    for (auto& [id, gone] : _layers) [gone.top() removeFromSuperlayer];
    _layers = std::move(next);

    // Paint order, bottom first, above the document (the root's contents). A layer that moves among
    // its siblings starts its track again, from where the clock has it.
    NSArray<CALayer*>* current = _stage.sublayers ?: @[];
    if (![current isEqualToArray:order]) {
        for (NSUInteger i = 0; i < order.count; i++) {
            if (i < _stage.sublayers.count && _stage.sublayers[i] == order[i]) continue;
            [_stage insertSublayer:order[i] atIndex:unsigned(i)];
            for (auto& [id, shown] : _layers)
                if (shown.top() == order[i]) [self play:shown];
        }
    }
}

/// Starts a layer's track where the core's clock has it now; Core Animation runs it from there.
- (void)play:(Shown&)shown {
    CALayer* layer = shown.content;
    [layer removeAnimationForKey:@"track"];
    double duration = 0;
    auto keys = twlite::resume_track(shown.track, twlite::Renderer::clock() - shown.track.start, duration);
    if (keys.empty()) return;
    // The model value is where the track ends, so a finished one-shot holds there.
    layer.transform = CATransform3DMakeAffineTransform(layer_transform(keys.back(), shown.bounds));
    layer.opacity = keys.back().opacity;
    if (keys.size() < 2 || duration <= 0) return;

    NSMutableArray* transforms = [NSMutableArray arrayWithCapacity:keys.size()];
    NSMutableArray* opacities = [NSMutableArray arrayWithCapacity:keys.size()];
    NSMutableArray* times = [NSMutableArray arrayWithCapacity:keys.size()];
    for (const twlite::LayerKey& key : keys) {
        [transforms addObject:[NSValue valueWithCATransform3D:CATransform3DMakeAffineTransform(layer_transform(key, shown.bounds))]];
        [opacities addObject:@(key.opacity)];
        [times addObject:@(key.t)];
    }
    // The core baked the track with linear segments, component by component, as CA interpolates.
    CAKeyframeAnimation* transform = [CAKeyframeAnimation animationWithKeyPath:@"transform"];
    transform.values = transforms;
    transform.keyTimes = times;
    transform.duration = duration;
    CAKeyframeAnimation* opacity = [CAKeyframeAnimation animationWithKeyPath:@"opacity"];
    opacity.values = opacities;
    opacity.keyTimes = times;
    opacity.duration = duration;
    CAAnimationGroup* group = [CAAnimationGroup animation];
    group.animations = @[ transform, opacity ];
    group.duration = duration;
    group.repeatCount = shown.track.repeat ? HUGE_VALF : 1;
    group.removedOnCompletion = YES;
    [layer addAnimation:group forKey:@"track"];
}

// ───────────────────────── input ─────────────────────────

- (BOOL)acceptsFirstResponder {
    return YES;
}

- (BOOL)acceptsFirstMouse:(NSEvent*)event {
    return YES;
}

/// The event's position in CSS px: points from the top left of the widget.
- (NSPoint)cssPoint:(NSEvent*)event {
    NSPoint p = [self convertPoint:event.locationInWindow fromView:nil];
    return self.isFlipped ? p : NSMakePoint(p.x, self.bounds.size.height - p.y);
}

- (void)updateTrackingAreas {
    [super updateTrackingAreas];
    if (_tracking) [self removeTrackingArea:_tracking];
    _tracking = [[NSTrackingArea alloc] initWithRect:NSZeroRect
                                             options:NSTrackingMouseMoved | NSTrackingMouseEnteredAndExited | NSTrackingActiveInKeyWindow | NSTrackingInVisibleRect
                                               owner:self
                                            userInfo:nil];
    [self addTrackingArea:_tracking];
}

- (void)mouseDown:(NSEvent*)event {
    [self.window makeFirstResponder:self];
    NSPoint p = [self cssPoint:event];
    if (_renderer) _renderer->touch(0, p.x, p.y, 1, true, true, modifiers(event.modifierFlags));
}

- (void)mouseDragged:(NSEvent*)event {
    NSPoint p = [self cssPoint:event];
    if (_renderer) _renderer->touch(1, p.x, p.y, 1, true, true, modifiers(event.modifierFlags));
}

- (void)mouseUp:(NSEvent*)event {
    NSPoint p = [self cssPoint:event];
    if (_renderer) _renderer->touch(2, p.x, p.y, 1, true, true, modifiers(event.modifierFlags));
}

- (void)mouseMoved:(NSEvent*)event {
    NSPoint p = [self cssPoint:event];
    if (_renderer) _renderer->hover(0, p.x, p.y, modifiers(event.modifierFlags));
}

- (void)mouseExited:(NSEvent*)event {
    if (_renderer) _renderer->hover(1, 0, 0, 0);
}

- (void)scrollWheel:(NSEvent*)event {
    NSPoint p = [self cssPoint:event];
    // DOM deltas grow downwards and rightwards; AppKit's grow the other way. A trackpad gives
    // points, a wheel lines.
    if (_renderer) {
        int mode = event.hasPreciseScrollingDeltas ? 0 : 1;
        _renderer->wheel(p.x, p.y, -event.scrollingDeltaX, -event.scrollingDeltaY, mode, modifiers(event.modifierFlags));
    }
    // The feed scrolls too: a widget cannot take the wheel from the chat.
    [super scrollWheel:event];
}

- (void)keyDown:(NSEvent*)event {
    auto [key, code] = dom_key(event);
    if (_renderer) _renderer->key("keydown", key, code, event.isARepeat, modifiers(event.modifierFlags));
}

- (void)keyUp:(NSEvent*)event {
    auto [key, code] = dom_key(event);
    if (_renderer) _renderer->key("keyup", key, code, false, modifiers(event.modifierFlags));
}

@end
