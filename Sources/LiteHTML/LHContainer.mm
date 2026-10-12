#import <Cocoa/Cocoa.h>
#import <ImageIO/ImageIO.h>

#include "LHContainer.h"

#include <algorithm>
#include <cmath>
#include <list>
#include <memory>
#include <mutex>
#include <string>
#include <utility>

namespace lh = litehtml;
using lh::operator""_px;

static inline CGFloat F(lh::pixel_t v) { return static_cast<CGFloat>(v.value()); }
static inline CGRect  R(const lh::position& p) { return CGRectMake(F(p.x), F(p.y), F(p.width), F(p.height)); }

static CGColorSpaceRef SRGB()
{
    static CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    return space;
}

static CGColorRef CreateColor(const lh::web_color& c)
{
    const CGFloat comps[4] = {c.red / 255.0, c.green / 255.0, c.blue / 255.0, c.alpha / 255.0};
    return CGColorCreate(SRGB(), comps);
}

static void SetFill(CGContextRef ctx, const lh::web_color& c)
{
    CGColorRef col = CreateColor(c);
    CGContextSetFillColorWithColor(ctx, col);
    CGColorRelease(col);
}

static void SetStroke(CGContextRef ctx, const lh::web_color& c)
{
    CGColorRef col = CreateColor(c);
    CGContextSetStrokeColorWithColor(ctx, col);
    CGColorRelease(col);
}

static void AddRoundedRect(CGMutablePathRef path, CGRect r, const lh::border_radiuses& rad)
{
    if(r.size.width <= 0 || r.size.height <= 0)
        return;
    const CGFloat k = 0.5522847498;
    const CGFloat hw = r.size.width / 2, hh = r.size.height / 2;
    auto clampx = [&](lh::pixel_t v) { return std::min<CGFloat>(std::max<CGFloat>(F(v), 0), hw); };
    auto clampy = [&](lh::pixel_t v) { return std::min<CGFloat>(std::max<CGFloat>(F(v), 0), hh); };
    CGFloat tlx = clampx(rad.top_left_x), tly = clampy(rad.top_left_y);
    CGFloat trx = clampx(rad.top_right_x), try_ = clampy(rad.top_right_y);
    CGFloat brx = clampx(rad.bottom_right_x), bry = clampy(rad.bottom_right_y);
    CGFloat blx = clampx(rad.bottom_left_x), bly = clampy(rad.bottom_left_y);
    CGFloat x0 = CGRectGetMinX(r), y0 = CGRectGetMinY(r), x1 = CGRectGetMaxX(r), y1 = CGRectGetMaxY(r);

    CGPathMoveToPoint(path, nullptr, x0 + tlx, y0);
    CGPathAddLineToPoint(path, nullptr, x1 - trx, y0);
    if(trx > 0 && try_ > 0)
        CGPathAddCurveToPoint(path, nullptr, x1 - trx + trx * k, y0, x1, y0 + try_ - try_ * k, x1, y0 + try_);
    CGPathAddLineToPoint(path, nullptr, x1, y1 - bry);
    if(brx > 0 && bry > 0)
        CGPathAddCurveToPoint(path, nullptr, x1, y1 - bry + bry * k, x1 - brx + brx * k, y1, x1 - brx, y1);
    CGPathAddLineToPoint(path, nullptr, x0 + blx, y1);
    if(blx > 0 && bly > 0)
        CGPathAddCurveToPoint(path, nullptr, x0 + blx - blx * k, y1, x0, y1 - bly + bly * k, x0, y1 - bly);
    CGPathAddLineToPoint(path, nullptr, x0, y0 + tly);
    if(tlx > 0 && tly > 0)
        CGPathAddCurveToPoint(path, nullptr, x0, y0 + tly - tly * k, x0 + tlx - tlx * k, y0, x0 + tlx, y0);
    CGPathCloseSubpath(path);
}

static lh::margins Uniform(CGFloat v)
{
    lh::margins m;
    m.left = m.right = m.top = m.bottom = static_cast<float>(v);
    return m;
}

static bool HasRadius(const lh::border_radiuses& r)
{
    return F(r.top_left_x) > 0 || F(r.top_right_x) > 0 || F(r.bottom_left_x) > 0 || F(r.bottom_right_x) > 0;
}

static bool InsideRoundedRect(CGRect r, const lh::border_radiuses& rad, CGRect b)
{
    if(!CGRectContainsRect(r, b))
        return false;
    const CGFloat hw = r.size.width / 2, hh = r.size.height / 2;
    auto clampx = [&](lh::pixel_t v) { return std::min<CGFloat>(std::max<CGFloat>(F(v), 0), hw); };
    auto clampy = [&](lh::pixel_t v) { return std::min<CGFloat>(std::max<CGFloat>(F(v), 0), hh); };
    CGFloat x0 = CGRectGetMinX(r), y0 = CGRectGetMinY(r), x1 = CGRectGetMaxX(r), y1 = CGRectGetMaxY(r);
    const CGRect corners[4] = {
        CGRectMake(x0, y0, clampx(rad.top_left_x), clampy(rad.top_left_y)),
        CGRectMake(x1 - clampx(rad.top_right_x), y0, clampx(rad.top_right_x), clampy(rad.top_right_y)),
        CGRectMake(x1 - clampx(rad.bottom_right_x), y1 - clampy(rad.bottom_right_y), clampx(rad.bottom_right_x), clampy(rad.bottom_right_y)),
        CGRectMake(x0, y1 - clampy(rad.bottom_left_y), clampx(rad.bottom_left_x), clampy(rad.bottom_left_y)),
    };
    for(const CGRect& corner : corners)
        if(corner.size.width > 0 && corner.size.height > 0 && CGRectIntersectsRect(corner, b))
            return false;
    return true;
}

static void ClipToRoundedRect(CGContextRef ctx, CGRect r, const lh::border_radiuses& rad)
{
    if(!HasRadius(rad))
    {
        CGContextClipToRect(ctx, r);
        return;
    }
    CGMutablePathRef p = CGPathCreateMutable();
    AddRoundedRect(p, r, rad);
    CGContextAddPath(ctx, p);
    CGContextClip(ctx);
    CGPathRelease(p);
}

namespace
{
struct ImageHandle
{
    CGImageRef image = nullptr;
    CGSize     size  = CGSizeZero;

    ImageHandle() = default;
    ImageHandle(CGImageRef i, CGSize s) : image(i ? CGImageRetain(i) : nullptr), size(s) {}
    ImageHandle(const ImageHandle& o) : ImageHandle(o.image, o.size) {}
    ImageHandle& operator=(const ImageHandle& o)
    {
        if(this != &o)
        {
            CGImageRef old = image;
            image          = o.image ? CGImageRetain(o.image) : nullptr;
            size           = o.size;
            if(old)
                CGImageRelease(old);
        }
        return *this;
    }
    ~ImageHandle()
    {
        if(image)
            CGImageRelease(image);
    }
    explicit operator bool() const { return image != nullptr; }
};

struct ImageStore
{
    std::mutex                                   lock;
    std::unordered_map<std::string, ImageHandle> images;
    std::list<std::string>                       dataOrder;
    std::unordered_map<std::string, std::pair<std::list<std::string>::iterator, size_t>> dataMeta;
    size_t                                       dataBytes = 0;
};

ImageStore& Store()
{
    static ImageStore* store = new ImageStore();
    return *store;
}

const size_t kMaxDataImageBytes = 24 * 1024 * 1024;

CGImageRef CreateCGImageFromNSImage(NSImage* image, CGSize logical, CGFloat scale, NSColor* tint)
{
    if(!std::isfinite(logical.width) || !std::isfinite(logical.height) || logical.width <= 0 || logical.height <= 0)
        return nullptr;
    size_t pw = static_cast<size_t>(std::ceil(logical.width * scale));
    size_t ph = static_cast<size_t>(std::ceil(logical.height * scale));
    if(pw == 0 || ph == 0 || pw > 4096 || ph > 4096)
        return nullptr;
    CGContextRef ctx = CGBitmapContextCreate(nullptr, pw, ph, 8, 0, SRGB(),
                                             kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
    if(!ctx)
        return nullptr;
    CGContextScaleCTM(ctx, scale, scale);
    NSGraphicsContext* gc = [NSGraphicsContext graphicsContextWithCGContext:ctx flipped:NO];
    [NSGraphicsContext saveGraphicsState];
    NSGraphicsContext.currentContext = gc;
    [image drawInRect:NSMakeRect(0, 0, logical.width, logical.height)
             fromRect:NSZeroRect
            operation:NSCompositingOperationSourceOver
             fraction:1.0];
    if(tint)
    {
        [tint setFill];
        NSRectFillUsingOperation(NSMakeRect(0, 0, logical.width, logical.height),
                                 NSCompositingOperationSourceAtop);
    }
    [NSGraphicsContext restoreGraphicsState];
    CGImageRef out = CGBitmapContextCreateImage(ctx);
    CGContextRelease(ctx);
    return out;
}

NSColor* ColorFromHex(NSString* hex)
{
    if(hex.length == 0)
        return nil;
    if([hex hasPrefix:@"#"])
        hex = [hex substringFromIndex:1];
    unsigned int v = 0;
    if(![[NSScanner scannerWithString:hex] scanHexInt:&v])
        return nil;
    if(hex.length == 8)
        return [NSColor colorWithSRGBRed:((v >> 24) & 0xff) / 255.0
                                   green:((v >> 16) & 0xff) / 255.0
                                    blue:((v >> 8) & 0xff) / 255.0
                                   alpha:(v & 0xff) / 255.0];
    return [NSColor colorWithSRGBRed:((v >> 16) & 0xff) / 255.0
                               green:((v >> 8) & 0xff) / 255.0
                                blue:(v & 0xff) / 255.0
                               alpha:1.0];
}

ImageHandle AdoptImage(CGImageRef image, CGSize size)
{
    ImageHandle out(image, size);
    if(image)
        CGImageRelease(image);
    return out;
}

ImageHandle LoadImageFromData(NSData* data, bool isSVG)
{
    if(!data)
        return {};
    if(!isSVG)
    {
        CGImageSourceRef src = CGImageSourceCreateWithData((__bridge CFDataRef)data, nullptr);
        if(src)
        {
            CGImageRef image = CGImageSourceCreateImageAtIndex(src, 0, nullptr);
            CFRelease(src);
            if(image)
                return AdoptImage(image, CGSizeMake(CGImageGetWidth(image), CGImageGetHeight(image)));
        }
        return {};
    }
    NSImage* img = [[NSImage alloc] initWithData:data];
    if(img && img.size.width > 0 && img.size.height > 0)
        return AdoptImage(CreateCGImageFromNSImage(img, img.size, 2.0, nil), img.size);
    return {};
}

ImageHandle LoadDataURI(const std::string& url)
{
    auto comma = url.find(',');
    if(comma == std::string::npos)
        return {};
    std::string meta    = url.substr(5, comma - 5);
    std::string payload = url.substr(comma + 1);
    bool        base64  = meta.find(";base64") != std::string::npos;
    bool        isSVG   = meta.find("svg") != std::string::npos;
    NSData*     data    = nil;
    NSString*   str     = [NSString stringWithUTF8String:payload.c_str()];
    if(!str)
        return {};
    if(base64)
        data = [[NSData alloc] initWithBase64EncodedString:str
                                                   options:NSDataBase64DecodingIgnoreUnknownCharacters];
    else
        data = [(str.stringByRemovingPercentEncoding ?: str) dataUsingEncoding:NSUTF8StringEncoding];
    return LoadImageFromData(data, isSVG);
}

ImageHandle LoadSymbol(const std::string& url)
{
    if(@available(macOS 11.0, *))
    {
        NSURLComponents* comps =
            [NSURLComponents componentsWithString:[@"sf://x/" stringByAppendingString:@(url.c_str() + 3)]];
        NSString*    name       = comps.path.lastPathComponent;
        CGFloat      size       = 20;
        NSColor*     tint       = [NSColor colorWithSRGBRed:0.25 green:0.25 blue:0.27 alpha:1];
        NSFontWeight weight     = NSFontWeightRegular;
        bool         multicolor = false;
        for(NSURLQueryItem* q in comps.queryItems)
        {
            if([q.name isEqualToString:@"size"])
                size = std::clamp(q.value.doubleValue, 1.0, 512.0);
            else if([q.name isEqualToString:@"color"])
                tint = ColorFromHex(q.value) ?: tint;
            else if([q.name isEqualToString:@"weight"])
                weight = [q.value isEqualToString:@"bold"]       ? NSFontWeightBold
                         : [q.value isEqualToString:@"semibold"] ? NSFontWeightSemibold
                         : [q.value isEqualToString:@"light"]    ? NSFontWeightLight
                                                                 : NSFontWeightRegular;
            else if([q.name isEqualToString:@"mode"])
                multicolor = [q.value isEqualToString:@"multicolor"];
        }
        if(name.length == 0)
            return {};
        NSImage* sym = [NSImage imageWithSystemSymbolName:name accessibilityDescription:nil];
        if(!sym)
            return {};
        NSImageSymbolConfiguration* cfg = [NSImageSymbolConfiguration configurationWithPointSize:size weight:weight];
        bool tinted = true;
        if(multicolor)
        {
            if(@available(macOS 12.0, *))
            {
                cfg    = [cfg configurationByApplyingConfiguration:[NSImageSymbolConfiguration configurationPreferringMulticolor]];
                tinted = false;
            }
        }
        sym = [sym imageWithSymbolConfiguration:cfg] ?: sym;
        return AdoptImage(CreateCGImageFromNSImage(sym, sym.size, 2.0, tinted ? tint : nil), sym.size);
    }
    return {};
}

NSImage* (^g_provider)(NSString* url) = nil;
std::mutex g_providerLock;

ImageHandle LoadProvided(const std::string& url)
{
    NSImage* (^provider)(NSString* url) = nil;
    {
        std::lock_guard<std::mutex> guard(g_providerLock);
        provider = g_provider;
    }
    NSString* string = provider ? [NSString stringWithUTF8String:url.c_str()] : nil;
    NSImage*  image  = string ? provider(string) : nil;
    if(!image)
        return {};
    if(image.representations.count == 1)
    {
        NSImageRep* rep = image.representations.firstObject;
        if([rep isKindOfClass:[NSBitmapImageRep class]] || [NSStringFromClass([rep class]) isEqualToString:@"NSCGImageSnapshotRep"])
        {
            CGImageRef direct = [image CGImageForProposedRect:nullptr context:nil hints:nil];
            if(direct && CGImageGetWidth(direct) <= 4096 && CGImageGetHeight(direct) <= 4096)
                return AdoptImage(CGImageRetain(direct), image.size);
        }
    }
    return AdoptImage(CreateCGImageFromNSImage(image, image.size, 2.0, nil), image.size);
}

ImageHandle LookupImage(const std::string& url)
{
    if(url.empty())
        return {};
    bool        isData = url.rfind("data:", 0) == 0;
    ImageStore& store  = Store();
    {
        std::lock_guard<std::mutex> guard(store.lock);
        auto                        it = store.images.find(url);
        if(it != store.images.end())
        {
            if(isData)
            {
                auto mit = store.dataMeta.find(url);
                if(mit != store.dataMeta.end())
                    store.dataOrder.splice(store.dataOrder.end(), store.dataOrder, mit->second.first);
            }
            return it->second;
        }
    }
    ImageHandle img;
    if(isData)
        img = LoadDataURI(url);
    else if(url.rfind("sf:", 0) == 0)
        img = LoadSymbol(url);
    else
    {
        img = LoadProvided(url);
        if(!img)
            return {};
    }

    std::lock_guard<std::mutex> guard(store.lock);
    if(isData)
    {
        auto existing = store.images.find(url);
        if(existing != store.images.end())
        {
            auto mit = store.dataMeta.find(url);
            if(mit != store.dataMeta.end())
                store.dataOrder.splice(store.dataOrder.end(), store.dataOrder, mit->second.first);
            return existing->second;
        }
        size_t bytes = img.image ? CGImageGetBytesPerRow(img.image) * CGImageGetHeight(img.image) : 16 * 1024;
        while(store.dataBytes + bytes > kMaxDataImageBytes && !store.dataOrder.empty())
        {
            std::string victim = store.dataOrder.front();
            auto        vit    = store.dataMeta.find(victim);
            if(vit != store.dataMeta.end())
            {
                store.dataBytes -= vit->second.second;
                store.dataMeta.erase(vit);
            }
            store.images.erase(victim);
            store.dataOrder.pop_front();
        }
        store.dataOrder.push_back(url);
        store.dataMeta[url] = {std::prev(store.dataOrder.end()), bytes};
        store.dataBytes += bytes;
    }
    store.images[url] = img;
    return img;
}
}

void LHRegisterAppImage(NSString* name, NSImage* image)
{
    ImageHandle img = AdoptImage(CreateCGImageFromNSImage(image, image.size, 2.0, nil), image.size);
    std::string key = std::string("app:") + name.UTF8String;
    ImageStore& store = Store();
    std::lock_guard<std::mutex> guard(store.lock);
    store.images[key] = img;
}

NSImage* (^LHImageProvider(void))(NSString* url)
{
    std::lock_guard<std::mutex> guard(g_providerLock);
    return g_provider;
}

void LHSetImageProvider(NSImage* (^provider)(NSString* url))
{
    std::lock_guard<std::mutex> guard(g_providerLock);
    g_provider = [provider copy];
}

void LHForgetImage(NSString* url)
{
    ImageStore& store = Store();
    std::lock_guard<std::mutex> guard(store.lock);
    store.images.erase(std::string(url.UTF8String));
}

static void DrawCGImage(CGContextRef ctx, CGImageRef img, CGRect r)
{
    CGContextSaveGState(ctx);
    CGContextTranslateCTM(ctx, r.origin.x, r.origin.y + r.size.height);
    CGContextScaleCTM(ctx, 1, -1);
    CGContextSetInterpolationQuality(ctx, kCGInterpolationHigh);
    CGContextDrawImage(ctx, CGRectMake(0, 0, r.size.width, r.size.height), img);
    CGContextRestoreGState(ctx);
}

namespace {

class RasterCache
{
  public:
    CGImageRef copy(const std::string& key)
    {
        std::lock_guard<std::mutex> lock(m_mutex);
        auto                        it = m_entries.find(key);
        if(it == m_entries.end())
            return nullptr;
        m_order.splice(m_order.begin(), m_order, it->second.position);
        return CGImageRetain(it->second.image);
    }

