#import <XCTest/XCTest.h>
#import "LHDocument.h"
#import "LHDocumentPainting.h"

#include <string>
#include <vector>

@interface FormRenderingTests : XCTestCase
@end

namespace
{
    struct Bitmap
    {
        size_t               width = 0, height = 0;
        std::vector<uint8_t> rgba;

        const uint8_t* at(size_t x, size_t y) const { return &rgba[(y * width + x) * 4]; }
    };

    Bitmap Draw(NSString* html, CGFloat width)
    {
        LHDocument* doc  = [[LHDocument alloc] initWithHTML:[@"<!doctype html><style>body{margin:0}</style>" stringByAppendingString:html] css:nil];
        CGSize      size = [doc layoutWithMaxWidth:width preferredWidth:width];
        Bitmap      out;
        CGImageRef  image = [doc copyImageWithMaxWidth:width rect:CGRectMake(0, 0, width, std::max<CGFloat>(1, size.height)) scale:1];
        if(!image)
            return out;
        out.width  = CGImageGetWidth(image);
        out.height = CGImageGetHeight(image);
        out.rgba.assign(out.width * out.height * 4, 0);
        CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        CGContextRef    ctx   = CGBitmapContextCreate(out.rgba.data(), out.width, out.height, 8, out.width * 4, space,
                                                      kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
        CGContextDrawImage(ctx, CGRectMake(0, 0, out.width, out.height), image);
        CGContextRelease(ctx);
        CGColorSpaceRelease(space);
        CGImageRelease(image);
        return out;
    }

    struct TextPainter : lhx::Painter
    {
        std::vector<std::string>        texts;
        std::vector<litehtml::web_color> colors;
        void text(const char* text, const lhx::TextStyle& style, const litehtml::web_color& color, const litehtml::position& pos) override
        {
            texts.emplace_back(text);
            colors.push_back(color);
        }
        void solidFill(const litehtml::background_layer& layer, const litehtml::web_color& color) override {}
        void path(const litehtml::vector_path& path) override {}
    };

    TextPainter Paint(NSString* html)
    {
        LHDocument* doc = [[LHDocument alloc] initWithHTML:[@"<!doctype html><style>body{margin:0}</style>" stringByAppendingString:html] css:nil];
        TextPainter painter;
        [doc paintWithMaxWidth:400 clip:CGRectMake(0, 0, 400, 400) painter:painter];
        return painter;
    }

    std::string Joined(const TextPainter& p)
    {
        std::string all;
        for(const auto& t : p.texts)
            all += t;
        return all;
    }
}

@implementation FormRenderingTests

- (void)testStyledRangeDrawsAuthorTrackAndThumbAtTheValue
{
    NSString* css = @"<style>.r{display:block;width:200px;height:28px;margin:0;-webkit-appearance:none;appearance:none;background:none}"
                    @".r::-webkit-slider-runnable-track{height:8px;background:#00f}"
                    @".r::-webkit-slider-thumb{width:20px;height:20px;margin-top:-6px;background:#f00}</style>";
    Bitmap b = Draw([css stringByAppendingString:@"<input class=r type=range min=0 max=100 value=50>"], 220);
    XCTAssertGreaterThanOrEqual(b.height, 28u);
    XCTAssertEqual(b.at(10, 14)[2], 255);
    XCTAssertEqual(b.at(10, 14)[0], 0);
    XCTAssertEqual(b.at(100, 14)[0], 255);
    XCTAssertEqual(b.at(100, 6)[0], 255);
    XCTAssertEqual(b.at(100, 3)[3], 0);
    XCTAssertEqual(b.at(150, 14)[0], 0);
}

- (void)testTextareaPlaceholderTakesThePlaceholderPseudoElementColor
{
    TextPainter p = Paint(@"<style>textarea::placeholder{color:#c000c0}</style><textarea placeholder='Message'></textarea>");
    XCTAssertEqual(Joined(p), "Message");
    XCTAssertEqual(p.colors.size(), 1u);
    XCTAssertEqual(p.colors[0].red, 0xc0);
    XCTAssertEqual(p.colors[0].green, 0);
}

- (void)testTextareaPlaceholderHidesOnceThereIsText
{
    XCTAssertEqual(Joined(Paint(@"<textarea placeholder='Message'>hi</textarea>")), "hi");
}

- (void)testResizeNoneHidesTheTextareaGrip
{
    NSString* box = @"<textarea style='display:block;width:100px;height:40px;border:0;padding:0;background:#fff;%@'></textarea>";
    Bitmap grip   = Draw([NSString stringWithFormat:box, @""], 120);
    Bitmap none   = Draw([NSString stringWithFormat:box, @"resize:none"], 120);
    int    gripInk = 0, noneInk = 0;
    for(size_t y = 26; y < 40; y++)
        for(size_t x = 86; x < 100; x++)
        {
            gripInk += grip.at(x, y)[0] < 200;
            noneInk += none.at(x, y)[0] < 200;
        }
    XCTAssertGreaterThan(gripInk, 0);
    XCTAssertEqual(noneInk, 0);
}

- (void)testDateInputShowsTheValueInLocaleOrderWithoutDashes
{
    std::string text = Joined(Paint(@"<input type=date value='2026-10-11'>"));
    XCTAssertNotEqual(text.find("2026"), std::string::npos);
    XCTAssertNotEqual(text.find("10"), std::string::npos);
    XCTAssertNotEqual(text.find("11"), std::string::npos);
    XCTAssertEqual(text.find("-"), std::string::npos);
}

- (void)testEmptyDateInputShowsThePattern
{
    std::string text = Joined(Paint(@"<input type=date>"));
    XCTAssertNotEqual(text.find("yyyy"), std::string::npos);
}

- (void)testSVGPresentationAttributesResolveCustomProperties
{
    Bitmap b = Draw(@"<div style='--c:#ff0000;--s:#0000ff'><svg width='60' height='20' style='display:block'><rect width='20' height='20' fill='var(--c)'/>"
                    @"<rect x='30' width='20' height='20' fill='none' stroke='var(--s, red)' stroke-width='4'/><rect x='55' width='5' height='20' fill='var(--missing, #00ff00)'/></svg></div>", 60);
    XCTAssertEqual(b.at(10, 10)[0], 255);
    XCTAssertEqual(b.at(10, 10)[3], 255);
    XCTAssertEqual(b.at(30, 10)[2], 255);
    XCTAssertEqual(b.at(40, 10)[3], 0);
    XCTAssertEqual(b.at(57, 10)[1], 255);
}

@end

@interface FormButtonSizingTests : XCTestCase
@end

@implementation FormButtonSizingTests

- (CGRect)bounds:(NSString*)html key:(NSString*)key
{
    LHDocument* document = [[LHDocument alloc] initWithHTML:[@"<!doctype html><style>body{margin:0;font:16px -apple-system}.b{padding:10px 18px;border:0;font-weight:600}</style>" stringByAppendingString:html] css:nil];
    [document layoutWithMaxWidth:400 preferredWidth:400];
    NSValue* value = [document boundsByAttribute:@"data-t"][key];
    XCTAssertNotNil(value, @"%@", key);
    return value.rectValue;
}

- (void)testButtonInAColumnFlexShrinksToItsLabelLikeInline
{
    CGRect inlineBox = [self bounds:@"<button class=b data-t=a>Column flex item</button>" key:@"a"];
    CGRect flexBox   = [self bounds:@"<div style='display:flex;flex-direction:column;align-items:flex-start'><button class=b data-t=a>Column flex item</button></div>" key:@"a"];
    XCTAssertEqualWithAccuracy(flexBox.size.width, inlineBox.size.width, 0.5);
    XCTAssertEqualWithAccuracy(flexBox.size.height, inlineBox.size.height, 0.5);
}

@end
