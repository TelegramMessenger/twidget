#import "LHDocument.h"
#import "LHGPUInternal.h"
#import "LHDocumentPainting.h"

#include "LHContainer.h"
#include "render_item.h"
#include "html_tag.h"
#include "el_space.h"
#include "el_svg.h"
#include "el_image.h"
#include <algorithm>
#include <cctype>
#include <chrono>
#include <cstring>
#include <cmath>
#include <functional>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <memory>
#include <mutex>

#include <set>

namespace lh = litehtml;

static double NowMs()
{
    using namespace std::chrono;
    return duration<double, std::milli>(steady_clock::now().time_since_epoch()).count();
}

static const std::string &MasterCSS()
{
    static const std::string css = [] {
        std::string s     = lh::master_css;
        size_t      start = s.find("a:link");
        size_t      end   = start == std::string::npos ? start : s.find('}', start);
        if(end != std::string::npos)
            s.replace(start, end - start + 1, "a:link { cursor: pointer; }");
        s += R"CSS(
[hidden], template, datalist { display: none; }
details, summary { display: block; }
summary::before { content: "▶︎"; display: inline-block; width: 1.25em; font-size: 0.9em; }
details[open] > summary::before { content: "▼︎"; }
details:not([open]) > :not(summary) { display: none; }
button { box-sizing: border-box; text-align: center; }
input, select, textarea { font-family: -apple-system; font-size: 13.333px; font-weight: 400; color: #000000; letter-spacing: normal; text-align: left; }
input { box-sizing: border-box; width: 153px; padding: 1px 3px; border: 1px solid #767676; border-radius: 3px; background-color: #ffffff; vertical-align: middle; }
input[type="checkbox"], input[type="radio"] { width: 13px; height: 13px; margin: 3px 3px 3px 4px; padding: 0; border-radius: 3px; vertical-align: -2px; }
input[type="radio"] { border-radius: 50%; }
input[type="range"] { width: 129px; height: 16px; margin: 2px; padding: 0; border: 0; background-color: transparent; }
input[type="color"] { width: 50px; height: 27px; padding: 4px 3px; background-color: #efefef; }
input[type="button"], input[type="submit"], input[type="reset"] { width: auto; padding: 1px 6px; background-color: #efefef; text-align: center; }
select { box-sizing: border-box; padding: 1px 4px; border: 1px solid #767676; border-radius: 4px; background-color: #ffffff; vertical-align: middle; }
textarea { display: inline-block; box-sizing: border-box; width: 182px; height: 36px; padding: 2px; border: 1px solid #767676; background-color: #ffffff; white-space: pre-wrap; overflow: hidden; font-family: ui-monospace; }
)CSS";
        return s;
    }();
    return css;
}

static void IgnoreRedraw(const lh::position &) {}

static bool UsesPercentHeight(const lh::element::ptr &el)
{
    return el->css().get_height().units() == lh::css_units_percentage ||
           el->css().get_min_height().units() == lh::css_units_percentage;
}

static bool NeedsViewportHeight(const lh::document::ptr &doc)
{
    lh::position::vector fixed;
    doc->get_fixed_boxes(fixed);
    if(!fixed.empty())
        return true;
    auto root = doc->root();
    if(!root)
        return false;
    if(UsesPercentHeight(root))
        return true;
    for(const auto &child : root->children())
        if(child->tag() == lh::_body_ && UsesPercentHeight(child))
            return true;
    return false;
}

static void CollectPositionedZ(const std::shared_ptr<lh::render_item> &item, std::set<int> &z)
{
    for(const auto &child : item->children())
    {
        if(child->src_el()->is_positioned())
            z.insert(child->src_el()->css().get_z_index());
        else
            CollectPositionedZ(child, z);
    }
}

static void DrawAsStackingContext(const std::shared_ptr<lh::render_item> &ri, lh::uint_ptr hdc, lh::pixel_t x, lh::pixel_t y)
{
    std::set<int> z;
    CollectPositionedZ(ri, z);
    for(int index : z)
        if(index < 0)
            ri->draw_children(hdc, x, y, nullptr, lh::draw_positioned, index);
    ri->draw_children(hdc, x, y, nullptr, lh::draw_block, 0);
    ri->draw_children(hdc, x, y, nullptr, lh::draw_floats, 0);
    ri->draw_children(hdc, x, y, nullptr, lh::draw_inlines, 0);
    for(int index : z)
        if(index >= 0)
            ri->draw_children(hdc, x, y, nullptr, lh::draw_positioned, index);
}

static bool CopyPixels(CGImageRef image, CGContextRef ctx)
{
    size_t width = CGImageGetWidth(image), height = CGImageGetHeight(image);
    if(CGImageGetBitsPerPixel(image) != 32 || CGImageGetBitmapInfo(image) != CGBitmapContextGetBitmapInfo(ctx) ||
       CGBitmapContextGetWidth(ctx) != width || CGBitmapContextGetHeight(ctx) != height)
        return false;
    CGDataProviderRef provider = CGImageGetDataProvider(image);
    CFDataRef         data     = provider ? CGDataProviderCopyData(provider) : nullptr;
    if(!data)
        return false;
    const uint8_t *src       = CFDataGetBytePtr(data);
    uint8_t       *dst       = static_cast<uint8_t *>(CGBitmapContextGetData(ctx));
    size_t         srcStride = CGImageGetBytesPerRow(image), dstStride = CGBitmapContextGetBytesPerRow(ctx);
    bool           ok        = dst && src && (size_t)CFDataGetLength(data) >= srcStride * height;
    if(ok)
        for(size_t y = 0; y < height; y++)
            memcpy(dst + y * dstStride, src + y * srcStride, width * 4);
    CFRelease(data);
    return ok;
}

@interface LHTextFragment ()
- (instancetype)initWithText:(NSString *)text font:(CTFontRef)font frame:(CGRect)frame;
@end

@implementation LHTextFragment
{
    CTFontRef _font;
    CTLineRef _line;
}

- (instancetype)initWithText:(NSString *)text font:(CTFontRef)font frame:(CGRect)frame
{
    if((self = [super init]))
    {
        _text  = [text copy];
        _font  = (CTFontRef)CFRetain(font);
        _frame = frame;
    }
    return self;
}

- (void)dealloc
{
    if(_line)
        CFRelease(_line);
    CFRelease(_font);
}

- (CTLineRef)line
{
    if(!_line)
    {
        NSDictionary *attributes = @{(__bridge NSString *)kCTFontAttributeName: (__bridge id)_font};
        NSAttributedString *string = [[NSAttributedString alloc] initWithString:_text attributes:attributes];
        _line = CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)string);
    }
    return _line;
}

- (NSUInteger)characterIndexForX:(CGFloat)x
{
    CFIndex index = CTLineGetStringIndexForPosition([self line], CGPointMake(x - _frame.origin.x, 0));
    if(index == kCFNotFound || index < 0)
        return 0;
    return MIN((NSUInteger)index, _text.length);
}

- (CGFloat)xForCharacterIndex:(NSUInteger)index
{
    index = MIN(index, _text.length);
    return _frame.origin.x + CTLineGetOffsetForStringIndex([self line], (CFIndex)index, nullptr);
}

@end

@interface LHDisplayList ()
- (instancetype)initWithOps:(std::vector<LHDrawOp>)ops;
@end

static uint64_t OpKey(const LHDrawOp &op)
{
    uint64_t     key       = op.hash;
    const double values[4] = {op.bounds.origin.x, op.bounds.origin.y, op.bounds.size.width, op.bounds.size.height};
    const auto  *bytes     = reinterpret_cast<const unsigned char *>(values);
    for(size_t i = 0; i < sizeof(values); i++)
        key = (key ^ bytes[i]) * 0x100000001b3ULL;
    return key;
}

static std::vector<std::pair<uint64_t, size_t>> OpIdentities(const std::vector<LHDrawOp> &ops)
{
    std::unordered_map<uint64_t, size_t>      seen;
    std::vector<std::pair<uint64_t, size_t>> result;
    result.reserve(ops.size());
    for(const auto &op : ops)
    {
        uint64_t key = OpKey(op);
        result.emplace_back(key, seen[key]++);
    }
    return result;
}

struct OpIdentityHash
{
    size_t operator()(const std::pair<uint64_t, size_t> &v) const { return std::hash<uint64_t>()(v.first ^ (v.second * 0x9e3779b97f4a7c15ULL)); }
};

using OpIdentitySet = std::unordered_set<std::pair<uint64_t, size_t>, OpIdentityHash>;

@implementation LHDisplayList
{
    std::vector<LHDrawOp>                    _ops;
    std::vector<LHGPUOp>                     _gpuOps;
    BOOL                                     _hasGPU;
    BOOL                                     _gpuUnsupported;
    std::once_flag                           _identitiesOnce;
    std::vector<std::pair<uint64_t, size_t>> _identities;
    OpIdentitySet                            _identitySet;
}

- (const std::vector<std::pair<uint64_t, size_t>> &)identities
{
    std::call_once(_identitiesOnce, [self] {
        _identities = OpIdentities(_ops);
        _identitySet.reserve(_identities.size());
        _identitySet.insert(_identities.begin(), _identities.end());
    });
    return _identities;
}

- (const OpIdentitySet &)identitySet
{
    [self identities];
    return _identitySet;
}

- (void)setGPUOps:(std::vector<LHGPUOp>)ops unsupported:(BOOL)unsupported
{
    _gpuOps         = std::move(ops);
    _hasGPU         = YES;
    _gpuUnsupported = unsupported;
}

- (const std::vector<LHGPUOp> *)gpuOps
{
    return _hasGPU ? &_gpuOps : nullptr;
}

- (BOOL)gpuUnsupported
{
    return _gpuUnsupported;
}

- (BOOL)gpuSupported
{
    return _hasGPU && !_gpuUnsupported;
}

- (instancetype)initWithOps:(std::vector<LHDrawOp>)ops
{
    if((self = [super init]))
        _ops = std::move(ops);
    return self;
}

- (NSArray<NSValue *> *)changedRectsSince:(LHDisplayList *)previous
{
    const std::vector<LHDrawOp> &a = previous->_ops;
    const std::vector<LHDrawOp> &b = _ops;
    NSMutableArray<NSValue *>   *rects = [NSMutableArray array];
    auto add = [&](const LHDrawOp &op) {
        if(!CGRectIsNull(op.bounds) && !CGRectIsEmpty(op.bounds))
            [rects addObject:[NSValue valueWithRect:NSRectFromCGRect(op.bounds)]];
    };
    const auto &ida = [previous identities], &idb = [self identities];
    const auto &inA = [previous identitySet], &inB = [self identitySet];
    std::vector<std::pair<uint64_t, size_t>> commonA, commonB;
    for(size_t i = 0; i < a.size(); i++)
    {
        if(inB.count(ida[i]))
            commonA.push_back(ida[i]);
        else
            add(a[i]);
    }
    for(size_t i = 0; i < b.size(); i++)
    {
        if(inA.count(idb[i]))
            commonB.push_back(idb[i]);
        else
            add(b[i]);
    }
    if(commonA != commonB)
    {
        [rects removeAllObjects];
        for(const auto &op : a)
            add(op);
        for(const auto &op : b)
            add(op);
    }
    return rects;
}

static std::unordered_multimap<uint64_t, CGRect> OpShapes(const std::vector<LHDrawOp> &ops)
{
    std::unordered_multimap<uint64_t, CGRect> shapes;
    shapes.reserve(ops.size());
    for(const auto &op : ops)
        shapes.emplace(op.shape, op.bounds);
    return shapes;
}

static bool HasMovedCopy(const std::unordered_multimap<uint64_t, CGRect> &shapes, const LHDrawOp &op, CGFloat dy)
{
    if(CGRectIsNull(op.bounds))
        return false;
    auto range = shapes.equal_range(op.shape);
    for(auto it = range.first; it != range.second; ++it)
    {
        const CGRect &r = it->second;
        if(!CGRectIsNull(r) && std::fabs(r.origin.x - op.bounds.origin.x) < 0.05 && std::fabs(r.origin.y - op.bounds.origin.y - dy) <= 1
           && std::fabs(r.size.width - op.bounds.size.width) < 0.05 && std::fabs(r.size.height - op.bounds.size.height) < 0.05)
            return true;
    }
    return false;
}

static CGRect OpInk(const LHDrawOp &op)
{
    CGRect ink = CGRectInset(op.bounds, 1, 1);
    return CGRectIsNull(ink) ? op.bounds : ink;
}

static bool MovedDistance(const std::vector<LHDrawOp> &ops, const std::unordered_multimap<uint64_t, CGRect> &previous, CGFloat estimate, CGFloat top, CGFloat *distance)
{
    std::unordered_map<int64_t, size_t> votes;
    for(const auto &op : ops)
    {
        if(CGRectIsNull(op.bounds) || CGRectGetMinY(op.bounds) < top - 1)
            continue;
        auto range = previous.equal_range(op.shape);
        for(auto it = range.first; it != range.second; ++it)
        {
            const CGRect &r = it->second;
            if(CGRectIsNull(r) || std::fabs(r.origin.x - op.bounds.origin.x) >= 0.05)
                continue;
            const CGFloat dy = op.bounds.origin.y - r.origin.y;
            if(std::fabs(dy - estimate) < 1.5)
                votes[static_cast<int64_t>(std::llround(dy * 64))]++;
        }
    }
    if(votes.empty())
        return false;
    auto best = std::max_element(votes.begin(), votes.end(), [](const auto &l, const auto &r) { return l.second < r.second; });
    *distance = static_cast<CGFloat>(best->first) / 64;
    return true;
}

static CGFloat ChangedBottom(const std::vector<LHDrawOp> &ops, const std::unordered_multimap<uint64_t, CGRect> &other, CGFloat dy, CGFloat top, CGFloat offset)
{
    std::vector<CGRect> moved, changed;
    for(const auto &op : ops)
    {
        if(CGRectIsNull(op.bounds) || CGRectIsEmpty(op.bounds))
            continue;
        const CGRect ink = OpInk(op);
        if(CGRectGetMinY(ink) < top - 1)
            continue;
        (HasMovedCopy(other, op, dy) ? moved : changed).push_back(ink);
    }
    CGFloat bottom = -CGFLOAT_MAX;
    for(const auto &ink : changed)
    {
        const CGPoint center = CGPointMake(CGRectGetMidX(ink), CGRectGetMidY(ink));
        if(std::any_of(moved.begin(), moved.end(), [&](const CGRect &box) { return CGRectContainsPoint(box, center); }))
            continue;
        bottom = std::max(bottom, CGRectGetMaxY(ink) + offset);
    }
    return bottom;
}

- (CGFloat)unchangedBottomSince:(LHDisplayList *)previous previousHeight:(CGFloat)previousHeight height:(CGFloat)height below:(CGFloat)top
{
    const std::vector<LHDrawOp> &a   = previous->_ops;
    const std::vector<LHDrawOp> &b   = _ops;
    const auto                   inA = OpShapes(a);
    const auto                   inB = OpShapes(b);
    CGFloat                      delta;
    if(!MovedDistance(b, inA, height - previousHeight, top, &delta))
        return 0;
    const CGFloat start = std::max({top, ChangedBottom(b, inA, -delta, top, 0), ChangedBottom(a, inB, delta, top, delta)});
    return std::max<CGFloat>(0, height - start);
}

@end

struct PixelSnap
{
    float saved;
    explicit PixelSnap(CGContextRef ctx) : saved(lh::pixel_snap_scale())
    {
        const CGAffineTransform m = CGContextGetCTM(ctx);
        const CGFloat           k = std::hypot(m.a, m.b);
        lh::pixel_snap_scale()    = k >= 1 && k <= 16 && std::isfinite(k) ? static_cast<float>(k) : 1.0f;
    }
    ~PixelSnap() { lh::pixel_snap_scale() = saved; }
};

static bool LHPaintTraceUncached()
{
    static const bool uncached = getenv("LH_NO_TRACE_CACHE") != nullptr;
    return uncached;
}

@implementation LHDocument
{
    std::mutex                   _lock;
    std::unique_ptr<LHContainer> _container;
    std::string                  _htmlSource;
    std::vector<const lh::render_item *> _paintTrace;
    BOOL                         _paintTraceValid;
    NSString                    *_html;
    lh::document::ptr            _doc;
    CGFloat                      _lastMaxWidth;
    CGFloat                      _preferredWidth;
    CGSize                       _size;
    BOOL                         _paintOnly;
    BOOL                         _pointerRestyled;
    NSString                    *_cursor;
    CGColorSpaceRef              _colorSpace;
    BOOL                         _noSubtreeCulling;
    std::string                  _master;
    BOOL                         _updatable;
    BOOL                         _consumed;
    BOOL                         _consumedCulling;
    BOOL                         _reusedPrevious;
    CGFloat                      _consumedMaxWidth;
    NSString                    *_exclusionAttribute;
    NSSet<NSString *>           *_exclusions;
    NSDictionary                *_declarations;
    NSArray                     *_declarationsKey;
    uint64_t                     _declarationsEpoch;
}

- (void)bindContainerLocked
{
    __weak LHDocument *weakSelf = self;
    _container->on_anchor = [weakSelf](const std::string &url) {
        NSString *u = @(url.c_str());
        dispatch_async(dispatch_get_main_queue(), ^{
            LHDocument *s = weakSelf;
            if(s.anchorHandler && u)
                s.anchorHandler(u);
        });
    };
    _container->on_action = [weakSelf](const std::string &action, const std::string &value) {
        NSString *a = @(action.c_str()), *v = value.empty() ? nil : @(value.c_str());
        dispatch_async(dispatch_get_main_queue(), ^{
            LHDocument *s = weakSelf;
            if(s.actionHandler && a)
                s.actionHandler(a, v);
        });
    };
    LHDocument *__unsafe_unretained unsafeSelf = self;
    _container->on_cursor = [unsafeSelf](const std::string &cursor) {
        unsafeSelf->_cursor = @(cursor.c_str()) ?: @"auto";
    };
}

static std::string MasterWithCSS(NSString *css)
{
    std::string master = MasterCSS();
    master += "\n";
    master += css.UTF8String ?: "";
    return master;
}

- (instancetype)initWithHTML:(NSString *)html css:(NSString *)css
{
    if((self = [super init]))
    {
        _html         = [html copy];
        _lastMaxWidth = -1;
        _cursor       = @"auto";
        _container    = std::make_unique<LHContainer>();
        [self bindContainerLocked];

        double t0 = NowMs();
        _doc = lh::document::createFromString(lh::estring(html.UTF8String ?: ""), _container.get(), MasterWithCSS(css));
        _parseMilliseconds = NowMs() - t0;
    }
    return self;
}

- (instancetype)initWithHTML:(NSString *)html css:(NSString *)css previous:(LHDocument *)previous
{
    if((self = [super init]))
    {
        _html         = [html copy];
        _lastMaxWidth = -1;
        _cursor       = @"auto";
        _updatable    = YES;
        _master       = MasterWithCSS(css);

        double            t0   = NowMs();
        const std::string body = html.UTF8String ?: "";
        if(previous)
        {
            std::lock_guard<std::mutex> guard(previous->_lock);
            if(previous->_doc && previous->_updatable && previous->_master == _master &&
               previous->_doc->update_from_string(body, nullptr))
            {
                _doc       = std::move(previous->_doc);
                _container = std::move(previous->_container);
                _cursor    = previous->_cursor;
                [self takeDeclarationsLocked:previous];
                previous->_consumed         = YES;
                previous->_consumedMaxWidth = previous->_lastMaxWidth;
                previous->_consumedCulling  = _container->clip_culling();
                _reusedPrevious             = YES;
                if(_doc->last_patch_paint_only() && previous->_lastMaxWidth > 0 && !previous->_pointerRestyled)
                {
                    _lastMaxWidth = previous->_lastMaxWidth;
                    _size         = previous->_size;
                    _paintOnly    = YES;
                }
            }
        }
        if(!_doc)
        {
            _container = std::make_unique<LHContainer>();
            _doc       = lh::document::createUpdatableFromString(body, _container.get(), _master);
        }
        [self bindContainerLocked];
        _parseMilliseconds = NowMs() - t0;
    }
    return self;
}

- (nullable instancetype)initWithPrevious:(LHDocument *)previous styles:(NSDictionary<NSString *, NSString *> *)styles attribute:(NSString *)attribute css:(NSString *)css
{
    if(!(self = [super init]))
        return nil;
    _lastMaxWidth = -1;
    _cursor       = @"auto";
    _updatable    = YES;
    _master       = MasterWithCSS(css);
    double t0     = NowMs();
    std::lock_guard<std::mutex> guard(previous->_lock);
    if(!previous->_doc || !previous->_updatable || previous->_master != _master || !previous->_doc->root() || styles.count == 0)
        return nil;
    std::unordered_map<std::string, std::string> wanted;
    for(NSString *key in styles)
        wanted.emplace(key.UTF8String ?: "", styles[key].UTF8String ?: "");
    std::vector<std::pair<lh::element::ptr, std::string>> patches;
    const char *name = attribute.UTF8String;
    std::function<void(const lh::element::ptr &)> visit = [&](const lh::element::ptr &el) {
        if(patches.size() == wanted.size())
            return;
        if(const char *v = el->get_attr(name))
        {
            auto it = wanted.find(v);
            if(it != wanted.end())
                patches.emplace_back(el, it->second);
        }
        for(const auto &child : el->children())
            visit(child);
    };
    visit(previous->_doc->root());
    if(patches.size() != wanted.size() || !previous->_doc->update_styles(patches, nullptr))
        return nil;
    if(![self adoptPatchedLocked:previous])
        return nil;
    _parseMilliseconds = NowMs() - t0;
    return self;
}

- (nullable instancetype)initWithPrevious:(LHDocument *)previous changes:(NSData *)changes attribute:(NSString *)attribute css:(NSString *)css
{
    if(!(self = [super init]))
        return nil;
    _lastMaxWidth = -1;
    _cursor       = @"auto";
    _updatable    = YES;
    _master       = MasterWithCSS(css);
    double t0     = NowMs();
    std::lock_guard<std::mutex> guard(previous->_lock);
    if(!previous->_doc || !previous->_updatable || previous->_master != _master || !previous->_doc->root() || changes.length == 0)
        return nil;
    const char *bytes = static_cast<const char *>(changes.bytes);
    const char *limit = bytes + changes.length;
    if(limit[-1] != 0)
        return nil;
    struct entry
    {
        std::string_view name, value;
    };
    std::unordered_map<std::string_view, std::vector<entry>> wanted;
    for(const char *p = bytes; p < limit;)
    {
        std::string_view parts[3];
        for(auto &part : parts)
        {
            if(p >= limit)
                return nil;
            part = std::string_view(p);
            p += part.size() + 1;
        }
        wanted[parts[0]].push_back({parts[1], parts[2]});
    }
    std::vector<lh::document::hole_change> patches;
    size_t matched = 0;
    const char *name = attribute.UTF8String;
    std::vector<lh::element *> pending{previous->_doc->root().get()};
    while(!pending.empty() && matched < wanted.size())
    {
        lh::element *el = pending.back();
        pending.pop_back();
        if(const char *v = el->get_attr(name))
        {
            auto it = wanted.find(std::string_view(v));
            if(it != wanted.end() && !it->second.empty())
            {
                matched++;
                const lh::element::ptr owner = el->shared_from_this();
                for(const auto &item : it->second)
                {
                    lh::document::hole_change change;
                    change.owner = owner;
                    change.text  = item.name.empty();
                    change.name  = std::string(item.name);
                    change.value = std::string(item.value);
                    patches.push_back(std::move(change));
                }
                it->second.clear();
            }
        }
        const auto &children = el->children();
        for(auto child = children.rbegin(); child != children.rend(); ++child)
            pending.push_back(child->get());
    }
    if(matched != wanted.size() || !previous->_doc->update_holes(patches, nullptr))
        return nil;
    if(![self adoptPatchedLocked:previous])
        return nil;
    _parseMilliseconds = NowMs() - t0;
    return self;
}

- (BOOL)adoptPatchedLocked:(LHDocument *)previous
{
    std::string html;
    if(!previous->_doc->template_html(html))
    {
        previous->_updatable = NO;
        previous->_doc.reset();
        previous->_container.reset();
        previous->_consumed         = YES;
        previous->_consumedMaxWidth = previous->_lastMaxWidth;
        return NO;
    }
    _htmlSource                 = std::move(html);
    _doc                        = std::move(previous->_doc);
    _container                  = std::move(previous->_container);
    _cursor                     = previous->_cursor;
    [self takeDeclarationsLocked:previous];
    previous->_consumed         = YES;
    previous->_consumedMaxWidth = previous->_lastMaxWidth;
    previous->_consumedCulling  = _container->clip_culling();
    _reusedPrevious             = YES;
    if(_doc->last_patch_paint_only() && previous->_lastMaxWidth > 0 && !previous->_pointerRestyled)
    {
        _lastMaxWidth = previous->_lastMaxWidth;
        _size         = previous->_size;
        _paintOnly    = YES;
    }
    [self bindContainerLocked];
    return YES;
}

+ (NSUInteger)cachedFontCount
{
    return LHContainer::cached_font_count();
}

+ (NSUInteger)rasterCacheBytes
{
    return LHContainer::raster_cache_bytes();
}

+ (void)purgeCaches
{
    LHContainer::purge_caches();
}

- (NSString *)html
{
    std::lock_guard<std::mutex> guard(_lock);
    if(!_html)
        _html = [[NSString alloc] initWithBytes:_htmlSource.data() length:_htmlSource.size() encoding:NSUTF8StringEncoding] ?: @"";
    return _html;
}

- (BOOL)reusedPrevious
{
    return _reusedPrevious;
}

- (BOOL)reusedLayout
{
    return _paintOnly;
}

- (NSArray<NSValue *> *)patchDirtyRects
{
    std::lock_guard<std::mutex> guard(_lock);
    std::vector<lh::element::ptr> regions;
    if(!_paintOnly || !_doc || !_doc->root_render() || !_doc->last_patch_regions(regions))
        return nil;
    if(regions.empty())
        return @[];
    const lh::render_item *root = _doc->root_render().get();
    std::unordered_map<const lh::element *, std::shared_ptr<lh::render_item>> found;
    for(const auto &region : regions)
    {
        if(found.count(region.get()))
            continue;
        std::shared_ptr<lh::render_item> single;
        int                              count = 0;
        region->for_each_render_item([&](const std::shared_ptr<lh::render_item> &ri) {
            auto top = ri;
            while(auto up = top->parent())
                top = up;
            if(top.get() != root)
                return;
            count++;
            single = ri;
        });
        if(count != 1)
            return nil;
        found.emplace(region.get(), single);
    }
    NSMutableArray<NSValue *> *rects = [NSMutableArray arrayWithCapacity:found.size()];
    for(const auto &entry : found)
    {
        const auto &ri = entry.second;
        if(!ri)
            return nil;
        const auto el       = ri->src_el();
        const bool text     = el->is_text();
        const bool replaced = std::dynamic_pointer_cast<lh::el_svg>(el) || std::dynamic_pointer_cast<lh::el_image>(el);
        if(!text && el->is_inline() && !el->is_inline_box() && !replaced)
            return nil;
        for(auto p = ri; p; p = p->parent())
        {
            const auto &css = p->src_el()->css();
            if(css.is_paint_group() || (css.get_position() != lh::element_position_static && css.get_position() != lh::element_position_relative))
                return nil;
        }
        if(!text && (!el->css().get_box_shadows().empty() || !el->css().get_text_shadows().empty() ||
                     (el->css().get_outline().style != lh::border_style_none && el->css().get_outline().style != lh::border_style_hidden)))
            return nil;
        CGRect box = BorderBox(ri);
        if(text)
        {
            const auto parent = el->parent();
            if(!parent || !parent->css().get_text_shadows().empty())
                return nil;
            const CGFloat slack = std::max<CGFloat>(4, std::ceil(box.size.height) / 2 + 1) + 1;
            box                 = CGRectInset(box, -slack, -slack);
        }
        if(CGRectIsEmpty(box))
            continue;
        [rects addObject:[NSValue valueWithRect:NSRectFromCGRect(CGRectInset(box, -1, -1))]];
    }
    return rects;
}

- (void)takeDeclarationsLocked:(LHDocument *)previous
{
    _declarations              = previous->_declarations;
    _declarationsKey           = previous->_declarationsKey;
    _declarationsEpoch         = previous->_declarationsEpoch;
    previous->_declarations    = nil;
    previous->_declarationsKey = nil;
}

- (void)restoreLocked
{
    if(!_consumed || _doc)
        return;
    _consumed     = NO;
    _declarations = nil;
    _paintTraceValid = NO;
    _container = std::make_unique<LHContainer>();
    _container->set_clip_culling(_consumedCulling);
    [self bindContainerLocked];
    _doc          = lh::document::createUpdatableFromString(_html ? std::string(_html.UTF8String ?: "") : _htmlSource, _container.get(), _master);
    _lastMaxWidth = -1;
    if(_consumedMaxWidth > 0)
        [self layoutLockedWithMaxWidth:_consumedMaxWidth];
}

- (void)dealloc
{
    _doc.reset();
    _container.reset();
    if(_colorSpace)
        CGColorSpaceRelease(_colorSpace);
}

- (CGColorSpaceRef)colorSpace
{
    std::lock_guard<std::mutex> guard(_lock);
    return _colorSpace;
}

- (void)setColorSpace:(CGColorSpaceRef)colorSpace
{
    std::lock_guard<std::mutex> guard(_lock);
    if(colorSpace == _colorSpace)
        return;
    if(_colorSpace)
        CGColorSpaceRelease(_colorSpace);
    _colorSpace = colorSpace ? CGColorSpaceRetain(colorSpace) : nullptr;
}

- (CGSize)size
{
    std::lock_guard<std::mutex> guard(_lock);
    return _size;
}

- (BOOL)clipCulling
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    return _container->clip_culling();
}

- (void)setClipCulling:(BOOL)clipCulling
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    _container->set_clip_culling(clipCulling);
}

- (BOOL)subtreeCulling
{
    std::lock_guard<std::mutex> guard(_lock);
    return !_noSubtreeCulling;
}

- (void)setSubtreeCulling:(BOOL)subtreeCulling
{
    std::lock_guard<std::mutex> guard(_lock);
    _noSubtreeCulling = !subtreeCulling;
}

- (NSString *)cursor
{
    std::lock_guard<std::mutex> guard(_lock);
    return _cursor;
}

- (void)fitViewportHeightLockedWithWidth:(CGFloat)width
{
    _paintTraceValid = NO;
    if(!NeedsViewportHeight(_doc))
        return;
    _container->set_viewport(lh::position(0, 0, static_cast<float>(width), static_cast<float>(std::ceil(_doc->height().value()))));
    _doc->render(static_cast<float>(width));
}

- (CGSize)layoutLockedWithMaxWidth:(CGFloat)maxWidth
{
    _paintTraceValid = NO;
    [self restoreLocked];
    maxWidth = std::max<CGFloat>(1, std::floor(maxWidth));
    if(!_doc)
        return CGSizeZero;
    if(std::fabs(maxWidth - _lastMaxWidth) < 0.5)
        return _size;
    if(_preferredWidth > 0.5 && _preferredWidth <= maxWidth + 0.5)
    {
        double  t0 = NowMs();
        CGFloat vw = std::min<CGFloat>(_preferredWidth, maxWidth);
        _container->set_viewport(lh::position(0, 0, static_cast<float>(vw), 0));
        _doc->render(static_cast<float>(vw));
        CGFloat extent = std::ceil(_doc->width().value());
        if(vw >= maxWidth - 0.5 || extent <= vw + 0.5)
        {
            [self fitViewportHeightLockedWithWidth:vw];
            _lastMaxWidth       = maxWidth;
            _layoutMilliseconds = NowMs() - t0;
            _size = CGSizeMake(std::max(std::ceil(vw), extent), std::ceil(_doc->height().value()));
            return _size;
        }
    }
    _lastMaxWidth = maxWidth;

    double t0 = NowMs();
    _container->set_viewport(lh::position(0, 0, static_cast<float>(maxWidth), 0));
    lh::pixel_t natural = _doc->render(static_cast<float>(maxWidth));
    CGFloat     width   = maxWidth;
    if(natural.value() > 0 && natural.value() < maxWidth)
    {
        width = std::min<CGFloat>(maxWidth, std::ceil(natural.value() + 0.01));
        _container->set_viewport(lh::position(0, 0, static_cast<float>(width), 0));
        _doc->render(static_cast<float>(width));
    }
    CGFloat extent = std::ceil(_doc->width().value());
    for(int pass = 0; pass < 2 && extent > width + 0.5 && width < maxWidth; pass++)
    {
        width = std::min<CGFloat>(maxWidth, extent);
        _container->set_viewport(lh::position(0, 0, static_cast<float>(width), 0));
        _doc->render(static_cast<float>(width));
        extent = std::ceil(_doc->width().value());
    }
    for(int pass = 0; pass < 3 && extent > maxWidth + 0.5; pass++)
    {
        CGFloat target = std::floor(width - (extent - maxWidth));
        if(target < maxWidth / 2)
            break;
        _container->set_viewport(lh::position(0, 0, static_cast<float>(target), 0));
        _doc->render(static_cast<float>(target));
        CGFloat narrowed = std::ceil(_doc->width().value());
        if(narrowed >= extent - 0.5)
        {
            _container->set_viewport(lh::position(0, 0, static_cast<float>(width), 0));
            _doc->render(static_cast<float>(width));
            break;
        }
        width  = target;
        extent = narrowed;
    }
    [self fitViewportHeightLockedWithWidth:width];
    _layoutMilliseconds = NowMs() - t0;
    _size               = CGSizeMake(std::max(std::ceil(width), extent), std::ceil(_doc->height().value()));
    return _size;
}

- (CGSize)layoutWithMaxWidth:(CGFloat)maxWidth
{
    std::lock_guard<std::mutex> guard(_lock);
    return [self layoutLockedWithMaxWidth:maxWidth];
}

- (CGSize)layoutWithMaxWidth:(CGFloat)maxWidth preferredWidth:(CGFloat)preferredWidth
{
    std::lock_guard<std::mutex> guard(_lock);
    _preferredWidth = preferredWidth;
    CGSize size     = [self layoutLockedWithMaxWidth:maxWidth];
    _preferredWidth = 0;
    return size;
}

- (void)resolveExclusionsLocked
{
    _doc->skip_paint.clear();
    if(_exclusions.count == 0 || !_doc->root())
        return;
    std::unordered_set<std::string> wanted;
    for(NSString *value in _exclusions)
        wanted.insert(value.UTF8String);
    const char *key = _exclusionAttribute.UTF8String;
    std::function<void(const lh::element::ptr &)> visit = [&](const lh::element::ptr &el) {
        const char *value = el->get_attr(key);
        if(value && wanted.count(value))
            _doc->skip_paint.insert(el.get());
        for(const auto &child : el->children())
            visit(child);
    };
    visit(_doc->root());
}

- (void)setPaintExclusions:(NSSet<NSString *> *)values attribute:(NSString *)attribute
{
    std::lock_guard<std::mutex> guard(_lock);
    _exclusions         = [values copy];
    _exclusionAttribute = [attribute copy];
}

- (std::shared_ptr<lh::render_item>)renderItemLockedWithAttribute:(NSString *)attribute value:(NSString *)value
{
    if(!_doc || !_doc->root_render())
        return nullptr;
    const char                      *key    = attribute.UTF8String;
    const std::string                wanted = value.UTF8String;
    std::shared_ptr<lh::render_item> found;
    std::function<void(const std::shared_ptr<lh::render_item> &)> visit = [&](const std::shared_ptr<lh::render_item> &ri) {
        if(found)
            return;
        const char *v = ri->src_el()->get_attr(key);
        if(v && wanted == v)
        {
            found = ri;
            return;
        }
        for(const auto &child : ri->children())
            visit(child);
    };
    visit(_doc->root_render());
    return found;
}

static CGRect BorderBox(const std::shared_ptr<lh::render_item> &ri)
{
    const lh::position p      = ri->get_placement();
    const CGFloat      left   = ri->padding_left().value() + ri->border_left().value();
    const CGFloat      top    = ri->padding_top().value() + ri->border_top().value();
    const CGFloat      right  = ri->padding_right().value() + ri->border_right().value();
    const CGFloat      bottom = ri->padding_bottom().value() + ri->border_bottom().value();
    return CGRectMake(p.x.value() - left, p.y.value() - top, p.width.value() + left + right, p.height.value() + top + bottom);
}

static CGRect TightInk(CGContextRef ctx, CGRect ink, CGRect box, CGFloat scale)
{
    static const bool disabled = getenv("LH_NO_TIGHT_INK") != nullptr;
    if(disabled)
        return ink;
    CGFloat step = 0;
    for(CGFloat k : {1.0, 2.0, 4.0, 8.0})
    {
        if(std::fabs(k * scale - std::round(k * scale)) < 1e-6)
        {
            step = k;
            break;
        }
    }
    const uint8_t* data = static_cast<const uint8_t*>(CGBitmapContextGetData(ctx));
    const size_t   pw = CGBitmapContextGetWidth(ctx), ph = CGBitmapContextGetHeight(ctx), row = CGBitmapContextGetBytesPerRow(ctx);
    if(step == 0 || !data || CGBitmapContextGetBitsPerPixel(ctx) != 32 || (CGBitmapContextGetBitmapInfo(ctx) & kCGBitmapAlphaInfoMask) != kCGImageAlphaPremultipliedFirst || (CGBitmapContextGetBitmapInfo(ctx) & kCGBitmapByteOrderMask) != kCGBitmapByteOrder32Little)
        return ink;
    size_t minX = pw, minY = ph, maxX = 0, maxY = 0;
    for(size_t py = 0; py < ph; py++)
    {
        const uint32_t* line = reinterpret_cast<const uint32_t*>(data + py * row);
        size_t first = pw;
        for(size_t px = 0; px < pw; px++)
        {
            if(line[px] >> 24)
            {
                first = px;
                break;
            }
        }
        if(first == pw)
            continue;
        size_t last = first;
        for(size_t px = pw; px-- > first;)
        {
            if(line[px] >> 24)
            {
                last = px;
                break;
            }
        }
        const size_t top = py;
        minX = std::min(minX, first);
        maxX = std::max(maxX, last + 1);
        minY = std::min(minY, top);
        maxY = std::max(maxY, top + 1);
    }
    CGRect painted = minX < maxX ? CGRectMake(ink.origin.x + minX / scale, ink.origin.y + minY / scale, (maxX - minX) / scale, (maxY - minY) / scale) : CGRectNull;
    CGRect wanted = CGRectIntersection(CGRectUnion(painted, box), ink);
    if(CGRectIsNull(wanted) || CGRectIsEmpty(wanted))
        return ink;
    CGFloat x0 = ink.origin.x + std::floor((CGRectGetMinX(wanted) - ink.origin.x) / step) * step;
    CGFloat y0 = ink.origin.y + std::floor((CGRectGetMinY(wanted) - ink.origin.y) / step) * step;
    CGFloat x1 = std::min(CGRectGetMaxX(ink), ink.origin.x + std::ceil((CGRectGetMaxX(wanted) - ink.origin.x) / step) * step);
    CGFloat y1 = std::min(CGRectGetMaxY(ink), ink.origin.y + std::ceil((CGRectGetMaxY(wanted) - ink.origin.y) / step) * step);
    return CGRectMake(x0, y0, x1 - x0, y1 - y0);
}

- (CGImageRef)copyImageOfElementWithAttribute:(NSString *)attribute value:(NSString *)value scale:(CGFloat)scale inkBounds:(CGRect *)inkOut borderBox:(CGRect *)boxOut
{
    return [self copyImageOfElementWithAttribute:attribute value:value scale:scale inkBounds:inkOut borderBox:boxOut radii:nullptr];
}

- (CGImageRef)copyImageOfElementWithAttribute:(NSString *)attribute value:(NSString *)value scale:(CGFloat)scale inkBounds:(CGRect *)inkOut borderBox:(CGRect *)boxOut radii:(CGFloat *)radiiOut
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    auto ri = [self renderItemLockedWithAttribute:attribute value:value];
    if(!ri)
        return nullptr;
    const CGRect       box       = BorderBox(ri);
    const lh::position placement = ri->get_placement();
    const lh::pixel_t  x         = placement.x - ri->pos().x;
    const lh::pixel_t  y         = placement.y - ri->pos().y;
    lh::position       redraw(static_cast<float>(box.origin.x), static_cast<float>(box.origin.y), static_cast<float>(box.size.width), static_cast<float>(box.size.height));
    ri->get_redraw_box(redraw, x, y);
    CGRect ink = CGRectIntegral(CGRectInset(CGRectUnion(box, CGRectMake(redraw.x.value(), redraw.y.value(), redraw.width.value(), redraw.height.value())), -48, -48));
    const size_t pw = (size_t)std::ceil(ink.size.width * scale), ph = (size_t)std::ceil(ink.size.height * scale);
    if(pw == 0 || ph == 0 || pw > 8192 || ph > 8192 || pw * ph > 16u * 1024u * 1024u)
        return nullptr;
    CGContextRef ctx = [self createContextWithWidth:pw height:ph];
    if(!ctx)
        return nullptr;
    CGContextTranslateCTM(ctx, 0, ph);
    CGContextScaleCTM(ctx, scale, -scale);
    CGContextTranslateCTM(ctx, -ink.origin.x, -ink.origin.y);
    CGContextSetAllowsFontSmoothing(ctx, false);
    CGContextSetShouldAntialias(ctx, true);
    auto skipped = _doc->skip_paint;
    [self resolveExclusionsLocked];
    _doc->skip_paint.erase(ri->src_el().get());
    PixelSnap snap(ctx);
    ri->src_el()->draw(reinterpret_cast<lh::uint_ptr>(ctx), x, y, nullptr, ri);
    if(ri->is_visible())
        DrawAsStackingContext(ri, reinterpret_cast<lh::uint_ptr>(ctx), x, y);
    CGRect tight = TightInk(ctx, ink, box, scale);
    CGImageRef image = CGBitmapContextCreateImage(ctx);
    CGContextRelease(ctx);
    if(image && !CGRectEqualToRect(tight, ink))
    {
        CGRect pixels = CGRectMake(std::round((tight.origin.x - ink.origin.x) * scale), std::round((tight.origin.y - ink.origin.y) * scale), std::round(tight.size.width * scale), std::round(tight.size.height * scale));
        pixels = CGRectIntersection(pixels, CGRectMake(0, 0, pw, ph));
        CGImageRef cropped = CGRectIsEmpty(pixels) ? nullptr : CGImageCreateWithImageInRect(image, pixels);
        if(cropped)
        {
            CGImageRelease(image);
            image = cropped;
            ink   = tight;
        }
    }
    if(inkOut)
        *inkOut = ink;
    if(boxOut)
        *boxOut = box;
    if(radiiOut)
    {
        auto r = ri->src_el()->css().get_borders().radius.calc_percents(lh::pixel_t(static_cast<float>(box.size.width)), lh::pixel_t(static_cast<float>(box.size.height)));
        r.fix_values(lh::pixel_t(static_cast<float>(box.size.width)), lh::pixel_t(static_cast<float>(box.size.height)));
        const CGFloat values[8] = {r.top_left_x.value(), r.top_left_y.value(), r.top_right_x.value(), r.top_right_y.value(), r.bottom_right_x.value(), r.bottom_right_y.value(), r.bottom_left_x.value(), r.bottom_left_y.value()};
        for(int i = 0; i < 8; i++)
            radiiOut[i] = std::max<CGFloat>(0, values[i]);
    }
    return image;
}