    void put(const std::string& key, CGImageRef image, size_t bytes)
    {
        if(bytes > kBudget / 4)
            return;
        std::lock_guard<std::mutex> lock(m_mutex);
        if(m_entries.count(key))
            return;
        m_order.push_front(key);
        m_entries.emplace(key, Entry{CGImageRetain(image), bytes, m_order.begin()});
        m_bytes += bytes;
        while(m_bytes > kBudget && !m_order.empty())
        {
            auto victim = m_entries.find(m_order.back());
            m_bytes -= victim->second.bytes;
            CGImageRelease(victim->second.image);
            m_entries.erase(victim);
            m_order.pop_back();
        }
    }

  private:
    struct Entry
    {
        CGImageRef                       image;
        size_t                           bytes;
        std::list<std::string>::iterator position;
    };
    static constexpr size_t                kBudget = 32 << 20;
    std::mutex                             m_mutex;
    std::unordered_map<std::string, Entry> m_entries;
    std::list<std::string>                 m_order;
    size_t                                 m_bytes = 0;
};

RasterCache& SharedRasterCache()
{
    static RasterCache* cache = new RasterCache();
    return *cache;
}

bool Integral(CGFloat v)
{
    return std::fabs(v - std::round(v)) < 0.001;
}

void AppendInteger(std::string& key, uint64_t v)
{
    char buf[24];
    int  n = snprintf(buf, sizeof(buf), "%llx,", (unsigned long long)v);
    key.append(buf, n);
}

void AppendNumber(std::string& key, double v)
{
    char buf[40];
    int  n = snprintf(buf, sizeof(buf), "%a,", v);
    key.append(buf, n);
}

}

// Paints `paint` (which draws in the context's user space) for the area `r` into a cached bitmap of
// exactly r's device size and copies it 1:1. CoreGraphics dithers gradients and resamples images
// relative to the current clip, so drawing them straight into a partially redrawn tile gives
// slightly different pixels than a full redraw; a pixel-aligned copy does not. Returns false when the
// transform is not axis-aligned or r is not on device pixels, and the caller draws directly.
static bool DrawRasterized(CGContextRef ctx, CGRect r, std::string key, const std::function<void(CGContextRef)>& paint, bool store = true)
{
    static const bool disabled = getenv("LH_NO_RASTER_CACHE") != nullptr;
    if(disabled)
        return false;
    CGAffineTransform m = CGContextGetCTM(ctx);
    if(std::fabs(m.b) > 1e-9 || std::fabs(m.c) > 1e-9 || r.size.width <= 0 || r.size.height <= 0)
        return false;
    CGRect device = CGContextConvertRectToDeviceSpace(ctx, r);
    if(!Integral(device.origin.x) || !Integral(device.origin.y) || !Integral(device.size.width) || !Integral(device.size.height))
        return false;
    size_t pw = (size_t)std::llround(device.size.width), ph = (size_t)std::llround(device.size.height);
    if(pw == 0 || ph == 0 || pw * ph > (4 << 20))
        return false;
    CGFloat sx = std::fabs(m.a), sy = std::fabs(m.d);
    AppendNumber(key, (double)pw);
    AppendNumber(key, (double)ph);
    AppendNumber(key, sx);
    AppendNumber(key, sy);
    AppendNumber(key, m.d < 0 ? -1 : 1);
    CGColorSpaceRef space = CGBitmapContextGetColorSpace(ctx);
    if(!space || CGColorSpaceGetModel(space) != kCGColorSpaceModelRGB)
        space = SRGB();
    AppendInteger(key, (uintptr_t)space);
    CGImageRef image = store ? SharedRasterCache().copy(key) : nullptr;
    if(!image)
    {
        CGContextRef bitmap = CGBitmapContextCreate(nullptr, pw, ph, 8, 0, space, kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
        if(!bitmap)
            return false;
        if(m.d < 0)
        {
            CGContextTranslateCTM(bitmap, 0, ph);
            CGContextScaleCTM(bitmap, sx, -sy);
        }
        else
            CGContextScaleCTM(bitmap, sx, sy);
        CGContextTranslateCTM(bitmap, -r.origin.x, -r.origin.y);
        CGContextSetAllowsFontSmoothing(bitmap, false);
        CGContextSetShouldAntialias(bitmap, true);
        paint(bitmap);
        image = CGBitmapContextCreateImage(bitmap);
        CGContextRelease(bitmap);
        if(!image)
            return false;
        if(store)
            SharedRasterCache().put(key, image, pw * ph * 4);
    }
    CGContextSaveGState(ctx);
    CGContextSetInterpolationQuality(ctx, kCGInterpolationNone);
    if(m.d < 0)
    {
        CGContextTranslateCTM(ctx, r.origin.x, r.origin.y + r.size.height);
        CGContextScaleCTM(ctx, 1, -1);
    }
    else
        CGContextTranslateCTM(ctx, r.origin.x, r.origin.y);
    CGContextDrawImage(ctx, CGRectMake(0, 0, r.size.width, r.size.height), image);
    CGContextRestoreGState(ctx);
    CGImageRelease(image);
    return true;
}

namespace {

struct SharedFont
{
    CTFontRef       font  = nullptr;
    CFDictionaryRef attrs = nullptr;
    CGFloat         ascent = 0, descent = 0, x_height = 0;
    CGFloat         underline_pos = 0, underline_thickness = 1;
    CGFloat         kern = 0;
    std::mutex                                widthsMutex;
    std::unordered_map<std::string, CGFloat> widths;
    std::mutex                                  linesMutex;
    std::unordered_map<std::string, CTLineRef> lines;

    ~SharedFont()
    {
        for(auto& entry : lines)
            CFRelease(entry.second);
        if(attrs)
            CFRelease(attrs);
        if(font)
            CFRelease(font);
    }
};

std::mutex                                                   gFontCacheMutex;
std::unordered_map<std::string, std::shared_ptr<SharedFont>> gFontCache;
constexpr size_t                                             kFontCacheLimit = 256;

}

struct LHContainer::Font
{
    std::shared_ptr<SharedFont> shared;
    CGFloat         shadow_x = 0, shadow_y = 0, shadow_blur = 0;
    lh::web_color   shadow_color = lh::web_color::transparent;
    bool            underline = false, overline = false, strike = false;
    lh::web_color   deco_color = lh::web_color::current_color;
    int             deco_style = lh::text_decoration_style_solid;
    CGFloat         deco_thickness = 0;
};

static NSFontWeight NSWeightFromCSS(int w)
{
    static const NSFontWeight table[] = {NSFontWeightUltraLight, NSFontWeightThin, NSFontWeightLight,
                                         NSFontWeightRegular, NSFontWeightMedium, NSFontWeightSemibold,
                                         NSFontWeightBold, NSFontWeightHeavy, NSFontWeightBlack};
    int idx = std::clamp((w + 50) / 100, 1, 9) - 1;
    return table[idx];
}

static NSInteger ManagerWeightFromCSS(int w)
{
    static const NSInteger table[] = {2, 3, 4, 5, 6, 8, 9, 10, 11};
    return table[std::clamp((w + 50) / 100, 1, 9) - 1];
}

static bool HasWeightAxis(NSFont* font)
{
    static std::mutex                            mutex;
    static std::unordered_map<std::string, bool> known;
    const int                                    axis = 0x77676874;
    NSString*                                    name = CFBridgingRelease(CTFontCopyPostScriptName((__bridge CTFontRef)font));
    const std::string                            key  = name.UTF8String ?: "";
    if(!key.empty())
    {
        std::lock_guard<std::mutex> lock(mutex);
        auto                        it = known.find(key);
        if(it != known.end())
            return it->second;
    }
    NSArray* axes      = CFBridgingRelease(CTFontCopyVariationAxes((__bridge CTFontRef)font));
    bool     hasWeight = false;
    for(NSDictionary* item in axes)
        hasWeight = hasWeight || [item[(__bridge NSString*)kCTFontVariationAxisIdentifierKey] intValue] == axis;
    if(!key.empty())
    {
        std::lock_guard<std::mutex> lock(mutex);
        if(known.size() > 512)
            known.clear();
        known.emplace(key, hasWeight);
    }
    return hasWeight;
}

static NSFont* WithCSSWeight(NSFont* font, int cssWeight)
{
    if(!font)
        return font;
    const int axis = 0x77676874;
    if(!HasWeightAxis(font))
        return font;
    CTFontDescriptorRef varied = CTFontDescriptorCreateCopyWithVariation((__bridge CTFontDescriptorRef)font.fontDescriptor,
                                                                         (__bridge CFNumberRef) @(axis), std::clamp(cssWeight, 1, 1000));
    CTFontRef result = CTFontCreateWithFontDescriptor(varied, font.pointSize, nullptr);
    CFRelease(varied);
    return result ? (NSFont*)CFBridgingRelease(result) : font;
}

static NSFont* SystemFontWithDesign(CGFloat size, NSFontWeight weight, bool serif)
{
    NSFont* base = [NSFont systemFontOfSize:size weight:weight];
    if(@available(macOS 10.15, *))
    {
        NSFontDescriptor* d = [base.fontDescriptor fontDescriptorWithDesign:serif ? NSFontDescriptorSystemDesignSerif
                                                                                  : NSFontDescriptorSystemDesignRounded];
        return d ? ([NSFont fontWithDescriptor:d size:size] ?: base) : base;
    }
    if(serif)
        return [NSFont fontWithName:@"Times New Roman" size:size] ?: base;
    return base;
}

static NSFont* MonospacedFont(CGFloat size, NSFontWeight weight)
{
    if(@available(macOS 10.15, *))
        return [NSFont monospacedSystemFontOfSize:size weight:weight];
    NSFont* menlo = [NSFont fontWithName:weight >= NSFontWeightSemibold ? @"Menlo-Bold" : @"Menlo-Regular" size:size];
    return menlo ?: ([NSFont userFixedPitchFontOfSize:size] ?: [NSFont systemFontOfSize:size weight:weight]);
}

static NSFont* ResolveFont(const std::string& families, CGFloat size, int cssWeight, bool italic)
{
    NSFontWeight weight = NSWeightFromCSS(cssWeight);
    NSFont*      font   = nil;

    NSArray<NSString*>* tokens = [@(families.c_str()) componentsSeparatedByString:@","];
    NSCharacterSet*     trim   = [NSCharacterSet characterSetWithCharactersInString:@" \t\r\n\"'"];
    for(NSString* raw in tokens)
    {
        NSString* name  = [raw stringByTrimmingCharactersInSet:trim];
        NSString* lower = name.lowercaseString;
        if(name.length == 0)
            continue;

        if([@[@"-apple-system", @"system-ui", @"blinkmacsystemfont", @"ui-sans-serif",
              @"sf pro", @"sf pro text", @"sf pro display"] containsObject:lower])
            font = WithCSSWeight([NSFont systemFontOfSize:size weight:NSFontWeightRegular], cssWeight);
        else if([@[@"ui-monospace", @"sf mono"] containsObject:lower])
            font = WithCSSWeight(MonospacedFont(size, NSFontWeightRegular), cssWeight);
        else if([@[@"ui-serif", @"new york"] containsObject:lower])
            font = WithCSSWeight(SystemFontWithDesign(size, NSFontWeightRegular, true), cssWeight);
        else if([@[@"ui-rounded", @"sf pro rounded"] containsObject:lower])
            font = WithCSSWeight(SystemFontWithDesign(size, NSFontWeightRegular, false), cssWeight);
        else
        {
            NSString* family = [lower isEqualToString:@"serif"]        ? @"Times"
                               : [lower isEqualToString:@"sans-serif"] ? @"Helvetica"
                               : [lower isEqualToString:@"monospace"]  ? @"Courier"
                               : [lower isEqualToString:@"cursive"]    ? @"Apple Chancery"
                               : [lower isEqualToString:@"fantasy"]    ? @"Papyrus"
                                                                        : name;
            NSFontManager* fm = NSFontManager.sharedFontManager;
            if([fm availableMembersOfFontFamily:family].count)
            {
                NSFontTraitMask traits = (italic ? NSItalicFontMask : 0) | (cssWeight >= 600 ? NSBoldFontMask : 0);
                NSInteger       mw     = ManagerWeightFromCSS(cssWeight);
                font = [fm fontWithFamily:family traits:traits weight:mw size:size]
                           ?: [fm fontWithFamily:family traits:0 weight:mw size:size]
                           ?: [fm fontWithFamily:family traits:0 weight:5 size:size];
            }
        }
        if(font)
            break;
    }
    if(!font)
        font = WithCSSWeight([NSFont systemFontOfSize:size weight:NSFontWeightRegular], cssWeight);

    if(italic && !(font.fontDescriptor.symbolicTraits & NSFontDescriptorTraitItalic))
    {
        NSFontDescriptor* d =
            [font.fontDescriptor fontDescriptorWithSymbolicTraits:font.fontDescriptor.symbolicTraits |
                                                                  NSFontDescriptorTraitItalic];
        NSFont* it = d ? [NSFont fontWithDescriptor:d size:size] : nil;
        if(it && (it.fontDescriptor.symbolicTraits & NSFontDescriptorTraitItalic))
            font = it;
        else
        {
            NSAffineTransform*      skew = [NSAffineTransform transform];
            NSAffineTransformStruct t    = {size, 0, size * 0.2, size, 0, 0};
            skew.transformStruct         = t;
            font = [NSFont fontWithDescriptor:font.fontDescriptor textTransform:skew] ?: font;
        }
    }
    return font;
}

size_t LHContainer::cached_font_count()
{
    std::lock_guard<std::mutex> lock(gFontCacheMutex);
    return gFontCache.size();
}

LHContainer::LHContainer() = default;
LHContainer::~LHContainer() = default;

static std::shared_ptr<SharedFont> AcquireSharedFont(const lh::font_description& descr, CGFloat size)
{
    bool        italic = descr.style == lh::font_style_italic;
    std::string key    = descr.family + "|" + std::to_string(size) + "|" + std::to_string(descr.weight) + "|" +
                      (italic ? "1" : "0");
    if(descr.letter_spacing != 0)
        key += "|" + std::to_string(descr.letter_spacing);

    {
        std::lock_guard<std::mutex> lock(gFontCacheMutex);
        auto                        it = gFontCache.find(key);
        if(it != gFontCache.end())
            return it->second;
    }

    NSFont* nsfont = ResolveFont(descr.family, size, descr.weight, italic);

    auto sf            = std::make_shared<SharedFont>();
    sf->font           = (CTFontRef)CFBridgingRetain(nsfont);
    sf->kern           = descr.letter_spacing;
    CFNumberRef kern   = CFNumberCreate(nullptr, kCFNumberCGFloatType, &sf->kern);
    const void* keys[] = {kCTFontAttributeName, kCTKernAttributeName};
    const void* vals[] = {sf->font, kern};
    sf->attrs = CFDictionaryCreate(nullptr, keys, vals, sf->kern != 0 ? 2 : 1, &kCFTypeDictionaryKeyCallBacks,
                                   &kCFTypeDictionaryValueCallBacks);
    CFRelease(kern);

    CGFloat ascent          = CTFontGetAscent(sf->font);
    CGFloat descent         = CTFontGetDescent(sf->font);
    CGFloat leading         = CTFontGetLeading(sf->font);
    sf->ascent              = std::round(ascent);
    sf->descent             = std::round(descent) + std::round(leading);
    sf->x_height            = CTFontGetXHeight(sf->font);
    sf->underline_pos       = -CTFontGetUnderlinePosition(sf->font);
    sf->underline_thickness = std::max<CGFloat>(1.0, std::round(CTFontGetUnderlineThickness(sf->font)));

    std::lock_guard<std::mutex> lock(gFontCacheMutex);
    if(gFontCache.size() >= kFontCacheLimit)
    {
        for(auto it = gFontCache.begin(); it != gFontCache.end();)
            it = it->second.use_count() == 1 ? gFontCache.erase(it) : std::next(it);
    }
    return gFontCache.emplace(key, sf).first->second;
}

lh::uint_ptr LHContainer::create_font(const lh::font_description& descr, const lh::document* doc,
                                      lh::font_metrics* fm)
{
    CGFloat size = std::max<CGFloat>(F(descr.size), 1.0);

    auto* f   = new Font();
    f->shared = AcquireSharedFont(descr, size);

    f->underline  = (descr.decoration_line & lh::text_decoration_line_underline) != 0;
    f->overline   = (descr.decoration_line & lh::text_decoration_line_overline) != 0;
    f->strike     = (descr.decoration_line & lh::text_decoration_line_line_through) != 0;
    f->deco_color = descr.decoration_color;
    f->deco_style = descr.decoration_style;
    f->shadow_x     = descr.shadow_x;
    f->shadow_y     = descr.shadow_y;
    f->shadow_blur  = descr.shadow_blur;
    f->shadow_color = descr.shadow_color;

    CGFloat height = f->shared->ascent + f->shared->descent;

    auto thickness = descr.decoration_thickness;
    if(!thickness.is_predefined() && doc)
    {
        lh::font_metrics tmp;
        tmp.font_size     = descr.size;
        tmp.height        = static_cast<float>(height);
        tmp.ascent        = static_cast<float>(f->shared->ascent);
        tmp.descent       = static_cast<float>(f->shared->descent);
        tmp.x_height      = static_cast<float>(f->shared->x_height);
        tmp.ch_width      = descr.size;
        f->deco_thickness = F(doc->to_pixels(thickness, tmp, descr.size));
    }

    if(fm)
    {
        fm->font_size   = descr.size;
        fm->height      = static_cast<float>(height);
        fm->ascent      = static_cast<float>(f->shared->ascent);
        fm->descent     = static_cast<float>(f->shared->descent);
        fm->x_height    = static_cast<float>(f->shared->x_height);
        fm->draw_spaces = descr.decoration_line != lh::text_decoration_line_none;
        fm->sub_shift   = descr.size / 5;
        fm->super_shift = descr.size / 3;
        fm->ch_width    = text_width("0", reinterpret_cast<lh::uint_ptr>(f));
    }
    return reinterpret_cast<lh::uint_ptr>(f);
}

void LHContainer::delete_font(lh::uint_ptr hFont)
{
    delete reinterpret_cast<Font*>(hFont);
}

static CTLineRef CreateLine(const char* text, CFDictionaryRef attrs)
{
    CFStringRef str = CFStringCreateWithCString(nullptr, text, kCFStringEncodingUTF8);
    if(!str)
        return nullptr;
    CFAttributedStringRef as   = CFAttributedStringCreate(nullptr, str, attrs);
    CTLineRef             line = CTLineCreateWithAttributedString(as);
    CFRelease(as);
    CFRelease(str);
    return line;
}

static CTLineRef CopyColoredLine(SharedFont& sf, const char* text, const lh::web_color& color)
{
    std::string key(text);
    key.append({'\0', static_cast<char>(color.red), static_cast<char>(color.green), static_cast<char>(color.blue),
                static_cast<char>(color.alpha)});
    {
        std::lock_guard<std::mutex> lock(sf.linesMutex);
        auto                        it = sf.lines.find(key);
        if(it != sf.lines.end())
            return static_cast<CTLineRef>(CFRetain(it->second));
    }
    CGColorRef      fg     = CreateColor(color);
    CFNumberRef     kern   = CFNumberCreate(nullptr, kCFNumberCGFloatType, &sf.kern);
    const void*     keys[] = {kCTFontAttributeName, kCTForegroundColorAttributeName, kCTKernAttributeName};
    const void*     vals[] = {sf.font, fg, kern};
    CFDictionaryRef attrs  = CFDictionaryCreate(nullptr, keys, vals, sf.kern != 0 ? 3 : 2,
                                                &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    CTLineRef line = CreateLine(text, attrs);
    CFRelease(attrs);
    CFRelease(kern);
    CGColorRelease(fg);
    if(!line)
        return nullptr;
    std::lock_guard<std::mutex> lock(sf.linesMutex);
    if(sf.lines.size() >= 2048)
    {
        for(auto& entry : sf.lines)
            CFRelease(entry.second);
        sf.lines.clear();
    }
    if(sf.lines.emplace(std::move(key), line).second)
        CFRetain(line);
    return line;
}

lh::pixel_t LHContainer::text_width(const char* text, lh::uint_ptr hFont)
{
    auto* f = reinterpret_cast<Font*>(hFont);
    if(!f || !f->shared || !text || !*text)
        return 0_px;
    auto& sf = *f->shared;
    {
        std::lock_guard<std::mutex> lock(sf.widthsMutex);
        auto                        it = sf.widths.find(text);
        if(it != sf.widths.end())
            return static_cast<float>(it->second);
    }

    CGFloat   w    = 0;
    CTLineRef line = CreateLine(text, sf.attrs);
    if(line)
    {
        w = CTLineGetTypographicBounds(line, nullptr, nullptr, nullptr);
        CFRelease(line);
    }
    std::lock_guard<std::mutex> lock(sf.widthsMutex);
    if(sf.widths.size() > 8192)
        sf.widths.clear();
    sf.widths.emplace(text, w);
    return static_cast<float>(w);
}

static void DrawDecoration(CGContextRef ctx, CGFloat x, CGFloat y, CGFloat w, CGFloat t, int style)
{
    switch(style)
    {
    case lh::text_decoration_style_double:
        CGContextFillRect(ctx, CGRectMake(x, y - t / 2, w, t));
        CGContextFillRect(ctx, CGRectMake(x, y + t * 1.5, w, t));
        break;
    case lh::text_decoration_style_dotted:
    case lh::text_decoration_style_dashed:
        {
            CGFloat dots[]   = {t, t * 1.5};
            CGFloat dashes[] = {t * 3, t * 2};
            CGContextSetLineWidth(ctx, t);
            CGContextSetLineDash(ctx, 0, style == lh::text_decoration_style_dotted ? dots : dashes, 2);
            CGContextMoveToPoint(ctx, x, y);
            CGContextAddLineToPoint(ctx, x + w, y);
            CGContextStrokePath(ctx);
            CGContextSetLineDash(ctx, 0, nullptr, 0);
        }
        break;
    case lh::text_decoration_style_wavy:
        {
            CGFloat amp = std::max<CGFloat>(t, 1.0) * 1.2, wl = amp * 4;
            CGContextSetLineWidth(ctx, t);
            CGContextMoveToPoint(ctx, x, y);
            for(CGFloat cx = x; cx < x + w; cx += wl)
            {
                CGContextAddCurveToPoint(ctx, cx + wl / 4, y - amp, cx + wl / 4, y - amp, cx + wl / 2, y);
                CGContextAddCurveToPoint(ctx, cx + wl * 3 / 4, y + amp, cx + wl * 3 / 4, y + amp, cx + wl, y);
            }
            CGContextStrokePath(ctx);
        }
        break;
    default:
        CGContextFillRect(ctx, CGRectMake(x, y - t / 2, w, t));
        break;
    }
}

struct LHContainer::OpHash
{
    uint64_t value  = 0xcbf29ce484222325ULL;
    uint64_t shape  = 0xcbf29ce484222325ULL;
    bool     placed = false;
    double   anchor = 0;

    static void mix(uint64_t& hash, const void* data, size_t length)
    {
        auto p = static_cast<const unsigned char*>(data);
        for(size_t i = 0; i < length; i++)
            hash = (hash ^ p[i]) * 0x100000001b3ULL;
    }
    void bytes(const void* data, size_t length)
    {
        mix(value, data, length);
        mix(shape, data, length);
    }
    void number(double v) { bytes(&v, sizeof(v)); }
    void integer(int64_t v) { bytes(&v, sizeof(v)); }
    void text(const char* s, size_t length) { integer(static_cast<int64_t>(length)); bytes(s, length); }
    void text(const std::string& s) { text(s.data(), s.size()); }
    void vertical(double y)
    {
        mix(value, &y, sizeof(y));
        if(!placed)
        {
            placed = true;
            anchor = y;
        }
        const double offset = std::round((y - anchor) * 64) / 64;
        mix(shape, &offset, sizeof(offset));
    }
    void box(const lh::position& p)
    {
        number(F(p.x));
        vertical(F(p.y));
        number(F(p.width));
        number(F(p.height));
    }
    void clip(const lh::position& p, const lh::border_radiuses& r, CGRect drawn)
    {
        const double radii[8] = {F(r.top_left_x), F(r.top_left_y), F(r.top_right_x), F(r.top_right_y),
                                 F(r.bottom_right_x), F(r.bottom_right_y), F(r.bottom_left_x), F(r.bottom_left_y)};
        for(double v : {F(p.x), F(p.y), F(p.width), F(p.height)})
            mix(value, &v, sizeof(v));
        mix(value, radii, sizeof(radii));
        if(InsideRoundedRect(R(p), r, drawn))
            return;
        const CGRect part = CGRectIntersection(R(p), drawn);
        const double cut[4] = {std::round(part.origin.x * 64) / 64, std::round((part.origin.y - anchor) * 64) / 64,
                               std::round(part.size.width * 64) / 64, std::round(part.size.height * 64) / 64};
        mix(shape, cut, sizeof(cut));
        mix(shape, radii, sizeof(radii));
    }
    void color(const lh::web_color& c)
    {
        const unsigned char v[5] = {c.red, c.green, c.blue, c.alpha, c.is_current_color};
        bytes(v, sizeof(v));
    }
    void radius(const lh::border_radiuses& r)
    {
        for(lh::pixel_t v : {r.top_left_x, r.top_left_y, r.top_right_x, r.top_right_y, r.bottom_right_x,
                             r.bottom_right_y, r.bottom_left_x, r.bottom_left_y})
            number(F(v));
    }
    void layer(const lh::background_layer& l)
    {
        box(l.border_box);
        radius(l.border_radius);
        box(l.clip_box);
        box(l.origin_box);
        integer(l.attachment);
        integer(l.repeat);
        integer(l.is_root);
    }
    void gradient(const lh::background_layer::gradient_base& g)
    {
        integer(static_cast<int64_t>(g.color_points.size()));
        for(const auto& point : g.color_points)
        {
            number(point.offset);
            color(point.color);
        }
        integer(g.color_space);
        integer(g.hue_interpolation);
    }
    void border(const lh::border& b)
    {
        number(F(b.width));
        integer(b.style);
        color(b.color);
    }
};

void LHContainer::draw_text(lh::uint_ptr hdc, const char* text, lh::uint_ptr hFont, lh::web_color color,
                            const lh::position& pos)
{
    if(m_painter)
    {
        auto* font = reinterpret_cast<Font*>(hFont);
        if(!font || !text || !*text)
            return;
        lhx::TextStyle style;
        style.nativeFont          = font->shared->font;
        style.ascent              = float(font->shared->ascent);
        style.descent             = float(font->shared->descent);
        style.underline           = font->underline;
        style.overline            = font->overline;
        style.strike              = font->strike;
        style.decorationColor     = font->deco_color;
        style.decorationStyle     = font->deco_style;
        style.decorationThickness = float(font->deco_thickness);
        style.shadowColor         = font->shadow_color;
        style.shadowX             = float(font->shadow_x);
        style.shadowY             = float(font->shadow_y);
        style.shadowBlur          = float(font->shadow_blur);
        m_painter->text(text, style, color, pos);
        return;
    }
    auto* f   = reinterpret_cast<Font*>(hFont);
    auto  ctx = reinterpret_cast<CGContextRef>(hdc);
    if(!f || !ctx || !text || !*text)
        return;
    if(m_runs)
    {
        if(m_hidden_depth == 0)
            m_runs->push_back({text, f->shared->font, CGRectApplyAffineTransform(R(pos), group_matrix())});
        return;
    }
    CGRect  box   = R(pos);
    CGFloat slack = std::max<CGFloat>(4, box.size.height / 2);
    if(f->shadow_color.alpha != 0)
        slack += f->shadow_blur + std::max(std::fabs(f->shadow_x), std::fabs(f->shadow_y));
    CGRect  ink   = CGRectInset(box, -slack, -slack);
    if(m_ops)
    {
        OpHash hash;
        hash.integer(1);
        hash.text(text, strlen(text));
        hash.integer(reinterpret_cast<intptr_t>(f->shared.get()));
        hash.integer(f->underline | f->overline << 1 | f->strike << 2);
        hash.color(f->deco_color);
        hash.integer(f->deco_style);
        hash.number(f->deco_thickness);
        hash.color(f->shadow_color);
        hash.number(f->shadow_x);
        hash.number(f->shadow_y);
        hash.number(f->shadow_blur);
        hash.color(color);
        hash.box(pos);
        record_op(hash, ink, !record_glyphs(text, f, color, pos, ink));
        if(!m_capture)
            return;
        ctx = m_capture;
    }

    CGContextSaveGState(ctx);
    apply_clips(ctx, ink);

    CTLineRef line = CopyColoredLine(*f->shared, text, color);

    if(line)
    {
        CGFloat x        = F(pos.x);
        CGFloat baseline = F(pos.y) + f->shared->ascent;
        CGContextSetTextMatrix(ctx, CGAffineTransformMakeScale(1, -1));
        CGContextSetTextPosition(ctx, x, baseline);
        if(f->shadow_color.alpha != 0)
        {
            CGAffineTransform ctm    = CGContextGetCTM(ctx);
            CGSize            offset = CGSizeApplyAffineTransform(CGSizeMake(f->shadow_x, f->shadow_y), ctm);
            CGFloat           scale  = std::sqrt(std::fabs(ctm.a * ctm.d - ctm.b * ctm.c));
            CGColorRef        shadow = CreateColor(f->shadow_color);
            CGContextSetShadowWithColor(ctx, offset, f->shadow_blur * scale, shadow);
            CGColorRelease(shadow);
        }
        CTLineDraw(line, ctx);
        CGContextSetShadowWithColor(ctx, CGSizeZero, 0, nullptr);

        if(f->underline || f->overline || f->strike)
        {
            CGFloat       w  = CTLineGetTypographicBounds(line, nullptr, nullptr, nullptr);
            CGFloat       t  = f->deco_thickness > 0 ? f->deco_thickness : f->shared->underline_thickness;
            lh::web_color dc = f->deco_color.is_current_color ? color : f->deco_color;
            SetFill(ctx, dc);
            SetStroke(ctx, dc);
            if(f->underline)
                DrawDecoration(ctx, x, std::round(baseline + std::max<CGFloat>(f->shared->underline_pos, 1.0) + t / 2),
                               w, t, f->deco_style);
            if(f->strike)
                DrawDecoration(ctx, x, std::round(baseline - f->shared->x_height / 2), w, t, f->deco_style);
            if(f->overline)
                DrawDecoration(ctx, x, std::round(baseline - f->shared->ascent + t / 2), w, t, f->deco_style);
        }
        CFRelease(line);
    }
    CGContextRestoreGState(ctx);
}

lh::pixel_t LHContainer::pt_to_px(float pt) const
{
    return pt * 96.0f / 72.0f;
}

lh::pixel_t LHContainer::get_default_font_size() const
{
    return m_default_font_size;
}

const char* LHContainer::get_default_font_name() const
{
    return "-apple-system";
}

void LHContainer::transform_text(std::string& text, lh::text_transform tt)
{
    NSString* s = @(text.c_str());
    if(!s)
        return;
    switch(tt)
    {
    case lh::text_transform_uppercase:
        s = s.uppercaseString;
        break;
    case lh::text_transform_lowercase:
        s = s.lowercaseString;
        break;
    case lh::text_transform_capitalize:
        if(s.length)
        {
            NSRange first = [s rangeOfComposedCharacterSequenceAtIndex:0];
            s = [[s substringWithRange:first].uppercaseString stringByAppendingString:[s substringFromIndex:NSMaxRange(first)]];
        }
        break;
    default:
        return;
    }
    text = s.UTF8String;
}

void LHContainer::set_clip(const lh::position& pos, const lh::border_radiuses& bdr_radius)
{
    if(m_painter)
        m_painter->pushClip(pos, bdr_radius);
    m_clips.push_back({pos, bdr_radius, group_matrix()});
}

void LHContainer::del_clip()
{
    if(m_painter)
        m_painter->popClip();
    if(!m_clips.empty())
        m_clips.pop_back();
}

void LHContainer::flush_capture()
{
    if(!m_capture)
        return;
    if(m_gpu && m_gpu->store)
        m_gpu->store(m_capture_key, m_capture, m_capture_device);
    CGContextRelease(m_capture);
    m_capture = nullptr;
}

const uint8_t* LHContainer::device_color(const lh::web_color& color)
{
    if(m_device_colors_space != m_gpu->space)
    {
        m_device_colors.clear();
        m_device_colors_space = m_gpu->space;
    }
    const uint32_t key = color.red | color.green << 8 | color.blue << 16 | static_cast<uint32_t>(color.alpha) << 24;
    auto           it  = m_device_colors.find(key);
    if(it != m_device_colors.end())
        return it->second.data();
    std::array<uint8_t, 4> pixel = {0, 0, 0, 0};
    CGContextRef ctx = CGBitmapContextCreate(pixel.data(), 1, 1, 8, 4, m_gpu->space,
                                             kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    if(ctx)
    {
        SetFill(ctx, color);
        CGContextFillRect(ctx, CGRectMake(0, 0, 1, 1));
        CGContextRelease(ctx);
    }
    return m_device_colors.emplace(key, pixel).first->second.data();
}

void LHContainer::push_mask(const LHGlyphSlot& slot, CGRect device, const uint8_t* rgba)
{
    if(slot.empty)
        return;
    LHGPUOp op;
    op.kind   = LHGPUOp::Mask;
    op.device = device;
    op.key    = static_cast<uint64_t>(slot.u) | static_cast<uint64_t>(slot.v) << 16 |
             static_cast<uint64_t>(slot.device.size.width) << 32 | static_cast<uint64_t>(slot.device.size.height) << 48;
    memcpy(op.color, rgba, 4);
    m_gpu->ops.push_back(op);
}

bool LHContainer::record_rounded(CGRect rect, const lh::border_radiuses& radius, const lh::web_color& color)
{
    if(!m_gpu->mask)
        return false;
    const CGFloat s  = m_gpu->scale;
    const CGFloat x0 = rect.origin.x * s, y0 = rect.origin.y * s, x1 = CGRectGetMaxX(rect) * s, y1 = CGRectGetMaxY(rect) * s;
    const CGFloat r[8] = {F(radius.top_left_x) * s, F(radius.top_left_y) * s, F(radius.top_right_x) * s, F(radius.top_right_y) * s,
                          F(radius.bottom_right_x) * s, F(radius.bottom_right_y) * s, F(radius.bottom_left_x) * s, F(radius.bottom_left_y) * s};
    for(CGFloat v : r)
    {
        if(v <= 0)
            return false;
    }
    if(r[0] + r[2] > x1 - x0 || r[6] + r[4] > x1 - x0 || r[1] + r[7] > y1 - y0 || r[3] + r[5] > y1 - y0)
        return false;
    const CGRect tl = CGRectMake(std::floor(x0), std::floor(y0), std::ceil(x0 + r[0]) - std::floor(x0), std::ceil(y0 + r[1]) - std::floor(y0));
    const CGRect tr = CGRectMake(std::floor(x1 - r[2]), std::floor(y0), std::ceil(x1) - std::floor(x1 - r[2]), std::ceil(y0 + r[3]) - std::floor(y0));
    const CGRect br = CGRectMake(std::floor(x1 - r[4]), std::floor(y1 - r[5]), std::ceil(x1) - std::floor(x1 - r[4]), std::ceil(y1) - std::floor(y1 - r[5]));
    const CGRect bl = CGRectMake(std::floor(x0), std::floor(y1 - r[7]), std::ceil(x0 + r[6]) - std::floor(x0), std::ceil(y1) - std::floor(y1 - r[7]));
    if(CGRectGetMaxY(tl) != CGRectGetMaxY(tr) || CGRectGetMinY(bl) != CGRectGetMinY(br) || CGRectGetMaxX(tl) > CGRectGetMinX(tr) ||
       CGRectGetMaxX(bl) > CGRectGetMinX(br) || CGRectGetMaxY(tl) > CGRectGetMinY(bl))
        return false;
    const uint8_t* rgba = device_color(color);
    auto           mask = [&](int part, CGRect box, CGRect canonical, const CGFloat* radii) -> bool {
        uint64_t key = 0xcbf29ce484222325ULL;
        double   v[12] = {double(part), canonical.origin.x - box.origin.x, canonical.origin.y - box.origin.y,
                          CGRectGetMaxX(canonical) - box.origin.x, CGRectGetMaxY(canonical) - box.origin.y, box.size.width, box.size.height,
                          radii[0], radii[1], radii[2], radii[3], s};
        OpHash::mix(key, v, sizeof(v));
        lh::border_radiuses rad;
        rad.top_left_x     = float(radii[0] / s);
        rad.top_left_y     = float(radii[1] / s);
        rad.top_right_x    = float(radii[2] / s);
        rad.top_right_y    = float(radii[3] / s);
        rad.bottom_right_x = float(radii[4] / s);
        rad.bottom_right_y = float(radii[5] / s);
        rad.bottom_left_x  = float(radii[6] / s);
        rad.bottom_left_y  = float(radii[7] / s);
        const CGRect doc   = CGRectMake(canonical.origin.x / s, canonical.origin.y / s, canonical.size.width / s, canonical.size.height / s);
        LHGlyphSlot  slot;
        bool ok = m_gpu->mask(key, box, [&](CGContextRef ctx) {
            ClipToRoundedRect(ctx, doc, rad);
            CGContextClipToRect(ctx, doc);
            CGContextFillRect(ctx, doc);
        }, slot);
        if(!ok)
            return false;
        push_mask(slot, box, rgba);
        return true;
    };
    flush_capture();
    const size_t mark = m_gpu->ops.size();
    const CGFloat pad = 8;
    const CGFloat none[8] = {0, 0, 0, 0, 0, 0, 0, 0};
    CGFloat       c0[8] = {r[0], r[1], 0, 0, 0, 0, 0, 0}, c1[8] = {0, 0, r[2], r[3], 0, 0, 0, 0};
    CGFloat       c2[8] = {0, 0, 0, 0, r[4], r[5], 0, 0}, c3[8] = {0, 0, 0, 0, 0, 0, r[6], r[7]};
    auto span = [&](CGFloat box, CGFloat radius) { return std::max(box + pad, radius * 2 + pad); };
    const CGFloat w0 = span(CGRectGetMaxX(tl) - x0, r[0]), h0 = span(CGRectGetMaxY(tl) - y0, r[1]);
    const CGFloat w1 = span(x1 - tr.origin.x, r[2]), h1 = span(CGRectGetMaxY(tr) - y0, r[3]);
    const CGFloat w2 = span(x1 - br.origin.x, r[4]), h2 = span(y1 - br.origin.y, r[5]);
    const CGFloat w3 = span(CGRectGetMaxX(bl) - x0, r[6]), h3 = span(y1 - bl.origin.y, r[7]);
    bool ok = mask(0, tl, CGRectMake(x0, y0, w0, h0), c0) &&
              mask(1, tr, CGRectMake(x1 - w1, y0, w1, h1), c1) &&
              mask(2, br, CGRectMake(x1 - w2, y1 - h2, w2, h2), c2) &&
              mask(3, bl, CGRectMake(x0, y1 - h3, w3, h3), c3);
    const CGFloat ix0 = std::ceil(x0), iy0 = std::ceil(y0), ix1 = std::floor(x1), iy1 = std::floor(y1);
    auto edge = [&](int part, CGRect strip, CGRect canonical) -> bool {
        if(strip.size.width <= 0 || strip.size.height <= 0)
            return true;
        uint64_t key = 0xcbf29ce484222325ULL;
        double   v[6] = {double(part + 10), canonical.origin.x - strip.origin.x, canonical.origin.y - strip.origin.y, CGRectGetMaxX(canonical) - strip.origin.x,
                         CGRectGetMaxY(canonical) - strip.origin.y, s};
        OpHash::mix(key, v, sizeof(v));
        const CGRect doc = CGRectMake(canonical.origin.x / s, canonical.origin.y / s, canonical.size.width / s, canonical.size.height / s);
        LHGlyphSlot  slot;
        if(!m_gpu->mask(key, CGRectMake(strip.origin.x, strip.origin.y, 1, 1), [&](CGContextRef ctx) {
               lh::border_radiuses zero;
               ClipToRoundedRect(ctx, doc, zero);
               CGContextClipToRect(ctx, doc);
               CGContextFillRect(ctx, doc);
           }, slot))
            return false;
        push_mask(slot, strip, rgba);
        return true;
    };
    if(ok && y0 != iy0)
        ok = edge(0, CGRectMake(CGRectGetMaxX(tl), std::floor(y0), CGRectGetMinX(tr) - CGRectGetMaxX(tl), 1),
                  CGRectMake(CGRectGetMaxX(tl) - pad, y0, pad * 2 + 1, iy0 + pad - y0));
    if(ok && y1 != iy1)
        ok = edge(1, CGRectMake(CGRectGetMaxX(bl), iy1, CGRectGetMinX(br) - CGRectGetMaxX(bl), 1),
                  CGRectMake(CGRectGetMaxX(bl) - pad, iy1 - pad, pad * 2 + 1, y1 - iy1 + pad));
    if(ok && x0 != ix0)
        ok = edge(2, CGRectMake(std::floor(x0), CGRectGetMaxY(tl), 1, CGRectGetMinY(bl) - CGRectGetMaxY(tl)),
                  CGRectMake(x0, CGRectGetMaxY(tl) - pad, ix0 + pad - x0, pad * 2 + 1));
    if(ok && x1 != ix1)
        ok = edge(3, CGRectMake(ix1, CGRectGetMaxY(tl), 1, CGRectGetMinY(bl) - CGRectGetMaxY(tl)),
                  CGRectMake(ix1 - pad, CGRectGetMaxY(tl) - pad, x1 - ix1 + pad, pad * 2 + 1));
    (void)none;
    if(!ok)
    {
        m_gpu->ops.resize(mark);
        return false;
    }
    auto slab = [&](CGFloat ax, CGFloat ay, CGFloat bx, CGFloat by) {
        if(bx <= ax || by <= ay)
            return;
        LHGPUOp op;
        op.kind   = LHGPUOp::Fill;
        op.device = CGRectMake(ax, ay, bx - ax, by - ay);
        op.key    = 0;
        memcpy(op.color, rgba, 4);
        m_gpu->ops.push_back(op);
    };
    slab(CGRectGetMaxX(tl), iy0, CGRectGetMinX(tr), CGRectGetMaxY(tl));
    slab(ix0, CGRectGetMaxY(tl), ix1, CGRectGetMinY(bl));
    slab(CGRectGetMaxX(bl), CGRectGetMinY(bl), CGRectGetMinX(br), iy1);
    return true;
}

bool LHContainer::record_solid(const lh::background_layer& layer, const lh::web_color& color)
{
    if(m_gpu && m_hidden_depth == 0 && m_groups.empty() && color.alpha != 0 && HasRadius(layer.border_radius) &&
       CGRectEqualToRect(R(layer.clip_box), R(layer.border_box)))
    {
        const CGRect rect = R(layer.border_box);
        bool         inside = true;
        for(const auto& c : m_clips)
        {
            if(!CGAffineTransformIsIdentity(c.matrix) || !InsideRoundedRect(R(c.box), c.radius, CGRectInset(rect, -1, -1)))
                inside = false;
        }
        if(inside && record_rounded(rect, layer.border_radius, color))
            return true;
    }
    if(!m_gpu || m_hidden_depth > 0 || HasRadius(layer.border_radius) || !m_groups.empty() || color.alpha == 0)
        return false;
    CGRect rect = CGRectIntersection(R(layer.clip_box), R(layer.border_box));
    for(const auto& c : m_clips)
    {
        if(!CGAffineTransformIsIdentity(c.matrix))
            return false;
        if(HasRadius(c.radius) && !InsideRoundedRect(R(c.box), c.radius, CGRectInset(rect, -1, -1)))
            return false;
        rect = CGRectIntersection(rect, R(c.box));
    }
    flush_capture();
    if(CGRectIsNull(rect) || CGRectIsEmpty(rect))
        return true;
    const CGFloat s = m_gpu->scale;
    LHGPUOp       op;
    op.kind   = LHGPUOp::Fill;
    op.device = CGRectMake(rect.origin.x * s, rect.origin.y * s, rect.size.width * s, rect.size.height * s);
    op.key    = 0;
    memcpy(op.color, device_color(color), 4);
    m_gpu->ops.push_back(op);
    return true;
}

bool LHContainer::record_glyphs(const char* text, Font* f, const lh::web_color& color, const lh::position& pos, CGRect ink)
{
    if(!m_gpu || !m_gpu->glyph || m_hidden_depth > 0 || !m_groups.empty() || f->shadow_color.alpha != 0 || f->underline ||
       f->overline || f->strike || color.alpha == 0)
        return false;
    for(const auto& c : m_clips)
    {
        if(!CGAffineTransformIsIdentity(c.matrix) || !InsideRoundedRect(R(c.box), c.radius, ink))
            return false;
    }
    CTLineRef line = CopyColoredLine(*f->shared, text, color);
    if(!line)
        return false;
    CFArrayRef runs  = CTLineGetGlyphRuns(line);
    CFIndex    count = CFArrayGetCount(runs);
    for(CFIndex i = 0; i < count; i++)
    {
        auto      run  = static_cast<CTRunRef>(CFArrayGetValueAtIndex(runs, i));
        CTFontRef font = static_cast<CTFontRef>(CFDictionaryGetValue(CTRunGetAttributes(run), kCTFontAttributeName));
        if(!font || (CTFontGetSymbolicTraits(font) & kCTFontTraitColorGlyphs))
        {
            CFRelease(line);
            return false;
        }
    }
    flush_capture();
    const size_t  mark     = m_gpu->ops.size();
    const CGFloat s        = m_gpu->scale;
    const CGFloat x        = F(pos.x);
    const CGFloat baseline = F(pos.y) + f->shared->ascent;
    const uint8_t* rgba    = device_color(color);
    std::vector<CGGlyph> glyphs;
    std::vector<CGPoint> points;
    for(CFIndex i = 0; i < count; i++)
    {
        auto      run  = static_cast<CTRunRef>(CFArrayGetValueAtIndex(runs, i));
        CTFontRef font = static_cast<CTFontRef>(CFDictionaryGetValue(CTRunGetAttributes(run), kCTFontAttributeName));
        CFIndex   n    = CTRunGetGlyphCount(run);
        glyphs.resize(n);
        points.resize(n);
        CTRunGetGlyphs(run, CFRangeMake(0, 0), glyphs.data());
        CTRunGetPositions(run, CFRangeMake(0, 0), points.data());
        for(CFIndex g = 0; g < n; g++)
        {
            CGPoint     device = CGPointMake((x + points[g].x) * s, (baseline - points[g].y) * s);
            LHGlyphSlot slot;
            if(!m_gpu->glyph(font, glyphs[g], device, slot))
            {
                m_gpu->ops.resize(mark);
                CFRelease(line);
                return false;
            }
            if(slot.empty)
                continue;
            LHGPUOp op;
            op.kind   = LHGPUOp::Mask;
            op.device = slot.device;
            op.key    = static_cast<uint64_t>(slot.u) | static_cast<uint64_t>(slot.v) << 16;
            memcpy(op.color, rgba, 4);
            m_gpu->ops.push_back(op);
        }
    }
    CFRelease(line);
    return true;
}

void LHContainer::record_op(OpHash& hash, CGRect ink, bool capture)
{
    if(m_hidden_depth > 0)
        return;
    if(m_gpu)
        flush_capture();
    auto HashMatrix = [](OpHash& hash, const CGAffineTransform& m) {
        if(CGAffineTransformIsIdentity(m))
            return;
        for(CGFloat v : {m.a, m.b, m.c, m.d, m.tx, m.ty})
            hash.number(v);
    };
    CGAffineTransform matrix = group_matrix();
    CGRect            bounds = CGAffineTransformIsIdentity(matrix) ? ink : CGRectApplyAffineTransform(ink, matrix);
    const CGRect      drawn  = bounds;
    for(const auto& c : m_clips)
    {
        hash.clip(c.box, c.radius, drawn);
        HashMatrix(hash, c.matrix);
        bounds = CGRectIntersection(bounds, CGAffineTransformIsIdentity(c.matrix) ? R(c.box)
                                                                                 : CGRectApplyAffineTransform(R(c.box), c.matrix));
    }
    for(const auto& g : m_groups)
        hash.number(g.opacity);
    HashMatrix(hash, matrix);
    m_ops->push_back({CGRectIsNull(bounds) ? CGRectNull : CGRectInset(bounds, -1, -1), hash.value, hash.shape});
    if(!m_gpu || !capture || CGRectIsNull(bounds) || CGRectIsEmpty(bounds))
        return;
    for(const auto& g : m_groups)
    {
        if(g.opacity < 1)
            m_gpu->unsupported = true;
    }
    const CGFloat s      = m_gpu->scale;
    const CGRect  device = CGRectIntegral(CGRectMake(bounds.origin.x * s, bounds.origin.y * s, bounds.size.width * s, bounds.size.height * s));
    if(device.size.width <= 0 || device.size.height <= 0 || device.size.width > 8192 || device.size.height > 8192)
        return;
    uint64_t key = hash.value;
    for(double v : {double(s), double(device.origin.x), double(device.origin.y), double(device.size.width), double(device.size.height)})
        OpHash::mix(key, &v, sizeof(v));
    LHGPUOp op;
    op.kind   = LHGPUOp::Bitmap;
    op.device = device;
    op.key    = key;
    m_gpu->ops.push_back(op);
    if(m_gpu->cached && m_gpu->cached(key))
        return;
    const size_t w   = static_cast<size_t>(device.size.width);
    const size_t h   = static_cast<size_t>(device.size.height);
    CGContextRef ctx = m_gpu->allocate ? m_gpu->allocate(key, device)
                                       : CGBitmapContextCreate(nullptr, w, h, 8, 0, m_gpu->space, kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    if(!ctx)
        return;
    CGContextTranslateCTM(ctx, 0, h);
    CGContextScaleCTM(ctx, s, -s);
    CGContextTranslateCTM(ctx, -device.origin.x / s, -device.origin.y / s);
    CGContextConcatCTM(ctx, group_matrix());
    CGContextSetAllowsFontSmoothing(ctx, false);
    CGContextSetShouldAntialias(ctx, true);
    m_capture        = ctx;
    m_capture_key    = key;
    m_capture_device = device;
}

void LHContainer::apply_clips(CGContextRef ctx, CGRect bounds) const
{
    CGRect            ink    = CGRectInset(bounds, -1, -1);
    CGAffineTransform matrix = group_matrix();
    for(const auto& c : m_clips)
    {
        CGRect box = R(c.box);
        if(CGAffineTransformEqualToTransform(c.matrix, matrix))
        {
            if(m_clip_culling && InsideRoundedRect(box, c.radius, ink))
                continue;
            ClipToRoundedRect(ctx, box, c.radius);
            continue;
        }
        CGAffineTransform t = CGAffineTransformConcat(c.matrix, CGAffineTransformInvert(matrix));
        CGContextConcatCTM(ctx, t);
        ClipToRoundedRect(ctx, box, c.radius);
        CGContextConcatCTM(ctx, CGAffineTransformInvert(t));
    }
}

void LHContainer::clip_background_layer(CGContextRef ctx, const lh::background_layer& layer) const
{
    ClipToRoundedRect(ctx, R(layer.border_box), layer.border_radius);
    CGContextClipToRect(ctx, R(layer.clip_box));
}

void LHContainer::for_each_tile(const lh::background_layer& layer, CGSize tile,
                                const std::function<void(CGRect)>& fn) const
{
    CGRect origin = R(layer.origin_box);
    CGRect clip   = R(layer.clip_box);
    if(tile.width < 1 || tile.height < 1)
        return;
    bool rx = layer.repeat == lh::background_repeat_repeat || layer.repeat == lh::background_repeat_repeat_x;
    bool ry = layer.repeat == lh::background_repeat_repeat || layer.repeat == lh::background_repeat_repeat_y;

    CGFloat startX = origin.origin.x, startY = origin.origin.y;
    CGFloat endX = startX + tile.width, endY = startY + tile.height;
    if(rx)
    {
        startX -= std::ceil((startX - CGRectGetMinX(clip)) / tile.width) * tile.width;
        endX = CGRectGetMaxX(clip);
    }
    if(ry)
    {
        startY -= std::ceil((startY - CGRectGetMinY(clip)) / tile.height) * tile.height;
        endY = CGRectGetMaxY(clip);
    }
    int guard = 0;
    for(CGFloat y = startY; y < endY && guard < 10000; y += tile.height)
        for(CGFloat x = startX; x < endX && guard < 10000; x += tile.width, ++guard)
            fn(CGRectMake(x, y, tile.width, tile.height));
}

void LHContainer::draw_solid_fill(lh::uint_ptr hdc, const lh::background_layer& layer, const lh::web_color& color)
{
    if(m_painter)
    {
        m_painter->solidFill(layer, color);
        return;
    }
    if(m_runs)
        return;
    if(m_ops)
    {
        OpHash hash;
        hash.integer(2);
        hash.layer(layer);
        hash.color(color);
        record_op(hash, R(layer.clip_box), !record_solid(layer, color));
        if(!m_capture)
            return;
        hdc = reinterpret_cast<lh::uint_ptr>(m_capture);
    }
    auto ctx = reinterpret_cast<CGContextRef>(hdc);
    if(color.alpha == 0)
        return;
    CGContextSaveGState(ctx);
    apply_clips(ctx, R(layer.clip_box));
    clip_background_layer(ctx, layer);
    SetFill(ctx, color);
    CGContextFillRect(ctx, R(layer.clip_box));
    CGContextRestoreGState(ctx);
}

static CGGradientRef CreateGradient(const std::vector<lh::background_layer::color_point>& points)
{
    if(points.empty())
        return nullptr;
    std::vector<CGFloat> comps, locs;
    CGFloat              prev = 0;
    for(const auto& p : points)
    {
        comps.insert(comps.end(), {p.color.red / 255.0, p.color.green / 255.0, p.color.blue / 255.0,
                                   p.color.alpha / 255.0});
        prev = std::max(prev, std::clamp<CGFloat>(p.offset, 0, 1));
        locs.push_back(prev);
    }
    if(points.size() == 1)
    {
        comps.insert(comps.end(), comps.begin(), comps.begin() + 4);
        locs = {0, 1};
    }
    return CGGradientCreateWithColorComponents(SRGB(), comps.data(), locs.data(), locs.size());
}

static void ConicColorAt(const std::vector<lh::background_layer::color_point>& points, CGFloat t, CGFloat out[4])
{
    auto set = [&](const lh::web_color& a, const lh::web_color& b, CGFloat k) {
        out[0] = (a.red + (b.red - a.red) * k) / 255.0;
        out[1] = (a.green + (b.green - a.green) * k) / 255.0;
        out[2] = (a.blue + (b.blue - a.blue) * k) / 255.0;
        out[3] = (a.alpha + (b.alpha - a.alpha) * k) / 255.0;
    };
    CGFloat prevOffset = std::clamp<CGFloat>(points.front().offset, 0, 1);
    if(t <= prevOffset)
    {
        set(points.front().color, points.front().color, 0);
        return;
    }
    for(size_t i = 1; i < points.size(); i++)
    {
        CGFloat offset = std::max(prevOffset, std::clamp<CGFloat>(points[i].offset, 0, 1));
        if(t <= offset)
        {
            CGFloat span = offset - prevOffset;
            set(points[i - 1].color, points[i].color, span > 0 ? (t - prevOffset) / span : 1);
            return;
        }
        prevOffset = offset;
    }
    set(points.back().color, points.back().color, 0);
}

static void DrawConicWedges(CGContextRef ctx, const std::vector<lh::background_layer::color_point>& points, CGPoint c,
                            CGFloat start, CGFloat radius)
{
    const int steps = 180;
    for(int i = 0; i < steps; i++)
    {
        CGFloat a0 = start + 2 * M_PI * i / steps;
        CGFloat a1 = start + 2 * M_PI * (i + 1) / steps + 0.01;
        CGFloat color[4];
        ConicColorAt(points, (i + 0.5) / steps, color);
        CGContextSetRGBFillColor(ctx, color[0], color[1], color[2], color[3]);
        CGContextBeginPath(ctx);
        CGContextMoveToPoint(ctx, c.x, c.y);
        CGContextAddLineToPoint(ctx, c.x + radius * std::cos(a0), c.y + radius * std::sin(a0));
        CGContextAddLineToPoint(ctx, c.x + radius * std::cos(a1), c.y + radius * std::sin(a1));
        CGContextClosePath(ctx);
        CGContextFillPath(ctx);
    }
}

static void AppendColorPoints(std::string& key, const std::vector<lh::background_layer::color_point>& points)
{
    for(const auto& p : points)
    {
        AppendNumber(key, p.offset);
        AppendInteger(key, p.color.red | p.color.green << 8 | p.color.blue << 16 | (uint32_t)p.color.alpha << 24);
    }
    key.push_back('|');
}

void LHContainer::draw_linear_gradient(lh::uint_ptr hdc, const lh::background_layer& layer,
                                       const lh::background_layer::linear_gradient& gradient)
{
    if(m_painter)
    {
        m_painter->linearGradient(layer, gradient);
        return;
    }
    if(m_runs)
        return;
    if(m_ops)
    {
        OpHash hash;
        hash.integer(3);
        hash.layer(layer);
        hash.gradient(gradient);
        hash.number(gradient.start.x);
        hash.number(gradient.start.y);
        hash.number(gradient.end.x);
        hash.number(gradient.end.y);
        record_op(hash, R(layer.clip_box));
        if(!m_capture)
            return;
        hdc = reinterpret_cast<lh::uint_ptr>(m_capture);
    }
    auto          ctx = reinterpret_cast<CGContextRef>(hdc);
    CGGradientRef g   = CreateGradient(gradient.color_points);
    if(!g)
        return;
    CGContextSaveGState(ctx);
    apply_clips(ctx, R(layer.clip_box));
    clip_background_layer(ctx, layer);
    CGRect origin = R(layer.origin_box);
    std::string key = "L|";
    AppendColorPoints(key, gradient.color_points);
    AppendNumber(key, gradient.start.x - origin.origin.x);
    AppendNumber(key, gradient.start.y - origin.origin.y);
    AppendNumber(key, gradient.end.x - origin.origin.x);
    AppendNumber(key, gradient.end.y - origin.origin.y);
    for_each_tile(layer, origin.size, [&](CGRect tile) {
        auto paint = [&](CGContextRef c) {
            CGContextSaveGState(c);
            CGContextClipToRect(c, tile);
            CGContextTranslateCTM(c, tile.origin.x - origin.origin.x, tile.origin.y - origin.origin.y);
            CGContextDrawLinearGradient(c, g, CGPointMake(gradient.start.x, gradient.start.y),
                                        CGPointMake(gradient.end.x, gradient.end.y),
                                        kCGGradientDrawsBeforeStartLocation | kCGGradientDrawsAfterEndLocation);
            CGContextRestoreGState(c);
        };
        if(!DrawRasterized(ctx, tile, key, paint))
            paint(ctx);
    });
    CGContextRestoreGState(ctx);
    CGGradientRelease(g);
}

void LHContainer::draw_radial_gradient(lh::uint_ptr hdc, const lh::background_layer& layer,
                                       const lh::background_layer::radial_gradient& gradient)
{
    if(m_painter)
    {
        m_painter->radialGradient(layer, gradient);
        return;
    }
    if(m_runs)
        return;
    if(m_ops)
    {
        OpHash hash;
        hash.integer(4);
        hash.layer(layer);
        hash.gradient(gradient);
        hash.number(gradient.position.x);
        hash.number(gradient.position.y);
        hash.number(gradient.radius.x);
        hash.number(gradient.radius.y);
        record_op(hash, R(layer.clip_box));
        if(!m_capture)
            return;
        hdc = reinterpret_cast<lh::uint_ptr>(m_capture);
    }
    auto ctx = reinterpret_cast<CGContextRef>(hdc);
    if(gradient.radius.x <= 0 || gradient.radius.y <= 0)
        return;
    CGGradientRef g = CreateGradient(gradient.color_points);
    if(!g)
        return;
    CGContextSaveGState(ctx);
    apply_clips(ctx, R(layer.clip_box));
    clip_background_layer(ctx, layer);
    CGRect  origin = R(layer.origin_box);
    CGPoint c      = CGPointMake(gradient.position.x, gradient.position.y);
    std::string key = "R|";
    AppendColorPoints(key, gradient.color_points);
    AppendNumber(key, c.x - origin.origin.x);
    AppendNumber(key, c.y - origin.origin.y);
    AppendNumber(key, gradient.radius.x);
    AppendNumber(key, gradient.radius.y);
    for_each_tile(layer, origin.size, [&](CGRect tile) {
        auto paint = [&](CGContextRef cg) {
            CGContextSaveGState(cg);
            CGContextClipToRect(cg, tile);
            CGContextTranslateCTM(cg, tile.origin.x - origin.origin.x, tile.origin.y - origin.origin.y);
            CGContextTranslateCTM(cg, c.x, c.y);
            CGContextScaleCTM(cg, 1, gradient.radius.y / gradient.radius.x);
            CGContextTranslateCTM(cg, -c.x, -c.y);
            CGContextDrawRadialGradient(cg, g, c, 0, c, gradient.radius.x,
                                        kCGGradientDrawsBeforeStartLocation | kCGGradientDrawsAfterEndLocation);
            CGContextRestoreGState(cg);
        };
        if(!DrawRasterized(ctx, tile, key, paint))
            paint(ctx);
    });
    CGContextRestoreGState(ctx);
    CGGradientRelease(g);
}

void LHContainer::draw_conic_gradient(lh::uint_ptr hdc, const lh::background_layer& layer,
                                      const lh::background_layer::conic_gradient& gradient)
{
    if(m_painter)
    {
        m_painter->conicGradient(layer, gradient);
        return;
    }
    if(m_runs)
        return;
    if(m_ops)
    {
        OpHash hash;
        hash.integer(5);
        hash.layer(layer);
        hash.gradient(gradient);
        hash.number(gradient.position.x);
        hash.number(gradient.position.y);
        hash.number(gradient.angle);
        hash.number(gradient.radius);
        record_op(hash, R(layer.clip_box));
        if(!m_capture)
            return;
        hdc = reinterpret_cast<lh::uint_ptr>(m_capture);
    }
    auto ctx = reinterpret_cast<CGContextRef>(hdc);
    if(gradient.color_points.empty())
        return;
    CGContextSaveGState(ctx);
    apply_clips(ctx, R(layer.clip_box));
    clip_background_layer(ctx, layer);
    CGRect  origin = R(layer.origin_box);
    CGPoint c      = CGPointMake(gradient.position.x, gradient.position.y);
    CGFloat start  = (gradient.angle - 90.0) * M_PI / 180.0;
    CGFloat radius = std::hypot(origin.size.width, origin.size.height) + 2;
    std::string key = "C|";
    AppendColorPoints(key, gradient.color_points);
    AppendNumber(key, c.x - origin.origin.x);
    AppendNumber(key, c.y - origin.origin.y);
    AppendNumber(key, start);
    AppendNumber(key, radius);
    for_each_tile(layer, origin.size, [&](CGRect tile) {
        auto paint = [&](CGContextRef cg) {
            CGContextSaveGState(cg);
            CGContextClipToRect(cg, tile);
            CGContextTranslateCTM(cg, tile.origin.x - origin.origin.x, tile.origin.y - origin.origin.y);
            if(@available(macOS 14.0, *))
            {
                CGGradientRef g = CreateGradient(gradient.color_points);
                if(g)
                {
                    CGContextDrawConicGradient(cg, g, c, start);
                    CGGradientRelease(g);
                }
            }
            else
                DrawConicWedges(cg, gradient.color_points, c, start, radius);
            CGContextRestoreGState(cg);
        };
        if(!DrawRasterized(ctx, tile, key, paint))
            paint(ctx);
    });
    CGContextRestoreGState(ctx);
}

void LHContainer::load_image(const char* src, const char* baseurl, bool redraw_on_ready)
{
    if(src)
        LookupImage(src);
}

void LHContainer::get_image_size(const char* src, const char* baseurl, lh::size& sz)
{
    ImageHandle img = src ? LookupImage(src) : ImageHandle();
    sz.width        = img ? static_cast<float>(img.size.width) : 0.0f;
    sz.height       = img ? static_cast<float>(img.size.height) : 0.0f;
}

void LHContainer::draw_image(lh::uint_ptr hdc, const lh::background_layer& layer, const std::string& url,
                             const std::string& base_url)
{
    if(m_painter)
    {
        m_painter->image(layer, url);
        return;
    }
    if(m_runs)
        return;
    if(m_ops)
    {
        OpHash hash;
        hash.integer(6);
        hash.layer(layer);
        hash.text(url);
        record_op(hash, R(layer.clip_box));
        if(!m_capture)
            return;
        hdc = reinterpret_cast<lh::uint_ptr>(m_capture);
    }
    auto ctx = reinterpret_cast<CGContextRef>(hdc);
    if(url.empty() || layer.clip_box.width == 0_px || layer.clip_box.height == 0_px)
        return;
    ImageHandle img = LookupImage(url);
    if(!img)
        return;
    CGContextSaveGState(ctx);
    apply_clips(ctx, R(layer.clip_box));
    clip_background_layer(ctx, layer);
    CGRect origin = R(layer.origin_box);
    std::string key = "I|";
    AppendInteger(key, std::hash<std::string>()(url));
    AppendInteger(key, url.size());
    AppendInteger(key, (uintptr_t)img.image);
    AppendInteger(key, CGImageGetWidth(img.image));
    AppendInteger(key, CGImageGetHeight(img.image));
    const bool changesEveryFrame = url.compare(0, 9, "twcanvas:") == 0;
    for_each_tile(layer, origin.size, [&](CGRect tile) {
        if(changesEveryFrame || !DrawRasterized(ctx, tile, key, [&](CGContextRef c) { DrawCGImage(c, img.image, tile); }))
            DrawCGImage(ctx, img.image, tile);
    });
    CGContextRestoreGState(ctx);
}

void LHContainer::draw_list_marker(lh::uint_ptr hdc, const lh::list_marker& marker)
{
    if(m_painter)
    {
        m_painter->listMarker(marker);
        return;
    }
    if(m_runs)
        return;
    if(m_ops)
    {
        OpHash hash;
        hash.integer(7);
        hash.text(marker.image);
        hash.integer(marker.marker_type);
        hash.color(marker.color);
        hash.box(marker.pos);
        hash.integer(marker.index);
        record_op(hash, R(marker.pos));
        if(!m_capture)
            return;
        hdc = reinterpret_cast<lh::uint_ptr>(m_capture);
    }
    auto   ctx = reinterpret_cast<CGContextRef>(hdc);
    CGRect r   = R(marker.pos);
    CGContextSaveGState(ctx);
    apply_clips(ctx, r);
    if(!marker.image.empty())
    {
        ImageHandle img = LookupImage(marker.image);
        if(img)
            DrawCGImage(ctx, img.image, r);
    }
    else
    {
        switch(marker.marker_type)
        {
        case lh::list_style_type_circle:
            SetStroke(ctx, marker.color);
            CGContextSetLineWidth(ctx, 1);
            CGContextStrokeEllipseInRect(ctx, CGRectInset(r, 0.5, 0.5));
            break;
        case lh::list_style_type_disc:
            SetFill(ctx, marker.color);
            CGContextFillEllipseInRect(ctx, r);
            break;
        case lh::list_style_type_square:
            SetFill(ctx, marker.color);
            CGContextFillRect(ctx, r);
            break;
        default:
            break;
        }
    }
    CGContextRestoreGState(ctx);
}

void LHContainer::draw_borders(lh::uint_ptr hdc, const lh::borders& borders, const lh::position& draw_pos, bool root)
{
    if(m_painter)
    {
        m_painter->borders(borders, draw_pos, root);
        return;
    }
    if(m_runs)
        return;
    if(m_ops)
    {
        OpHash hash;
        hash.integer(8);
        hash.border(borders.left);
        hash.border(borders.top);
        hash.border(borders.right);
        hash.border(borders.bottom);
        hash.radius(borders.radius);
        hash.box(draw_pos);
        hash.integer(root);
        record_op(hash, R(draw_pos));
        if(!m_capture)
            return;
        hdc = reinterpret_cast<lh::uint_ptr>(m_capture);
    }
    auto    ctx     = reinterpret_cast<CGContextRef>(hdc);
    auto    visible = [](const lh::border& b) { return b.width > 0_px && b.style > lh::border_style_hidden; };
    CGFloat L       = visible(borders.left) ? F(borders.left.width) : 0;
    CGFloat T       = visible(borders.top) ? F(borders.top.width) : 0;
    CGFloat Rr      = visible(borders.right) ? F(borders.right.width) : 0;
    CGFloat B       = visible(borders.bottom) ? F(borders.bottom.width) : 0;
    if(L == 0 && T == 0 && Rr == 0 && B == 0)
        return;

    CGRect outer = R(draw_pos);
    CGRect inner = CGRectMake(outer.origin.x + L, outer.origin.y + T, outer.size.width - L - Rr,
                              outer.size.height - T - B);
    const lh::border_radiuses& orad = borders.radius;
    lh::border_radiuses        irad = orad;
    irad.top_left_x     = std::max(0.0f, orad.top_left_x.value() - (float)L);
    irad.top_left_y     = std::max(0.0f, orad.top_left_y.value() - (float)T);
    irad.top_right_x    = std::max(0.0f, orad.top_right_x.value() - (float)Rr);
    irad.top_right_y    = std::max(0.0f, orad.top_right_y.value() - (float)T);
    irad.bottom_right_x = std::max(0.0f, orad.bottom_right_x.value() - (float)Rr);
    irad.bottom_right_y = std::max(0.0f, orad.bottom_right_y.value() - (float)B);
    irad.bottom_left_x  = std::max(0.0f, orad.bottom_left_x.value() - (float)L);
    irad.bottom_left_y  = std::max(0.0f, orad.bottom_left_y.value() - (float)B);

    CGMutablePathRef ring = CGPathCreateMutable();
    AddRoundedRect(ring, outer, orad);
    if(inner.size.width > 0 && inner.size.height > 0)
        AddRoundedRect(ring, inner, irad);

    CGContextSaveGState(ctx);
    apply_clips(ctx, outer);

    const lh::border* sides[4] = {&borders.top, &borders.right, &borders.bottom, &borders.left};
    bool              uniform  = true;
    for(int i = 1; i < 4; i++)
        uniform = uniform && sides[i]->width == sides[0]->width && sides[i]->color == sides[0]->color &&
                  sides[i]->style == sides[0]->style && visible(*sides[i]) == visible(*sides[0]);
    lh::border_style style = borders.top.style;

    if(uniform && (style == lh::border_style_dotted || style == lh::border_style_dashed))
    {
        CGFloat w = T;
        CGContextAddPath(ctx, ring);
        CGContextEOClip(ctx);
        lh::border_radiuses mrad = orad;
        mrad -= Uniform(w / 2);
        CGMutablePathRef mid = CGPathCreateMutable();
        AddRoundedRect(mid, CGRectInset(outer, w / 2, w / 2), mrad);
        CGFloat dots[]   = {0, w * 2};
        CGFloat dashes[] = {w * 2.5f, w * 1.5f};
        SetStroke(ctx, borders.top.color);
        CGContextSetLineWidth(ctx, w);
        CGContextSetLineCap(ctx, style == lh::border_style_dotted ? kCGLineCapRound : kCGLineCapButt);
        CGContextSetLineDash(ctx, 0, style == lh::border_style_dotted ? dots : dashes, 2);
        CGContextAddPath(ctx, mid);
        CGContextStrokePath(ctx);
        CGPathRelease(mid);
    }
    else if(uniform && style == lh::border_style_double && T >= 3)
    {
        CGFloat          t3 = T / 3;
        CGMutablePathRef p  = CGPathCreateMutable();
        AddRoundedRect(p, outer, orad);
        lh::border_radiuses r1 = orad;
        r1 -= Uniform(t3);
        AddRoundedRect(p, CGRectInset(outer, t3, t3), r1);
        lh::border_radiuses r2 = orad;
        r2 -= Uniform(t3 * 2);
        AddRoundedRect(p, CGRectInset(outer, t3 * 2, t3 * 2), r2);
        if(inner.size.width > 0 && inner.size.height > 0)
            AddRoundedRect(p, inner, irad);
        SetFill(ctx, borders.top.color);
        CGContextAddPath(ctx, p);
        CGContextEOFillPath(ctx);
        CGPathRelease(p);
    }
    else if(uniform && (style == lh::border_style_solid || style == lh::border_style_double))
    {
        SetFill(ctx, borders.top.color);
        CGContextAddPath(ctx, ring);
        CGContextEOFillPath(ctx);
    }
    else
    {
        CGContextAddPath(ctx, ring);
        CGContextEOClip(ctx);
        CGFloat ox0 = CGRectGetMinX(outer), oy0 = CGRectGetMinY(outer);
        CGFloat ox1 = CGRectGetMaxX(outer), oy1 = CGRectGetMaxY(outer);
        CGFloat ix0 = ox0 + L, iy0 = oy0 + T, ix1 = ox1 - Rr, iy1 = oy1 - B;
        struct Side
        {
            const lh::border* b;
            CGFloat           w;
            CGPoint           poly[4];
            CGPoint           a, z;
            bool              lightSide;
        } list[4] = {
            {&borders.top, T, {{ox0, oy0}, {ox1, oy0}, {ix1, iy0}, {ix0, iy0}}, {ox0, oy0 + T / 2}, {ox1, oy0 + T / 2}, true},
            {&borders.right, Rr, {{ox1, oy0}, {ox1, oy1}, {ix1, iy1}, {ix1, iy0}}, {ox1 - Rr / 2, oy0}, {ox1 - Rr / 2, oy1}, false},
            {&borders.bottom, B, {{ox1, oy1}, {ox0, oy1}, {ix0, iy1}, {ix1, iy1}}, {ox0, oy1 - B / 2}, {ox1, oy1 - B / 2}, false},
            {&borders.left, L, {{ox0, oy1}, {ox0, oy0}, {ix0, iy0}, {ix0, iy1}}, {ox0 + L / 2, oy0}, {ox0 + L / 2, oy1}, true},
        };
        for(auto& s : list)
        {
            if(s.w <= 0)
                continue;
            lh::web_color c = s.b->color;
            switch(s.b->style)
            {
            case lh::border_style_inset:
            case lh::border_style_groove:
                if(s.lightSide)
                    c = c.darken(0.35);
                break;
            case lh::border_style_outset:
            case lh::border_style_ridge:
                if(!s.lightSide)
                    c = c.darken(0.35);
                break;
            default:
                break;
            }
            CGContextSaveGState(ctx);
            CGContextBeginPath(ctx);
            CGContextAddLines(ctx, s.poly, 4);
            CGContextClosePath(ctx);
            if(s.b->style == lh::border_style_dotted || s.b->style == lh::border_style_dashed)
            {
                CGContextClip(ctx);
                CGFloat dots[]   = {0, s.w * 2};
                CGFloat dashes[] = {s.w * 2.5, s.w * 1.5};
                bool    dotted   = s.b->style == lh::border_style_dotted;
                SetStroke(ctx, c);
                CGContextSetLineWidth(ctx, s.w);
                CGContextSetLineCap(ctx, dotted ? kCGLineCapRound : kCGLineCapButt);
                CGContextSetLineDash(ctx, 0, dotted ? dots : dashes, 2);
                CGContextMoveToPoint(ctx, s.a.x, s.a.y);
                CGContextAddLineToPoint(ctx, s.z.x, s.z.y);
                CGContextStrokePath(ctx);
            }
            else
            {
                SetFill(ctx, c);
                CGContextFillPath(ctx);
            }
            CGContextRestoreGState(ctx);
        }
    }
    CGContextRestoreGState(ctx);
    CGPathRelease(ring);
}

bool LHContainer::draw_cached(lh::uint_ptr hdc, const lh::position& box, uint64_t key, const std::function<void(lh::uint_ptr)>& paint)
{
    if(m_painter || m_runs || m_capture || m_hidden_depth > 0)
        return false;
    CGRect r = R(box);
    if(r.size.width <= 0 || r.size.height <= 0)
        return false;
    if(m_ops)
    {
        OpHash hash;
        hash.integer(77);
        hash.integer(static_cast<intptr_t>(key));
        hash.box(box);
        record_op(hash, r);
        return true;
    }
    auto ctx = reinterpret_cast<CGContextRef>(hdc);
    if(!ctx)
        return false;
    CGRect device   = CGRectIntegral(CGContextConvertRectToDeviceSpace(ctx, r));
    CGRect integral = CGContextConvertRectToUserSpace(ctx, device);
    std::string cacheKey = "svg:";
    AppendInteger(cacheKey, static_cast<uintptr_t>(key));
    AppendNumber(cacheKey, integral.origin.x - r.origin.x);
    AppendNumber(cacheKey, integral.origin.y - r.origin.y);
    return DrawRasterized(ctx, integral, cacheKey, [&](CGContextRef c) { paint(reinterpret_cast<lh::uint_ptr>(c)); });
}

void LHContainer::draw_isolated(lh::uint_ptr hdc, const lh::position& box, const std::function<void(lh::uint_ptr)>& paint)
{
    auto ctx = reinterpret_cast<CGContextRef>(hdc);
    if(m_painter || m_runs || m_ops || !ctx)
    {
        paint(hdc);
        return;
    }
    CGRect integral = CGContextConvertRectToUserSpace(ctx, CGRectIntegral(CGContextConvertRectToDeviceSpace(ctx, R(box))));
    if(DrawRasterized(ctx, integral, std::string(), [&](CGContextRef c) { paint(reinterpret_cast<lh::uint_ptr>(c)); }, false))
        return;
    CGContextSaveGState(ctx);
    CGContextClipToRect(ctx, integral);
    CGContextBeginTransparencyLayer(ctx, nullptr);
    paint(hdc);
    CGContextEndTransparencyLayer(ctx);
    CGContextRestoreGState(ctx);
}

void LHContainer::draw_box_shadow(lh::uint_ptr hdc, const lh::position& box, const lh::border_radiuses& radius,
                                  const lh::shadow_paint& shadow)
{
    if(m_painter)
    {
        m_painter->boxShadow(box, radius, shadow);
        return;
    }
    if(m_runs || shadow.color.alpha == 0)
        return;
    CGFloat dx = F(shadow.x), dy = F(shadow.y), blur = F(shadow.blur), spread = F(shadow.spread);
    CGRect  r     = R(box);
    CGFloat reach = blur + std::fabs(spread) + std::max(std::fabs(dx), std::fabs(dy)) + 2;
    CGRect  ink   = shadow.inset ? r : CGRectInset(r, -reach, -reach);
    if(m_ops)
    {
        OpHash hash;
        hash.integer(9);
        hash.box(box);
        hash.radius(radius);
        for(CGFloat v : {dx, dy, blur, spread})
            hash.number(v);
        hash.color(shadow.color);
        hash.integer(shadow.inset);
        record_op(hash, ink);
        if(!m_capture)
            return;
        hdc = reinterpret_cast<lh::uint_ptr>(m_capture);
    }
    auto ctx = reinterpret_cast<CGContextRef>(hdc);
    if(!ctx || r.size.width <= 0 || r.size.height <= 0)
        return;

    lh::border_radiuses shape_radius = radius;
    CGRect              shape        = r;
    if(shadow.inset)
    {
        shape = CGRectInset(r, spread, spread);
        shape_radius -= Uniform(spread);
    } else
    {
        shape = CGRectInset(r, -spread, -spread);
        if(HasRadius(radius))
            shape_radius += Uniform(spread);
    }

    CGContextSaveGState(ctx);
    apply_clips(ctx, ink);
    CGFloat far = CGRectGetWidth(ink) + reach * 4 + 1000;
    CGMutablePathRef path = CGPathCreateMutable();
    if(shadow.inset)
    {
        CGMutablePathRef clip = CGPathCreateMutable();
        AddRoundedRect(clip, r, radius);
        CGContextAddPath(ctx, clip);
        CGContextClip(ctx);
        CGPathRelease(clip);
        CGRect frame = CGRectInset(r, -reach * 2, -reach * 2);
        CGPathAddRect(path, nullptr, CGRectOffset(frame, far, 0));
        if(shape.size.width > 0 && shape.size.height > 0)
            AddRoundedRect(path, CGRectOffset(shape, far, 0), shape_radius);
    } else
    {
        CGMutablePathRef clip = CGPathCreateMutable();
        CGPathAddRect(clip, nullptr, CGRectInset(ink, -1, -1));
        AddRoundedRect(clip, r, radius);
        CGContextAddPath(ctx, clip);
        CGContextEOClip(ctx);
        CGPathRelease(clip);
        AddRoundedRect(path, CGRectOffset(shape, far, 0), shape_radius);
    }
    CGAffineTransform ctm    = CGContextGetCTM(ctx);
    CGSize            offset = CGSizeApplyAffineTransform(CGSizeMake(dx - far, dy), ctm);
    CGFloat           scale  = std::sqrt(std::fabs(ctm.a * ctm.d - ctm.b * ctm.c));
    CGColorRef        color  = CreateColor(shadow.color);
    CGContextSetShadowWithColor(ctx, offset, blur * scale, color);
    CGColorRelease(color);
    CGContextSetRGBFillColor(ctx, 0, 0, 0, 1);
    CGContextAddPath(ctx, path);
    if(shadow.inset)
        CGContextEOFillPath(ctx);
    else
        CGContextFillPath(ctx);
    CGPathRelease(path);
    CGContextRestoreGState(ctx);
}

static CGPathRef CreateVectorPath(const lh::vector_path& path)
{
    CGMutablePathRef out = CGPathCreateMutable();
    const float*     p   = path.points.data();
    size_t           n   = path.points.size();
    size_t           i   = 0;
    bool             open = false;
    for(auto op : path.ops)
    {
        size_t need = op == lh::vector_path::cubic ? 6 : op == lh::vector_path::quad ? 4 : op == lh::vector_path::close ? 0 : 2;
        if(i + need > n)
            break;
        switch(op)
        {
        case lh::vector_path::move:
            CGPathMoveToPoint(out, nullptr, p[i], p[i + 1]);
            open = true;
            break;
        case lh::vector_path::line:
            if(open)
                CGPathAddLineToPoint(out, nullptr, p[i], p[i + 1]);
            break;
        case lh::vector_path::quad:
            if(open)
                CGPathAddQuadCurveToPoint(out, nullptr, p[i], p[i + 1], p[i + 2], p[i + 3]);
            break;
        case lh::vector_path::cubic:
            if(open)
                CGPathAddCurveToPoint(out, nullptr, p[i], p[i + 1], p[i + 2], p[i + 3], p[i + 4], p[i + 5]);
            break;
        case lh::vector_path::close:
            if(open)
                CGPathCloseSubpath(out);
            break;
        }
        i += need;
    }
    return out;
}

void LHContainer::draw_path(lh::uint_ptr hdc, const lh::vector_path& path)
{
    if(m_painter)
    {
        m_painter->path(path);
        return;
    }
    if(m_runs || (!path.fill && !path.stroke))
        return;
    CGPathRef cgpath = CreateVectorPath(path);
    CGRect    box    = CGPathGetPathBoundingBox(cgpath);
    if(CGRectIsNull(box))
    {
        CGPathRelease(cgpath);
        return;
    }
    CGFloat reach = path.stroke ? path.stroke_width * std::max<CGFloat>(path.miter_limit, 1) / 2 + 1 : 1;
    CGRect  ink   = CGRectInset(box, -reach, -reach);
    if(m_ops)
    {
        OpHash hash;
        hash.integer(10);
        for(auto op : path.ops)
            hash.integer(op);
        for(float v : path.points)
            hash.number(v);
        hash.integer(path.fill);
        hash.color(path.fill_color);
        hash.integer(path.evenodd);
        hash.integer(path.stroke);
        hash.color(path.stroke_color);
        hash.number(path.stroke_width);
        hash.integer(path.line_cap | path.line_join << 4);
        hash.number(path.miter_limit);
        for(float v : path.dashes)
            hash.number(v);
        hash.number(path.dash_offset);
        if(path.fill_gradient.type != 0)
        {
            const auto& g = path.fill_gradient;
            hash.integer(g.type);
            for(float v : {g.x1, g.y1, g.x2, g.y2, g.r, g.fx, g.fy})
                hash.number(v);
            for(const auto& stop : g.stops)
            {
                hash.number(stop.first);
                hash.color(stop.second);
            }
        }
        record_op(hash, ink);
        if(!m_capture)
        {
            CGPathRelease(cgpath);
            return;
        }
        hdc = reinterpret_cast<lh::uint_ptr>(m_capture);
    }
    auto ctx = reinterpret_cast<CGContextRef>(hdc);
    if(!ctx)
    {
        CGPathRelease(cgpath);
        return;
    }
    CGContextSaveGState(ctx);
    apply_clips(ctx, ink);
    if(path.fill && path.fill_gradient.type != 0)
    {
        std::vector<CGColorRef> colors;
        std::vector<CGFloat>    locations;
        for(const auto& stop : path.fill_gradient.stops)
        {
            colors.push_back(CreateColor(stop.second));
            locations.push_back(stop.first);
        }
        CFArrayRef  array    = CFArrayCreate(nullptr, (const void**)colors.data(), colors.size(), &kCFTypeArrayCallBacks);
        CGGradientRef gradient = CGGradientCreateWithColors(SRGB(), array, locations.data());
        CFRelease(array);
        for(CGColorRef color : colors)
            CGColorRelease(color);
        if(gradient)
        {
            const auto& g = path.fill_gradient;
            CGContextSaveGState(ctx);
            CGContextAddPath(ctx, cgpath);
            if(path.evenodd)
                CGContextEOClip(ctx);
            else
                CGContextClip(ctx);
            const CGGradientDrawingOptions options = kCGGradientDrawsBeforeStartLocation | kCGGradientDrawsAfterEndLocation;
            if(g.type == 1)
                CGContextDrawLinearGradient(ctx, gradient, CGPointMake(g.x1, g.y1), CGPointMake(g.x2, g.y2), options);
            else
                CGContextDrawRadialGradient(ctx, gradient, CGPointMake(g.fx, g.fy), 0, CGPointMake(g.x1, g.y1), g.r, options);
            CGContextRestoreGState(ctx);
            CGGradientRelease(gradient);
        }
    } else if(path.fill && path.fill_color.alpha > 0)
    {
        SetFill(ctx, path.fill_color);
        CGContextAddPath(ctx, cgpath);
        if(path.evenodd)
            CGContextEOFillPath(ctx);
        else
            CGContextFillPath(ctx);
    }
    if(path.stroke && path.stroke_color.alpha > 0)
    {
        SetStroke(ctx, path.stroke_color);
        CGContextSetLineWidth(ctx, path.stroke_width);
        CGContextSetLineCap(ctx, path.line_cap == 1 ? kCGLineCapRound : path.line_cap == 2 ? kCGLineCapSquare : kCGLineCapButt);
        CGContextSetLineJoin(ctx, path.line_join == 1 ? kCGLineJoinRound : path.line_join == 2 ? kCGLineJoinBevel : kCGLineJoinMiter);
        CGContextSetMiterLimit(ctx, path.miter_limit);
        bool dashed = false;
        for(float v : path.dashes)
            dashed = dashed || v > 0;
        if(dashed)
        {
            std::vector<CGFloat> lengths(path.dashes.begin(), path.dashes.end());
            CGContextSetLineDash(ctx, path.dash_offset, lengths.data(), lengths.size());
        }
        CGContextAddPath(ctx, cgpath);
        CGContextStrokePath(ctx);
    }
    CGContextRestoreGState(ctx);
    CGPathRelease(cgpath);
}

void LHContainer::begin_group(lh::uint_ptr hdc, const lh::paint_group& group)
{
    if(m_painter)
    {
        m_painter->beginGroup(group);
        return;
    }
    const auto&       m   = group.matrix;
    CGAffineTransform own = CGAffineTransformMake(m.a, m.b, m.c, m.d, m.tx, m.ty);
    Group             g;
    g.matrix  = CGAffineTransformConcat(own, group_matrix());
    g.opacity = group.opacity;
    g.hidden  = group.backface_hidden && (g.matrix.a * g.matrix.d - g.matrix.b * g.matrix.c) < 0;
    g.raster  = !m_ops && !m_runs && hdc;
    g.layer   = false;
    if(g.hidden)
        m_hidden_depth++;
    if(g.raster)
    {
        auto ctx = reinterpret_cast<CGContextRef>(hdc);
        CGContextSaveGState(ctx);
        if(g.hidden)
            CGContextClipToRect(ctx, CGRectZero);
        CGContextConcatCTM(ctx, own);
        if(group.opacity < 1 && !g.hidden && m_hidden_depth == 0)
        {
            CGContextSetAlpha(ctx, group.opacity);
            CGRect layerRect = CGRectIntersection(CGRectInset(R(group.bounds), -64, -64), CGContextGetClipBoundingBox(ctx));
            CGContextBeginTransparencyLayerWithRect(ctx, CGRectIsNull(layerRect) ? CGRectZero : layerRect, nullptr);
            g.layer = true;
        }
    }
    m_groups.push_back(g);
}

void LHContainer::end_group(lh::uint_ptr hdc)
{
    if(m_painter)
    {
        m_painter->endGroup();
        return;
    }
    if(m_groups.empty())
        return;
    Group g = m_groups.back();
    m_groups.pop_back();
    if(g.raster)
    {
        auto ctx = reinterpret_cast<CGContextRef>(hdc);
        if(g.layer)
            CGContextEndTransparencyLayer(ctx);
        CGContextRestoreGState(ctx);
    }
    if(g.hidden)
        m_hidden_depth--;
}

void LHContainer::on_anchor_click(const char* url, const lh::element::ptr& el)
{
    if(on_element_click(el))
        return;
    if(on_anchor && url)
        on_anchor(url);
}

bool LHContainer::on_element_click(const lh::element::ptr& el)
{
    const char* action = el && on_action ? el->get_attr("data-action") : nullptr;
    if(!action)
        return false;
    on_action(action, el->get_attr("data-value", ""));
    return true;
}

void LHContainer::set_cursor(const char* cursor)
{
    if(on_cursor)
        on_cursor(cursor ? cursor : "auto");
}

void LHContainer::get_viewport(lh::position& viewport) const
{
    viewport = m_viewport;
}

void LHContainer::get_media_features(lh::media_features& media) const
{
    media.type          = lh::media_type_screen;
    media.width         = m_viewport.width;
    media.height        = m_viewport.height;
    media.device_width  = 1440;
    media.device_height = 900;
    media.color         = 8;
    media.monochrome    = 0;
    media.color_index   = 256;
    media.resolution    = 96;
}

void LHContainer::get_language(std::string& language, std::string& culture) const
{
    language = "en";
    culture  = "";
}
