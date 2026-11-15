#import <XCTest/XCTest.h>
#import <ImageIO/ImageIO.h>
#import "LHDocument.h"
#import "LHImages.h"
#import "LHSanitizer.h"

namespace
{
NSData* Encode(CFStringRef type, size_t width, size_t height)
{
    CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef    ctx   = CGBitmapContextCreate(nullptr, width, height, 8, 0, space, kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
    CGContextSetRGBFillColor(ctx, 0.2, 0.6, 0.9, 1);
    CGContextFillRect(ctx, CGRectMake(0, 0, width, height));
    CGImageRef image = CGBitmapContextCreateImage(ctx);
    CGContextRelease(ctx);
    CGColorSpaceRelease(space);
    NSMutableData*          out  = [NSMutableData data];
    CGImageDestinationRef   dest = CGImageDestinationCreateWithData((__bridge CFMutableDataRef)out, type, 1, nullptr);
    if(!dest)
    {
        CGImageRelease(image);
        return nil;
    }
    CGImageDestinationAddImage(dest, image, nullptr);
    bool done = CGImageDestinationFinalize(dest);
    CFRelease(dest);
    CGImageRelease(image);
    return done ? out : nil;
}

NSString* DataURL(NSString* mime, NSData* data, NSString* tag)
{
    return [NSString stringWithFormat:@"data:%@;t=%@;base64,%@", mime, tag, [data base64EncodedStringWithOptions:0]];
}

NSData* PNGHeaderClaiming(uint32_t width, uint32_t height)
{
    uint8_t bytes[] = {0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, 'I', 'H', 'D', 'R',
                       uint8_t(width >> 24), uint8_t(width >> 16), uint8_t(width >> 8), uint8_t(width),
                       uint8_t(height >> 24), uint8_t(height >> 16), uint8_t(height >> 8), uint8_t(height),
                       8, 6, 0, 0, 0, 0, 0, 0, 0,
                       0, 0, 0, 0, 'I', 'E', 'N', 'D', 0xAE, 0x42, 0x60, 0x82};
    return [NSData dataWithBytes:bytes length:sizeof(bytes)];
}

bool Loads(NSString* url)
{
    CGImageRef image = [LHImages copyImageForURL:url];
    if(image)
        CGImageRelease(image);
    return image != nullptr;
}
}

@interface ImageDecodeHardeningTests : XCTestCase
@end

@implementation ImageDecodeHardeningTests

- (void)tearDown
{
    [LHSanitizer setLinkSchemes:@[@"http", @"https", @"mailto"] imageSchemes:@[]];
}

- (void)testAllowedRasterFormatsStillLoad
{
    NSData* png  = Encode(CFSTR("public.png"), 8, 6);
    NSData* jpeg = Encode(CFSTR("public.jpeg"), 8, 6);
    NSData* gif  = Encode(CFSTR("com.compuserve.gif"), 8, 6);
    NSData* webp = [[NSData alloc] initWithBase64EncodedString:@"UklGRhoAAABXRUJQVlA4TA0AAAAvAAAAEAcQERGIiP4HAA==" options:0];
    XCTAssertTrue(Loads(DataURL(@"image/png", png, @"ok-png")));
    XCTAssertTrue(Loads(DataURL(@"image/jpeg", jpeg, @"ok-jpeg")));
    XCTAssertTrue(Loads(DataURL(@"image/gif", gif, @"ok-gif")));
    XCTAssertTrue(Loads(DataURL(@"image/webp", webp, @"ok-webp")));
    XCTAssertTrue(CGSizeEqualToSize([LHImages sizeForURL:DataURL(@"image/png", png, @"ok-png")], CGSizeMake(8, 6)));
    XCTAssertTrue(Loads(DataURL(@"image/png", Encode(CFSTR("public.png"), 1024, 1024), @"ok-edge")));
}

- (void)testOtherImageIOFormatsAreRefusedWhateverTheDeclaredType
{
    NSData* bmp  = Encode(CFSTR("com.microsoft.bmp"), 8, 6);
    NSData* ico  = Encode(CFSTR("com.microsoft.ico"), 16, 16);
    NSData* tiff = Encode(CFSTR("public.tiff"), 8, 6);
    XCTAssertNotNil(bmp);
    XCTAssertNotNil(tiff);
    for(NSString* mime in @[@"image/png", @"image/bmp", @"image/jpeg", @"image/webp"])
    {
        XCTAssertFalse(Loads(DataURL(mime, bmp, [@"bmp-" stringByAppendingString:[mime substringFromIndex:6]])), @"%@", mime);
        XCTAssertFalse(Loads(DataURL(mime, tiff, [@"tiff-" stringByAppendingString:[mime substringFromIndex:6]])), @"%@", mime);
        if(ico)
            XCTAssertFalse(Loads(DataURL(mime, ico, [@"ico-" stringByAppendingString:[mime substringFromIndex:6]])), @"%@", mime);
    }
    uint8_t avif[] = {0, 0, 0, 0x1C, 'f', 't', 'y', 'p', 'a', 'v', 'i', 'f', 0, 0, 0, 0, 'a', 'v', 'i', 'f', 'm', 'i', 'f', '1', 'm', 'i', 'a', 'f'};
    XCTAssertFalse(Loads(DataURL(@"image/avif", [NSData dataWithBytes:avif length:sizeof(avif)], @"avif")));
    XCTAssertFalse(Loads(DataURL(@"image/png", [NSData dataWithBytes:avif length:sizeof(avif)], @"avif-as-png")));
    NSData* svg = [@"<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'><rect width='8' height='8'/></svg>" dataUsingEncoding:NSUTF8StringEncoding];
    XCTAssertFalse(Loads(DataURL(@"image/png", svg, @"svg-as-png")));
}

- (void)testMislabelledAllowedBytesDecodeByTheirMagic
{
    NSData* png = Encode(CFSTR("public.png"), 5, 5);
    XCTAssertTrue(Loads(DataURL(@"image/jpeg", png, @"png-as-jpeg")));
    XCTAssertTrue(Loads(DataURL(@"image/gif", png, @"png-as-gif")));
    NSData* jpeg  = Encode(CFSTR("public.jpeg"), 5, 5);
    NSMutableData* forged = [NSMutableData dataWithBytes:"RIFF\x10\0\0\0WEBP" length:12];
    [forged appendData:jpeg];
    XCTAssertFalse(Loads(DataURL(@"image/webp", forged, @"jpeg-behind-webp-magic")));
}

- (void)testOversizedImagesAreRefusedFromTheirHeader
{
    XCTAssertFalse(Loads(DataURL(@"image/png", Encode(CFSTR("public.png"), 2000, 600), @"wide")));
    XCTAssertFalse(Loads(DataURL(@"image/png", Encode(CFSTR("public.png"), 1025, 4), @"just-over")));
    XCTAssertFalse(Loads(DataURL(@"image/gif", Encode(CFSTR("com.compuserve.gif"), 4, 1100), @"tall-gif")));
    XCTAssertFalse(Loads(DataURL(@"image/jpeg", Encode(CFSTR("public.jpeg"), 1200, 1200), @"big-jpeg")));
    CFAbsoluteTime started = CFAbsoluteTimeGetCurrent();
    XCTAssertFalse(Loads(DataURL(@"image/png", PNGHeaderClaiming(65535, 65535), @"bomb")));
    XCTAssertFalse(Loads(DataURL(@"image/png", PNGHeaderClaiming(4096, 4096), @"bomb-4k")));
    XCTAssertFalse(Loads(DataURL(@"image/png", PNGHeaderClaiming(0, 10), @"zero")));
    XCTAssertLessThan(CFAbsoluteTimeGetCurrent() - started, 0.5);
}

- (void)testSanitizerAllowsOnlyRasterDataImages
{
    [LHSanitizer setLinkSchemes:@[@"https"] imageSchemes:@[]];
    NSString* png = DataURL(@"image/png", Encode(CFSTR("public.png"), 4, 4), @"s");
    NSMutableString* html = [NSMutableString stringWithFormat:@"<img src=\"%@\">", png];
    for(NSString* mime in @[@"image/svg+xml", @"image/avif", @"image/bmp", @"image/x-icon", @"image/vnd.microsoft.icon", @"image/tiff"])
        [html appendFormat:@"<img src=\"data:%@;base64,AAAA\">", mime];
    for(NSString* mime in @[@"image/jpeg", @"image/jpg", @"image/gif", @"image/webp"])
        [html appendFormat:@"<img src=\"data:%@;base64,AAAA\">", mime];
    LHDocument* document = [[LHDocument alloc] initWithHTML:html css:nil];
    [document layoutWithMaxWidth:300];
    XCTAssertEqual(document.sanitizerCounts[@"url-blocked"].intValue, 6, @"%@", document.sanitizerDetails);
}

@end