- (BOOL)isElementWithAttribute:(NSString *)attribute value:(NSString *)value isolatedInRect:(CGRect)rect
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    auto ri = [self renderItemLockedWithAttribute:attribute value:value];
    if(!ri || (ri->src_el()->is_inline() && !ri->src_el()->is_inline_box()))
        return NO;
    std::unordered_set<const lh::render_item *> ancestors;
    for(auto p = ri->parent(); p; p = p->parent())
    {
        ancestors.insert(p.get());
        if(p->src_el()->css().get_overflow() > lh::overflow_visible || p->src_el()->css().is_paint_group())
            return NO;
    }
    bool isolated = true;
    std::function<void(const std::shared_ptr<lh::render_item> &)> visit = [&](const std::shared_ptr<lh::render_item> &item) {
        if(!isolated || item == ri)
            return;
        if(!ancestors.count(item.get()) && !std::dynamic_pointer_cast<lh::el_space>(item->src_el()))
        {
            CGRect box = BorderBox(item);
            if(box.size.width > 0 && box.size.height > 0 && CGRectIntersectsRect(box, rect))
            {
                isolated = false;
                return;
            }
        }
        for(const auto &child : item->children())
            visit(child);
    };
    visit(_doc->root_render());
    return isolated;
}

- (BOOL)isElementWithAttribute:(NSString *)attribute value:(NSString *)value unobscuredInRect:(CGRect)rect
{
    return [self isElementWithAttribute:attribute value:value unobscuredInRect:rect ignoring:[NSSet set]];
}

