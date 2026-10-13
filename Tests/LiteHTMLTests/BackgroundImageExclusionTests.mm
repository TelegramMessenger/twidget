#import <XCTest/XCTest.h>
#import "LHDocument.h"

#include <cmath>

@interface BackgroundImageExclusionTests : XCTestCase
@end

@implementation BackgroundImageExclusionTests

static NSData *Tile(LHDocument *document, CGFloat width, CGFloat height)
{
    size_t pw = (size_t)std::ceil(width * 2), ph = (size_t)std::ceil(height * 2);
    CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef ctx = CGBitmapContextCreate(nullptr, pw, ph, 8, pw * 4, space, kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    CGColorSpaceRelease(space);
    CGContextTranslateCTM(ctx, 0, ph);
    CGContextScaleCTM(ctx, 2, -2);
    [document drawInContext:ctx origin:CGPointZero clip:CGRectMake(0, 0, width, height)];
    NSData *data = [NSData dataWithBytes:CGBitmapContextGetData(ctx) length:pw * ph * 4];
    CGContextRelease(ctx);
    return data;
}

static NSData *Pixels(CGImageRef image)
{
    return CFBridgingRelease(CGDataProviderCopyData(CGImageGetDataProvider(image)));
}

- (void)testExcludedGradientLeavesTilesButNotSnapshots
{
    NSString *gradient = @"<style>body{margin:0}.sk{width:200px;height:20px;border-radius:6px;background-color:#ccc;background-image:linear-gradient(90deg,rgba(0,0,0,0),rgba(0,0,0,.5),rgba(0,0,0,0));background-size:40% 100%;background-repeat:no-repeat;background-position:30% 0}</style><div class=\"sk\" data-twid=\"5\"></div>";
    NSString *flat = @"<style>body{margin:0}.sk{width:200px;height:20px;border-radius:6px;background-color:#ccc}</style><div class=\"sk\" data-twid=\"5\"></div>";
    LHDocument *document = [[LHDocument alloc] initWithHTML:gradient css:nil];
    LHDocument *plain = [[LHDocument alloc] initWithHTML:flat css:nil];
    [document layoutWithMaxWidth:220 preferredWidth:220];
    [plain layoutWithMaxWidth:220 preferredWidth:220];
    CGRect rect = CGRectMake(0, 0, 220, 30);
    CGImageRef before = [document copyImageWithMaxWidth:220 rect:rect scale:2];
    NSData *tileBefore = Tile(document, 220, 30);
    [document setBackgroundImageExclusions:[NSSet setWithObject:@"5"] attribute:@"data-twid"];
    NSData *tileAfter = Tile(document, 220, 30);
    CGImageRef after = [document copyImageWithMaxWidth:220 rect:rect scale:2];
    XCTAssertNotEqualObjects(tileBefore, tileAfter);
    XCTAssertEqualObjects(tileAfter, Tile(plain, 220, 30));
    XCTAssertEqualObjects(Pixels(before), Pixels(after));
    CGImageRelease(before);
    CGImageRelease(after);
}

@end
