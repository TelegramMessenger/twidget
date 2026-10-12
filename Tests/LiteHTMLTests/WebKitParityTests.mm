#import <XCTest/XCTest.h>
#import "LHDocument.h"

#include <vector>

@interface WebKitParityTests : XCTestCase
@end

namespace
{
    struct Pixels
    {
        size_t               width = 0, height = 0;
        std::vector<uint8_t> rgba;

        const uint8_t* at(size_t x, size_t y) const { return &rgba[(y * width + x) * 4]; }
        double alphaSum() const
        {
            double sum = 0;
            for(size_t i = 3; i < rgba.size(); i += 4)
                sum += rgba[i];
            return sum / 255;
        }
    };

    Pixels Render(NSString* html, CGFloat width, CGFloat* heightOut = nullptr)
    {
        LHDocument* doc  = [[LHDocument alloc] initWithHTML:html css:nil];
        CGSize      size = [doc layoutWithMaxWidth:width];
        if(heightOut)
            *heightOut = size.height;
        Pixels     out;
        CGImageRef image = [doc copyImageWithMaxWidth:width rect:CGRectMake(0, 0, width, std::max<CGFloat>(1, size.height)) scale:2];
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

@implementation WebKitParityTests

- (void)testFontSmoothingDilatesGlyphsUnlessAntialiased
{
    NSString* text = @"<p style=\"margin:0;font:15px -apple-system\">Bom dia! Checked us all in</p>";
    double    smooth = Render(text, 300).alphaSum();
    double    antialiased =
        Render([NSString stringWithFormat:@"<div style=\"-webkit-font-smoothing:antialiased\">%@</div>", text], 300).alphaSum();
    double inherited = Render([NSString stringWithFormat:@"<div style=\"-webkit-font-smoothing:antialiased\"><div style=\"-webkit-font-smoothing:auto\">%@</div></div>", text], 300).alphaSum();
    XCTAssertGreaterThan(antialiased, 0);
    XCTAssertGreaterThan(smooth, antialiased * 1.08);
    XCTAssertEqualWithAccuracy(inherited, smooth, 0.5);
}

@end