- (NSDictionary<NSString *, NSNumber *> *)paintOrderOfAttribute:(NSString *)attribute values:(NSSet<NSString *> *)values
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    NSMutableDictionary<NSString *, NSNumber *> *result = [NSMutableDictionary dictionary];
    if(!_doc || !_doc->root_render() || values.count == 0)
        return result;
    std::vector<const lh::render_item *> trace;
    trace.reserve(512);
    CGContextRef ctx = [self createContextWithWidth:1 height:1];
    if(!ctx)
        return result;
    auto saved        = _doc->skip_paint;
    _doc->skip_paint.clear();
    lh::g_paint_trace = &trace;
    _doc->draw(reinterpret_cast<lh::uint_ptr>(ctx), 0, 0, nullptr);
    lh::g_paint_trace = nullptr;
    _doc->skip_paint  = saved;
    CGContextRelease(ctx);
    const char *key = attribute.UTF8String;
    for(size_t index = 0; index < trace.size(); index++)
    {
        const char *v = trace[index]->src_el()->get_attr(key);
        if(!v)
            continue;
        NSString *string = @(v);
        if([values containsObject:string] && !result[string])
            result[string] = @(index);
    }
    return result;
}

- (BOOL)clipOfElementWithAttribute:(NSString *)attribute value:(NSString *)value rect:(CGRect *)rect radii:(CGFloat *)radii
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    auto ri = [self renderItemLockedWithAttribute:attribute value:value];
    if(!ri)
        return NO;
    std::shared_ptr<lh::render_item> clip;
    for(auto p = ri->parent(); p; p = p->parent())
    {
        if(p->src_el()->css().get_overflow() > lh::overflow_visible)
        {
            if(clip)
                return NO;
            clip = p;
        }
    }
    if(!clip)
        return NO;
    CGRect border = BorderBox(clip);
    CGFloat bl = clip->border_left().value(), bt = clip->border_top().value(), br = clip->border_right().value(), bb = clip->border_bottom().value();
    *rect = CGRectMake(border.origin.x + bl, border.origin.y + bt, border.size.width - bl - br, border.size.height - bt - bb);
    const auto r = clip->src_el()->css().get_borders().radius.calc_percents(lh::pixel_t(static_cast<float>(border.size.width)), lh::pixel_t(static_cast<float>(border.size.height)));
    CGFloat values[8] = {r.top_left_x.value() - bl, r.top_left_y.value() - bt, r.top_right_x.value() - br, r.top_right_y.value() - bt,
                         r.bottom_right_x.value() - br, r.bottom_right_y.value() - bb, r.bottom_left_x.value() - bl, r.bottom_left_y.value() - bb};
    for(int i = 0; i < 8; i++)
        radii[i] = std::max<CGFloat>(0, values[i]);
    return YES;
}

