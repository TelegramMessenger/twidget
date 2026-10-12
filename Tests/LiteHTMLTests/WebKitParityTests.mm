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
        LHDocument* doc  = [[LHDocument alloc] initWithHTML:[@"<style>body{margin:0}</style>" stringByAppendingString:html] css:nil];
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

- (void)testStylesheetFillReferencesPaintServers
{
    NSString* html = @"<style>.a{fill:url(#g)}.b{fill:url(#missing)}.c{fill:url(#missing) red}</style>"
                     @"<svg width=\"300\" height=\"60\" style=\"display:block\"><defs><linearGradient id=\"g\"><stop offset=\"0\" stop-color=\"#00f\"/><stop offset=\"1\" stop-color=\"#00f\"/></linearGradient></defs>"
                     @"<rect class=\"a\" x=\"0\" width=\"80\" height=\"60\"/><rect class=\"b\" x=\"100\" width=\"80\" height=\"60\"/><rect class=\"c\" x=\"200\" width=\"80\" height=\"60\"/></svg>";
    Pixels p = Render(html, 300);
    XCTAssertEqual(p.at(80, 60)[2], 255);
    XCTAssertEqual(p.at(80, 60)[3], 255);
    XCTAssertEqual(p.at(280, 60)[3], 0);
    XCTAssertEqual(p.at(480, 60)[0], 255);
    XCTAssertEqual(p.at(480, 60)[3], 255);
}

- (void)testTableHeadersCenterUnlessTheRowAlignsAndTextAlignEndAlignsRight
{
    NSString* box = @"<span style=\"display:inline-block;width:20px;height:10px;background:#f00;vertical-align:top\"></span>";
    NSString* table = @"<table style=\"width:200px;border-spacing:0;%@\"><tr><th style=\"padding:0;line-height:10px\">%@</th></tr></table>";
    Pixels centered = Render([NSString stringWithFormat:table, @"", box], 200);
    XCTAssertEqual(centered.at(200, 10)[3], 255);
    XCTAssertEqual(centered.at(20, 10)[3], 0);
    Pixels left = Render([NSString stringWithFormat:table, @"text-align:left", box], 200);
    XCTAssertEqual(left.at(20, 10)[3], 255);
    XCTAssertEqual(left.at(200, 10)[3], 0);
    Pixels end = Render([NSString stringWithFormat:@"<div style=\"width:200px;text-align:end;line-height:10px\">%@</div>", box], 200);
    XCTAssertEqual(end.at(380, 10)[3], 255);
    XCTAssertEqual(end.at(20, 10)[3], 0);
}

- (void)testWordsKeepFractionalPositionsLikeOneShapedRun
{
    Pixels p = Render(@"<div style=\"font:15px -apple-system;line-height:20px;white-space:nowrap\">l l l l l l l l l l l l</div>", 400);
    std::vector<double> centroids;
    double              sum = 0, weighted = 0;
    for(size_t x = 0; x < p.width; x++)
    {
        double column = 0;
        for(size_t y = 0; y < p.height; y++)
            column += p.at(x, y)[3];
        if(column > 0)
        {
            sum += column;
            weighted += column * x;
        } else if(sum > 0)
        {
            centroids.push_back(weighted / sum);
            sum = weighted = 0;
        }
    }
    XCTAssertEqual(centroids.size(), 12u);
    bool fractional = false;
    for(size_t i = 1; i < centroids.size(); i++)
    {
        double step = centroids[i] - centroids[i - 1];
        fractional  = fractional || std::fabs(step - std::round(step)) > 0.3;
    }
    XCTAssertTrue(fractional);
}

@end
