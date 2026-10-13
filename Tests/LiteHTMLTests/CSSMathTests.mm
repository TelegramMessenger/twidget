#import <XCTest/XCTest.h>
#import "LHDocument.h"

#include <vector>

@interface CSSMathTests : XCTestCase
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
}

@implementation CSSMathTests

- (void)testCalcWithNestedMaxAndEnvFallbackResolvesPadding
{
    Bitmap b = Draw(@"<div style=\"padding:0 0 0 calc(10px + max(0px, env(safe-area-inset-left, 0px)) + 5px)\"><div style=\"height:10px;background:#f00\"></div></div>", 100);
    XCTAssertGreaterThanOrEqual(b.height, 10u);
    XCTAssertEqual(b.at(13, 5)[3], 0);
    XCTAssertEqual(b.at(16, 5)[0], 255);
    XCTAssertEqual(b.at(16, 5)[3], 255);
}

- (void)testEnvWithoutFallbackIsZeroForSafeAreaInsets
{
    Bitmap b = Draw(@"<div style=\"margin-left:calc(env(safe-area-inset-left) + 20px);height:10px;background:#f00\"></div>", 100);
    XCTAssertEqual(b.at(18, 5)[3], 0);
    XCTAssertEqual(b.at(22, 5)[3], 255);
}

- (void)testCalcWithCustomPropertiesAndClamp
{
    Bitmap b = Draw(@"<div style=\"--a:4px;margin-left:calc(var(--a) * 2 + 2px);width:clamp(5px, 100px, 20px);height:10px;background:#f00\"></div>", 100);
    XCTAssertEqual(b.at(8, 5)[3], 0);
    XCTAssertEqual(b.at(12, 5)[3], 255);
    XCTAssertEqual(b.at(28, 5)[3], 255);
    XCTAssertEqual(b.at(32, 5)[3], 0);
}

- (void)testMinAndMaxPickTheExtremes
{
    Bitmap b = Draw(@"<div style=\"width:min(40px, 30px, 50px);height:max(4px, 10px);background:#f00\"></div>", 100);
    XCTAssertEqual(b.height, 10u);
    XCTAssertEqual(b.at(28, 5)[3], 255);
    XCTAssertEqual(b.at(32, 5)[3], 0);
}

- (void)testColorMixWithTransparentKeepsTheHueAndScalesAlpha
{
    Bitmap b = Draw(@"<div style=\"height:10px;background:color-mix(in srgb, #ff0000 60%, transparent)\"></div>", 20);
    const uint8_t* p = b.at(10, 5);
    XCTAssertEqualWithAccuracy(p[3], 153, 2);
    XCTAssertEqualWithAccuracy(p[0], 153, 2);
    XCTAssertEqual(p[1], 0);
    XCTAssertEqual(p[2], 0);
}

- (void)testColorMixOfTwoOpaqueColorsAveragesChannels
{
    Bitmap b = Draw(@"<div style=\"height:10px;background:color-mix(in srgb, #ff0000, #0000ff)\"></div>", 20);
    const uint8_t* p = b.at(10, 5);
    XCTAssertEqual(p[3], 255);
    XCTAssertEqualWithAccuracy(p[0], 128, 2);
    XCTAssertEqualWithAccuracy(p[2], 128, 2);
}

- (void)testColorMixInsideGradientFolds
{
    Bitmap b = Draw(@"<div style=\"height:10px;background:linear-gradient(color-mix(in srgb, #0000ff 50%, #0000ff), #0000ff)\"></div>", 20);
    XCTAssertEqual(b.at(10, 5)[2], 255);
    XCTAssertEqual(b.at(10, 5)[3], 255);
}

- (void)testMixedUnitCalcIsLeftUnresolved
{
    Bitmap b = Draw(@"<div style=\"width:30px;width:calc(100% - 10px);height:10px;background:#f00\"></div>", 100);
    XCTAssertGreaterThanOrEqual(b.height, 10u);
}

@end