- (BOOL)isElementWithAttribute:(NSString *)attribute value:(NSString *)value unobscuredInRect:(CGRect)rect ignoring:(NSSet<NSString *> *)ignoring
{
    return [self isElementWithAttribute:attribute value:value unobscuredInRect:rect ignoring:ignoring clipped:NO];
}

- (BOOL)isElementWithAttribute:(NSString *)attribute value:(NSString *)value unobscuredInRect:(CGRect)rect ignoring:(NSSet<NSString *> *)ignoring clipped:(BOOL)clipped
{
    NSArray<NSString *> *obscuring = [self elementsObscuringAttribute:attribute value:value inRect:rect ignoring:ignoring clipped:clipped];
    return obscuring != nil && obscuring.count == 0;
}

- (nullable NSArray<NSString *> *)elementsObscuringAttribute:(NSString *)attribute value:(NSString *)value inRect:(CGRect)rect ignoring:(NSSet<NSString *> *)ignoring clipped:(BOOL)clipped
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    auto ri = [self renderItemLockedWithAttribute:attribute value:value];
    if(!ri || (ri->src_el()->is_inline() && !ri->src_el()->is_inline_box()) || CGRectIsNull(rect) || CGRectIsEmpty(rect))
        return nil;
    for(auto p = ri->parent(); p; p = p->parent())
    {
        if(p->src_el()->css().is_paint_group())
            return nil;
        if(p->src_el()->css().get_overflow() > lh::overflow_visible && !clipped)
        {
            CGRect border = BorderBox(p);
            CGFloat bl = p->border_left().value(), bt = p->border_top().value(), br = p->border_right().value(), bb = p->border_bottom().value();
            CGRect inner  = CGRectMake(border.origin.x + bl, border.origin.y + bt, border.size.width - bl - br, border.size.height - bt - bb);
            const auto &radius = p->src_el()->css().get_borders().radius.calc_percents(lh::pixel_t(static_cast<float>(border.size.width)), lh::pixel_t(static_cast<float>(border.size.height)));
            CGFloat corner = std::max({radius.top_left_x.value(), radius.top_left_y.value(), radius.top_right_x.value(), radius.top_right_y.value(),
                                       radius.bottom_left_x.value(), radius.bottom_left_y.value(), radius.bottom_right_x.value(), radius.bottom_right_y.value()});
            CGRect safe = CGRectInset(inner, corner, corner);
            if(!CGRectContainsRect(safe, rect))
                return nil;
        }
    }
    if(!_paintTraceValid || LHPaintTraceUncached())
    {
        _paintTrace.clear();
        _paintTrace.reserve(512);
        CGContextRef ctx = [self createContextWithWidth:1 height:1];
        if(!ctx)
            return nil;
        [self resolveExclusionsLocked];
        auto saved        = _doc->skip_paint;
        lh::g_paint_trace = &_paintTrace;
        _doc->skip_paint.clear();
        _doc->draw(reinterpret_cast<lh::uint_ptr>(ctx), 0, 0, nullptr);
        lh::g_paint_trace = nullptr;
        _doc->skip_paint  = saved;
        CGContextRelease(ctx);
        _paintTraceValid = YES;
    }
    const std::vector<const lh::render_item *> &trace = _paintTrace;
    const char *key = attribute.UTF8String;
    NSMutableOrderedSet<NSString *> *found = [NSMutableOrderedSet orderedSet];
    auto self_it = std::find(trace.begin(), trace.end(), ri.get());
    if(self_it == trace.end())
        return nil;
    for(auto it = std::next(self_it); it != trace.end(); ++it)
    {
        const lh::render_item *item = *it;
        bool descendant = false;
        for(auto p = item->parent(); p; p = p->parent())
        {
            if(p.get() == ri.get())
            {
                descendant = true;
                break;
            }
        }
        if(descendant || std::dynamic_pointer_cast<lh::el_space>(item->src_el()))
            continue;
        if(ignoring.count > 0)
        {
            const char *key     = attribute.UTF8String;
            bool        skipped = false;
            for(const lh::render_item *walk = item; walk && !skipped; walk = walk->parent().get())
            {
                const char *v = walk->src_el()->get_attr(key);
                if(v && [ignoring containsObject:@(v)])
                    skipped = true;
            }
            if(skipped)
                continue;
        }
        CGRect box = BorderBox(std::const_pointer_cast<lh::render_item>(item->shared_from_this()));
        if(box.size.width > 0 && box.size.height > 0 && CGRectIntersectsRect(box, rect))
        {
            NSString *owner = nil;
            for(const lh::render_item *walk = item; walk; walk = walk->parent().get())
            {
                bool ancestorOfSelf = false;
                for(auto p = ri->parent(); p; p = p->parent())
                {
                    if(p.get() == walk)
                    {
                        ancestorOfSelf = true;
                        break;
                    }
                }
                if(ancestorOfSelf)
                    break;
                if(const char *v = walk->src_el()->get_attr(key))
                {
                    owner = @(v);
                    break;
                }
            }
            if(!owner)
                return nil;
            [found addObject:owner];
            if(found.count > 8)
                return nil;
        }
    }
    return found.array;
}

- (void)drawLockedInContext:(CGContextRef)ctx origin:(CGPoint)origin clip:(CGRect)clip
{
    [self restoreLocked];
    if(!_doc)
        return;
    [self resolveExclusionsLocked];
    CGContextSaveGState(ctx);
    CGContextTranslateCTM(ctx, origin.x, origin.y);
    lh::position c(static_cast<float>(clip.origin.x), static_cast<float>(clip.origin.y),
                   static_cast<float>(clip.size.width), static_cast<float>(clip.size.height));
    PixelSnap  snap(ctx);
    static const bool disabled = getenv("LH_NO_SUBTREE_CULL") != nullptr;
    _doc->draw(reinterpret_cast<lh::uint_ptr>(ctx), 0, 0, &c, !_noSubtreeCulling && !disabled && _container->rasterizes());
    CGContextRestoreGState(ctx);
}

- (void)drawInContext:(CGContextRef)ctx origin:(CGPoint)origin clip:(CGRect)clip
{
    std::lock_guard<std::mutex> guard(_lock);
    [self drawLockedInContext:ctx origin:origin clip:clip];
}

- (CGImageRef)copyImageWithMaxWidth:(CGFloat)maxWidth rect:(CGRect)rect scale:(CGFloat)scale base:(CGImageRef)base dirty:(CGRect)dirty
{
    std::lock_guard<std::mutex> guard(_lock);
    [self layoutLockedWithMaxWidth:maxWidth];
    size_t pw = (size_t)std::ceil(rect.size.width * scale);
    size_t ph = (size_t)std::ceil(rect.size.height * scale);
    if(!_doc || pw == 0 || ph == 0)
        return nullptr;
    if(CGImageGetWidth(base) != pw || CGImageGetHeight(base) != ph)
        return [self copyImageLockedWithRect:rect scale:scale];
    CGRect local = CGRectIntersection(CGRectOffset(dirty, -rect.origin.x, -rect.origin.y),
                                      CGRectMake(0, 0, rect.size.width, rect.size.height));
    if(CGRectIsNull(local) || CGRectIsEmpty(local))
        return CGImageRetain(base);
    CGFloat x0 = std::floor(CGRectGetMinX(local) * scale) / scale, y0 = std::floor(CGRectGetMinY(local) * scale) / scale;
    CGFloat x1 = std::ceil(CGRectGetMaxX(local) * scale) / scale, y1 = std::ceil(CGRectGetMaxY(local) * scale) / scale;
    CGRect  aligned = CGRectMake(x0, y0, x1 - x0, y1 - y0);

    CGContextRef ctx = [self createContextWithWidth:pw height:ph];
    if(!ctx)
        return nullptr;
    if(!CopyPixels(base, ctx))
    {
        CGContextRelease(ctx);
        return [self copyImageLockedWithRect:rect scale:scale];
    }
    CGContextTranslateCTM(ctx, 0, ph);
    CGContextScaleCTM(ctx, scale, -scale);
    CGContextSetAllowsFontSmoothing(ctx, false);
    CGContextSetShouldAntialias(ctx, true);
    CGContextClipToRect(ctx, aligned);
    CGContextClearRect(ctx, aligned);
    [self drawLockedInContext:ctx origin:CGPointMake(-rect.origin.x, -rect.origin.y) clip:rect];
    CGImageRef image = CGBitmapContextCreateImage(ctx);
    CGContextRelease(ctx);
    return image;
}

- (CGContextRef)createContextWithWidth:(size_t)pw height:(size_t)ph CF_RETURNS_RETAINED
{
    CGColorSpaceRef space = _colorSpace ? CGColorSpaceRetain(_colorSpace) : CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef    ctx   = CGBitmapContextCreate(nullptr, pw, ph, 8, 0, space,
                                                  kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    CGColorSpaceRelease(space);
    return ctx;
}

- (nullable NSString *)attribute:(NSString *)name atPoint:(CGPoint)p
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    if(!_doc || !_doc->root_render())
        return nil;
    const lh::pixel_t x  = static_cast<float>(p.x), y = static_cast<float>(p.y);
    const char*       key = name.UTF8String;
    for(lh::element::ptr el = _doc->root_render()->get_element_by_point(x, y, x, y, nullptr); el; el = el->parent())
    {
        if(const char* value = el->get_attr(key))
            return @(value);
    }
    return nil;
}

- (NSDictionary<NSString *, NSDictionary<NSString *, NSString *> *> *)declarations:(NSArray<NSString *> *)names byAttribute:(NSString *)attribute
{
    return [self declarations:names byAttribute:attribute requiringAny:@[]];
}

- (NSDictionary<NSString *, NSDictionary<NSString *, NSString *> *> *)declarations:(NSArray<NSString *> *)names byAttribute:(NSString *)attribute requiringAny:(NSArray<NSString *> *)required
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    NSMutableDictionary *result = [NSMutableDictionary dictionary];
    if(!_doc || !_doc->root())
        return result;
    NSArray *cacheKey = @[names, attribute, required];
    if(_declarations && _declarationsEpoch == _doc->style_epoch() && [_declarationsKey isEqualToArray:cacheKey] && !getenv("LH_NO_DECLARATION_CACHE"))
        return _declarations;
    std::vector<std::pair<lh::string_id, NSString *>> keys;
    for(NSString *name in names)
    {
        const lh::string_id raw = lh::raw_declaration_id(lh::_id(name.UTF8String ?: ""));
        if(raw != lh::empty_id)
            keys.emplace_back(raw, name);
    }
    std::vector<lh::string_id> gates;
    for(NSString *name in required)
    {
        const lh::string_id raw = lh::raw_declaration_id(lh::_id(name.UTF8String ?: ""));
        if(raw != lh::empty_id)
            gates.push_back(raw);
    }
    if(required.count > 0 && gates.empty())
        return result;
    const char *key = attribute.UTF8String;
    std::vector<const lh::element *> pending{_doc->root().get()};
    while(!pending.empty())
    {
        const lh::element *el = pending.back();
        pending.pop_back();
        if(el->css().get_display() == lh::display_none)
            continue;
        if(auto tag = dynamic_cast<const lh::html_tag *>(el))
        {
            bool open = gates.empty();
            for(size_t i = 0; !open && i < gates.size(); i++)
            {
                const lh::css_token_vector *tokens = tag->get_token_property(gates[i]);
                open = tokens && !lh::get_repr(*tokens, 0, -1, true).empty();
            }
            const char *value = open ? tag->get_attr(key) : nullptr;
            if(value)
            {
                NSMutableDictionary<NSString *, NSString *> *found = nil;
                for(const auto &entry : keys)
                {
                    const lh::css_token_vector *tokens = tag->get_token_property(entry.first);
                    if(!tokens)
                        continue;
                    std::string raw = lh::get_repr(*tokens, 0, -1, true);
                    if(raw.empty())
                        continue;
                    if(!found)
                        found = [NSMutableDictionary dictionary];
                    found[entry.second] = @(raw.c_str());
                }
                if(found)
                    result[@(value)] = found;
            }
        }
        const auto &children = el->children();
        for(auto it = children.rbegin(); it != children.rend(); ++it)
            pending.push_back(it->get());
    }
    _declarations      = [result copy];
    _declarationsKey   = cacheKey;
    _declarationsEpoch = _doc->style_epoch();
    return _declarations;
}

- (NSDictionary<NSString *, NSString *> *)computedStyleForAttribute:(NSString *)attribute value:(NSString *)value
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    if(!_doc || !_doc->root_render())
        return @{};
    const char                          *key    = attribute.UTF8String;
    const std::string                    wanted = value.UTF8String;
    std::shared_ptr<lh::render_item>     found;
    std::function<void(const std::shared_ptr<lh::render_item> &)> visit = [&](const std::shared_ptr<lh::render_item> &ri) {
        if(found)
            return;
        const char *v = ri->src_el()->get_attr(key);
        if(v && wanted == v)
        {
            found = ri;
            return;
        }
        for(const auto &child : ri->children())
            visit(child);
    };
    visit(_doc->root_render());
    lh::element::ptr el = found ? found->src_el() : nullptr;
    if(!el)
    {
        std::function<lh::element::ptr(const lh::element::ptr &)> search = [&](const lh::element::ptr &e) -> lh::element::ptr {
            const char *v = e->get_attr(key);
            if(v && wanted == v)
                return e;
            for(const auto &child : e->children())
                if(auto hit = search(child))
                    return hit;
            return nullptr;
        };
        el = _doc->root() ? search(_doc->root()) : nullptr;
    }
    if(!el)
        return @{};
    const lh::css_properties &css = el->css();
    NSMutableDictionary<NSString *, NSString *> *out = [NSMutableDictionary dictionary];
    auto px = [](double v) { return [NSString stringWithFormat:@"%gpx", v]; };
    auto length = [&](const lh::css_length &l) -> NSString * {
        if(l.is_predefined())
            return @"none";
        switch(l.units())
        {
        case lh::css_units_percentage:
            return [NSString stringWithFormat:@"%g%%", l.val()];
        default:
            return px(l.val());
        }
    };
    if(found)
    {
        out[@"padding-top"]         = px(found->padding_top().value());
        out[@"padding-right"]       = px(found->padding_right().value());
        out[@"padding-bottom"]      = px(found->padding_bottom().value());
        out[@"padding-left"]        = px(found->padding_left().value());
        out[@"border-top-width"]    = px(found->border_top().value());
        out[@"border-right-width"]  = px(found->border_right().value());
        out[@"border-bottom-width"] = px(found->border_bottom().value());
        out[@"border-left-width"]   = px(found->border_left().value());
        out[@"margin-top"]          = px(found->margin_top().value());
        out[@"margin-right"]        = px(found->margin_right().value());
        out[@"margin-bottom"]       = px(found->margin_bottom().value());
        out[@"margin-left"]         = px(found->margin_left().value());
        const lh::position p        = found->get_placement();
        out[@"width"]               = px(p.width.value());
        out[@"height"]              = px(p.height.value());
    } else
    {
        for(NSString *name in @[@"padding-top", @"padding-right", @"padding-bottom", @"padding-left", @"border-top-width", @"border-right-width",
                                @"border-bottom-width", @"border-left-width", @"margin-top", @"margin-right", @"margin-bottom", @"margin-left", @"width", @"height"])
            out[name] = @"0px";
    }
    out[@"max-width"]   = length(css.get_max_width());
    out[@"max-height"]  = length(css.get_max_height());
    out[@"box-sizing"]  = css.get_box_sizing() == lh::box_sizing_border_box ? @"border-box" : @"content-box";
    out[@"display"]     = @(std::string(lh::style_display_strings[css.get_display()]).c_str());
    out[@"position"]    = @(std::string(lh::element_position_strings[css.get_position()]).c_str());
    out[@"font-size"]   = px(css.get_font_size().value());
    out[@"font-weight"] = [NSString stringWithFormat:@"%d", (int)css.font_weight().val()];
    out[@"font-family"] = @(css.font_family().c_str());
    out[@"font-style"]  = css.font_style_value() == lh::font_style_italic ? @"italic" : @"normal";
    const lh::web_color c = css.get_color();
    out[@"color"]       = c.alpha == 255 ? [NSString stringWithFormat:@"rgb(%d, %d, %d)", c.red, c.green, c.blue]
                                         : [NSString stringWithFormat:@"rgba(%d, %d, %d, %g)", c.red, c.green, c.blue, c.alpha / 255.0];
    out[@"opacity"]     = [NSString stringWithFormat:@"%g", css.get_opacity()];
    out[@"visibility"]  = css.get_visibility() == lh::visibility_visible ? @"visible" : @"hidden";
    return out;
}

- (NSDictionary<NSString *, NSValue *> *)boundsByAttribute:(NSString *)name
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    NSMutableDictionary<NSString *, NSValue *> *result = [NSMutableDictionary dictionary];
    if(!_doc || !_doc->root_render())
        return result;
    const char *key = name.UTF8String;
    std::unordered_map<std::string, CGRect> boxes;
    std::function<void(const std::shared_ptr<lh::render_item> &)> visit = [&](const std::shared_ptr<lh::render_item> &ri) {
        if(const char *value = ri->src_el()->get_attr(key))
        {
            const lh::position p      = ri->get_placement();
            const CGFloat      left   = ri->padding_left().value() + ri->border_left().value();
            const CGFloat      top    = ri->padding_top().value() + ri->border_top().value();
            const CGFloat      right  = ri->padding_right().value() + ri->border_right().value();
            const CGFloat      bottom = ri->padding_bottom().value() + ri->border_bottom().value();
            const CGRect       box    = CGRectMake(p.x.value() - left, p.y.value() - top, p.width.value() + left + right, p.height.value() + top + bottom);
            auto               it     = boxes.find(value);
            if(it == boxes.end())
                boxes.emplace(value, box);
            else
                it->second = CGRectUnion(it->second, box);
        }
        for(const auto &child : ri->children())
            visit(child);
    };
    visit(_doc->root_render());
    for(const auto &entry : boxes)
        result[@(entry.first.c_str())] = [NSValue valueWithRect:NSRectFromCGRect(entry.second)];
    return result;
}

- (NSString *)layoutSignature
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    std::string out;
    if(!_doc || !_doc->root_render())
        return @"";
    char line[256];
    std::function<void(const std::shared_ptr<lh::render_item> &, int)> visit = [&](const std::shared_ptr<lh::render_item> &ri, int depth) {
        const auto         el = ri->src_el();
        const lh::position p  = ri->get_placement();
        if(p.width.value() == 0 && p.height.value() == 0)
        {
            for(const auto &child : ri->children())
                visit(child, depth + 1);
            return;
        }
        if(el->is_text())
        {
            std::string text;
            el->get_text(text);
            out += "#" + text;
        } else
            out += el->get_tagName();
        if(ri->skip())
        {
            out += " skipped\n";
            for(const auto &child : ri->children())
                visit(child, depth + 1);
            return;
        }
        snprintf(line, sizeof(line), " %d %.3f %.3f %.3f %.3f m%.3f,%.3f,%.3f,%.3f p%.3f,%.3f,%.3f,%.3f b%.3f,%.3f,%.3f,%.3f %d\n", depth,
                 p.x.value(), p.y.value(), p.width.value(), p.height.value(), ri->margin_left().value(), ri->margin_top().value(),
                 ri->margin_right().value(), ri->margin_bottom().value(), ri->padding_left().value(), ri->padding_top().value(),
                 ri->padding_right().value(), ri->padding_bottom().value(), ri->border_left().value(), ri->border_top().value(),
                 ri->border_right().value(), ri->border_bottom().value(), ri->skip() ? 1 : 0);
        out += line;
        for(const auto &child : ri->children())
            visit(child, depth + 1);
    };
    visit(_doc->root_render(), 0);
    return [[NSString alloc] initWithBytes:out.data() length:out.size() encoding:NSUTF8StringEncoding] ?: @"";
}

- (NSDictionary<NSString *, NSString *> *)parentsByAttribute:(NSString *)name
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    NSMutableDictionary<NSString *, NSString *> *result = [NSMutableDictionary dictionary];
    if(!_doc || !_doc->root())
        return result;
    const char *key = name.UTF8String;
    std::function<void(const lh::element::ptr &, const char *)> visit = [&](const lh::element::ptr &el, const char *parent) {
        const char *value = el->get_attr(key);
        if(value && parent)
            result[@(value)] = @(parent);
        for(const auto &child : el->children())
            visit(child, value ? value : parent);
    };
    visit(_doc->root(), nullptr);
    return result;
}

- (CGRect)boundsOfElementsMatching:(NSString *)selector
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    if(!_doc || !_doc->root() || !_doc->root_render())
        return CGRectNull;
    std::unordered_set<const lh::element *> wanted;
    for(const auto &el : _doc->root()->select_all(std::string(selector.UTF8String)))
        wanted.insert(el.get());
    CGRect result = CGRectNull;
    if(wanted.empty())
        return result;
    std::function<void(const std::shared_ptr<lh::render_item> &)> visit = [&](const std::shared_ptr<lh::render_item> &ri) {
        if(wanted.count(ri->src_el().get()))
        {
            const lh::position p      = ri->get_placement();
            const CGFloat      left   = ri->padding_left().value() + ri->border_left().value();
            const CGFloat      top    = ri->padding_top().value() + ri->border_top().value();
            const CGFloat      right  = ri->padding_right().value() + ri->border_right().value();
            const CGFloat      bottom = ri->padding_bottom().value() + ri->border_bottom().value();
            const CGRect       box    = CGRectMake(p.x.value() - left, p.y.value() - top, p.width.value() + left + right, p.height.value() + top + bottom);
            if(box.size.width > 0 && box.size.height > 0)
                result = CGRectUnion(result, box);
        }
        for(const auto &child : ri->children())
            visit(child);
    };
    visit(_doc->root_render());
    return result;
}

- (void)recordLockedSize:(CGSize)size ops:(std::vector<LHDrawOp> &)ops recorder:(lhx::RenderRecorder &)recorder
{
    CGContextRef ctx = _doc ? [self createContextWithWidth:1 height:1] : nullptr;
    if(!ctx)
        return;
    _container->set_draw_ops(&ops);
    _container->set_gpu_recorder(&recorder);
    [self drawLockedInContext:ctx origin:CGPointZero clip:CGRectMake(0, 0, size.width, std::max<CGFloat>(size.height, 1))];
    _container->set_gpu_recorder(nullptr);
    _container->set_draw_ops(nullptr);
    CGContextRelease(ctx);
}

- (LHDisplayList *)displayListWithMaxWidth:(CGFloat)maxWidth gpu:(LHGPURenderer *)renderer scale:(CGFloat)scale
{
    std::lock_guard<std::mutex> guard(_lock);
    CGSize                size = [self layoutLockedWithMaxWidth:maxWidth];
    std::vector<LHDrawOp> ops;
    LHGPURecorder         recorder;
    CGColorSpaceRef       space = _colorSpace ? CGColorSpaceRetain(_colorSpace) : CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    [renderer beginRecordingWithScale:scale space:space recorder:&recorder];
    [self recordLockedSize:size ops:ops recorder:recorder];
    CGColorSpaceRelease(space);
    LHDisplayList *list = [[LHDisplayList alloc] initWithOps:std::move(ops)];
    [list setGPUOps:std::move(recorder.ops) unsupported:recorder.unsupported];
    return list;
}

- (LHDisplayList *)displayListWithMaxWidth:(CGFloat)maxWidth
{
    return [self displayListWithMaxWidth:maxWidth scale:1];
}

- (LHDisplayList *)displayListWithMaxWidth:(CGFloat)maxWidth scale:(CGFloat)scale
{
    std::lock_guard<std::mutex> guard(_lock);
    CGSize                size = [self layoutLockedWithMaxWidth:maxWidth];
    std::vector<LHDrawOp> ops;
    CGContextRef          ctx = _doc ? [self createContextWithWidth:1 height:1] : nullptr;
    if(ctx)
    {
        if(scale > 0 && std::isfinite(scale))
            CGContextScaleCTM(ctx, scale, scale);
        _container->set_draw_ops(&ops);
        [self drawLockedInContext:ctx origin:CGPointZero clip:CGRectMake(0, 0, size.width, std::max<CGFloat>(size.height, 1))];
        _container->set_draw_ops(nullptr);
        CGContextRelease(ctx);
    }
    return [[LHDisplayList alloc] initWithOps:std::move(ops)];
}

- (CGImageRef)copyImageWithMaxWidth:(CGFloat)maxWidth rect:(CGRect)rect scale:(CGFloat)scale
{
    std::lock_guard<std::mutex> guard(_lock);
    [self layoutLockedWithMaxWidth:maxWidth];
    return [self copyImageLockedWithRect:rect scale:scale];
}

- (CGImageRef)copyImageWithMaxWidth:(CGFloat)maxWidth rect:(CGRect)rect scale:(CGFloat)scale excluding:(NSSet<NSString *> *)values attribute:(NSString *)attribute
{
    std::lock_guard<std::mutex> guard(_lock);
    [self layoutLockedWithMaxWidth:maxWidth];
    NSSet<NSString *> *savedValues    = _exclusions;
    NSString          *savedAttribute = _exclusionAttribute;
    NSMutableSet<NSString *> *merged  = [NSMutableSet setWithSet:values];
    if(savedValues.count > 0 && [savedAttribute isEqualToString:attribute])
        [merged unionSet:savedValues];
    _exclusions         = merged;
    _exclusionAttribute = [attribute copy];
    CGImageRef image    = [self copyImageLockedWithRect:rect scale:scale];
    _exclusions         = savedValues;
    _exclusionAttribute = savedAttribute;
    return image;
}

- (CGImageRef)copyImageLockedWithRect:(CGRect)rect scale:(CGFloat)scale CF_RETURNS_RETAINED
{
    size_t pw = (size_t)std::ceil(rect.size.width * scale);
    size_t ph = (size_t)std::ceil(rect.size.height * scale);
    if(!_doc || pw == 0 || ph == 0)
        return nullptr;
    CGContextRef ctx = [self createContextWithWidth:pw height:ph];
    if(!ctx)
        return nullptr;
    CGContextTranslateCTM(ctx, 0, ph);
    CGContextScaleCTM(ctx, scale, -scale);
    CGContextSetAllowsFontSmoothing(ctx, false);
    CGContextSetShouldAntialias(ctx, true);
    [self drawLockedInContext:ctx origin:CGPointMake(-rect.origin.x, -rect.origin.y) clip:rect];
    CGImageRef image = CGBitmapContextCreateImage(ctx);
    CGContextRelease(ctx);
    return image;
}

- (NSArray<LHTextFragment *> *)textFragmentsWithMaxWidth:(CGFloat)maxWidth
{
    std::lock_guard<std::mutex> guard(_lock);
    CGSize size = [self layoutLockedWithMaxWidth:maxWidth];
    if(!_doc)
        return @[];
    CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef    ctx   = CGBitmapContextCreate(nullptr, 1, 1, 8, 0, space,
                                                  kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    CGColorSpaceRelease(space);
    if(!ctx)
        return @[];
    std::vector<LHTextRun> runs;
    _container->set_text_runs(&runs);
    [self drawLockedInContext:ctx origin:CGPointZero clip:CGRectMake(0, 0, size.width, std::max<CGFloat>(size.height, 1))];
    _container->set_text_runs(nullptr);
    CGContextRelease(ctx);

    NSMutableArray<LHTextFragment *> *fragments = [NSMutableArray arrayWithCapacity:runs.size()];
    for(const auto &run : runs)
    {
        NSString *text = @(run.text.c_str());
        if(text.length == 0 || !run.font)
            continue;
        [fragments addObject:[[LHTextFragment alloc] initWithText:text font:run.font frame:run.frame]];
    }
    return fragments;
}

- (BOOL)mouseMoved:(CGPoint)p
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    float x = static_cast<float>(p.x), y = static_cast<float>(p.y);
    _paintTraceValid = NO;
    const BOOL changed = _doc && _doc->on_mouse_over(x, y, x, y, IgnoreRedraw);
    _pointerRestyled = _pointerRestyled || changed;
    return changed;
}

- (BOOL)mouseDown:(CGPoint)p
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    float x = static_cast<float>(p.x), y = static_cast<float>(p.y);
    _paintTraceValid = NO;
    const BOOL changed = _doc && _doc->on_lbutton_down(x, y, x, y, IgnoreRedraw);
    _pointerRestyled = _pointerRestyled || changed;
    return changed;
}

- (BOOL)mouseUp:(CGPoint)p
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    float x = static_cast<float>(p.x), y = static_cast<float>(p.y);
    _paintTraceValid = NO;
    const BOOL changed = _doc && _doc->on_lbutton_up(x, y, x, y, IgnoreRedraw);
    _pointerRestyled = _pointerRestyled || changed;
    return changed;
}

- (BOOL)mouseExited
{
    std::lock_guard<std::mutex> guard(_lock);
    [self restoreLocked];
    _paintTraceValid = NO;
    const BOOL changed = _doc && _doc->on_mouse_leave(IgnoreRedraw);
    _pointerRestyled = _pointerRestyled || changed;
    return changed;
}

- (NSDictionary<NSString *, NSNumber *> *)sanitizerCounts
{
    std::lock_guard<std::mutex> guard(_lock);
    NSMutableDictionary<NSString *, NSNumber *> *counts = [NSMutableDictionary dictionary];
    if(_container)
    {
        for(const auto &entry : _container->sanitizer_report())
            counts[@(entry.first.c_str())] = @(entry.second.first);
    }
    return counts;
}

- (NSArray<NSString *> *)sanitizerDetails
{
    std::lock_guard<std::mutex> guard(_lock);
    NSMutableArray<NSString *> *details = [NSMutableArray array];
    if(_container)
    {
        for(const auto &entry : _container->sanitizer_report())
        {
            for(const auto &detail : entry.second.second)
                [details addObject:[NSString stringWithFormat:@"%s: %s", entry.first.c_str(), detail.c_str()]];
        }
    }
    return details;
}

@end

@implementation LHDocument (Painting)

- (CGSize)paintWithMaxWidth:(CGFloat)maxWidth clip:(CGRect)clip painter:(lhx::Painter &)painter
{
    std::lock_guard<std::mutex> guard(_lock);
    CGSize       size = [self layoutLockedWithMaxWidth:maxWidth];
    CGContextRef ctx  = _doc ? [self createContextWithWidth:1 height:1] : nullptr;
    if(ctx)
    {
        _container->set_painter(&painter);
        [self drawLockedInContext:ctx origin:CGPointZero clip:clip];
        _container->set_painter(nullptr);
        CGContextRelease(ctx);
    }
    return size;
}

- (CGSize)recordWithMaxWidth:(CGFloat)maxWidth recorder:(lhx::RenderRecorder &)recorder
{
    std::lock_guard<std::mutex> guard(_lock);
    CGSize                size  = [self layoutLockedWithMaxWidth:maxWidth];
    CGColorSpaceRef       given = recorder.space;
    std::vector<LHDrawOp> ops;
    if(!given)
        recorder.space = _colorSpace ? CGColorSpaceRetain(_colorSpace) : CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    [self recordLockedSize:size ops:ops recorder:recorder];
    if(!given)
    {
        CGColorSpaceRelease(recorder.space);
        recorder.space = nullptr;
    }
    return size;
}

@end

static std::string::size_type FindCaseless(const std::string &text, const char *needle, bool last)
{
    const size_t n = std::strlen(needle);
    if(text.size() < n)
        return std::string::npos;
    auto matches = [&](size_t at) {
        for(size_t i = 0; i < n; i++)
            if(std::tolower(static_cast<unsigned char>(text[at + i])) != needle[i])
                return false;
        return true;
    };
    if(last)
    {
        for(size_t at = text.size() - n + 1; at-- > 0;)
            if(matches(at))
                return at;
        return std::string::npos;
    }
    for(size_t at = 0; at + n <= text.size(); at++)
        if(matches(at))
            return at;
    return std::string::npos;
}

static thread_local int SVGImageDepth = 0;

bool LHRenderingSVGImage(void)
{
    return SVGImageDepth > 0;
}

CGImageRef LHCreateSVGImage(const std::string &markup, CGSize *size)
{
    int         &depth      = SVGImageDepth;
    const size_t kMaxMarkup = 2 * 1024 * 1024;
    if(depth > 0 || markup.size() > kMaxMarkup)
        return nullptr;
    size_t open  = FindCaseless(markup, "<svg", false);
    size_t close = FindCaseless(markup, "</svg>", true);
    if(open == std::string::npos || close == std::string::npos || close < open)
        return nullptr;
    std::string body = "<body style=\"margin: 0; color: #000000\">" + markup.substr(open, close + 6 - open) + "</body>";
    NSString   *html = [[NSString alloc] initWithBytes:body.data() length:body.size() encoding:NSUTF8StringEncoding];
    if(!html)
        return nullptr;
    depth++;
    CGImageRef image = nullptr;
    @autoreleasepool
    {
        LHDocument *doc = [[LHDocument alloc] initWithHTML:html css:@"body > :not(svg) { display: none !important; }"];
        [doc layoutWithMaxWidth:300 preferredWidth:300];
        CGRect box = [doc boundsOfElementsMatching:@"body > svg"];
        if(!CGRectIsNull(box) && box.size.width > 0 && box.size.height > 0 && box.size.width <= 16384 && box.size.height <= 16384)
        {
            CGFloat scale = std::min<CGFloat>(2, std::min(4096 / box.size.width, 4096 / box.size.height));
            image         = [doc copyImageWithMaxWidth:300 rect:box scale:scale];
            if(image && size)
                *size = box.size;
        }
    }
    depth--;
    return image;
}
